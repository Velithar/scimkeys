//! scimkeys: reads the Corsair Scimitar PRO RGB's vendor HID report for its
//! 12 side buttons (G1..G12) and emits real key events through a uinput
//! virtual keyboard, driven by a TOML mapping. See README.md for details.

mod config;
mod layout;

use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::{Context, Result};
use clap::Parser;
use config::Config;
use evdev::uinput::VirtualDevice;
use evdev::{AttributeSet, BusType, InputEvent, InputId, KeyCode, KeyEvent};
use layout::{diff_mask, extract_mask};
use log::{debug, error, info, warn};

const VIRTUAL_DEVICE_NAME: &str = "scimkeys virtual keyboard";
const DISCOVERY_RETRY_INTERVAL: Duration = Duration::from_secs(2);
/// Report ID byte for the vendor button report on interface 0.
const BUTTON_REPORT_ID: u8 = 0x03;
/// Report ID byte plus 63 data bytes.
const REPORT_LEN: usize = 64;

#[derive(Parser, Debug)]
#[command(name = "scimkeys", about = "Corsair Scimitar PRO RGB side-button daemon")]
struct Cli {
    /// Path to the TOML config file.
    #[arg(long, default_value = "/etc/scimkeys.toml")]
    config: PathBuf,

    /// Enable debug-level logging of per-event activity.
    #[arg(short = 'v', long = "verbose")]
    verbose: bool,

    /// Print raw report changes instead of emitting key events. Does not
    /// create a uinput device.
    #[arg(long)]
    dump: bool,
}

fn main() -> ExitCode {
    let cli = Cli::parse();

    let default_level = if cli.verbose { "debug" } else { "info" };
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or(default_level))
        .init();

    let config = match config::load(&cli.config) {
        Ok(config) => config,
        Err(e) => {
            error!("failed to load config from {}: {e:#}", cli.config.display());
            return ExitCode::FAILURE;
        }
    };

    let result = if cli.dump {
        run_dump(&config)
    } else {
        run(&config)
    };

    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            error!("fatal error: {e:#}");
            ExitCode::FAILURE
        }
    }
}

/// State shared between the main loop and the signal handler, so the
/// handler can release any held keys before the process exits.
struct SharedState {
    device: VirtualDevice,
    button_keys: [Option<KeyCode>; 12],
    held_mask: u16,
}

impl SharedState {
    /// Release every currently-held key (if any) and reset the held mask.
    fn release_all(&mut self) {
        if self.held_mask == 0 {
            return;
        }
        let mut events: Vec<InputEvent> = Vec::with_capacity(12);
        for (i, key) in self.button_keys.iter().enumerate() {
            if self.held_mask & (1 << i) != 0 {
                if let Some(key) = key {
                    events.push(InputEvent::from(KeyEvent::new(*key, 0)));
                }
            }
        }
        if !events.is_empty() {
            if let Err(e) = self.device.emit(&events) {
                warn!("failed to release held keys: {e}");
            }
        }
        self.held_mask = 0;
    }
}

/// Build the uinput virtual keyboard, declaring exactly the keys that are
/// bound in the config.
fn build_virtual_device(config: &Config) -> Result<VirtualDevice> {
    let mut keys = AttributeSet::<KeyCode>::new();
    for key in config.buttons.iter().flatten() {
        keys.insert(*key);
    }

    let id = InputId::new(BusType::BUS_USB, config.vendor, config.product, 0x0100);

    VirtualDevice::builder()
        .context("failed to open /dev/uinput")?
        .name(VIRTUAL_DEVICE_NAME)
        .input_id(id)
        .with_keys(&keys)
        .context("failed to set uinput key capabilities")?
        .build()
        .context("failed to create uinput virtual device")
}

/// Install a handler for SIGINT and SIGTERM that releases any held keys and
/// exits with code 0.
fn install_signal_handler(state: Arc<Mutex<SharedState>>) -> Result<()> {
    ctrlc::set_handler(move || {
        if let Ok(mut guard) = state.lock() {
            guard.release_all();
        }
        info!("received shutdown signal, exiting");
        std::process::exit(0);
    })
    .context("failed to install signal handler")
}

/// Install a handler for SIGINT and SIGTERM that just exits with code 0.
/// Used in --dump mode, where there is no uinput device or held key state.
fn install_signal_handler_dump() -> Result<()> {
    ctrlc::set_handler(|| {
        info!("received shutdown signal, exiting");
        std::process::exit(0);
    })
    .context("failed to install signal handler")
}

/// Scan /sys/class/hidraw for the configured vendor/product on interface 0
/// and return the matching /dev/hidrawN path, if any.
fn discover_device(vendor: u16, product: u16) -> Option<PathBuf> {
    let base = Path::new("/sys/class/hidraw");
    let entries = fs::read_dir(base).ok()?;
    for entry in entries.flatten() {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if !name.starts_with("hidraw") {
            continue;
        }
        let uevent_path = entry.path().join("device/uevent");
        let content = match fs::read_to_string(&uevent_path) {
            Ok(content) => content,
            Err(_) => continue,
        };
        if uevent_matches(&content, vendor, product) {
            return Some(PathBuf::from("/dev").join(name.as_ref()));
        }
    }
    None
}

/// Check whether a hidraw sysfs uevent file's contents describe interface 0
/// of the configured vendor/product device.
fn uevent_matches(content: &str, vendor: u16, product: u16) -> bool {
    let mut hid_id_match = false;
    let mut phys_is_input0 = false;
    for line in content.lines() {
        if let Some(value) = line.strip_prefix("HID_ID=") {
            let parts: Vec<&str> = value.split(':').collect();
            if let [_bus, vendor_hex, product_hex] = parts[..] {
                if let (Ok(v), Ok(p)) = (
                    u32::from_str_radix(vendor_hex, 16),
                    u32::from_str_radix(product_hex, 16),
                ) {
                    hid_id_match = v == u32::from(vendor) && p == u32::from(product);
                }
            }
        } else if let Some(value) = line.strip_prefix("HID_PHYS=") {
            phys_is_input0 = value.ends_with("input0");
        }
    }
    hid_id_match && phys_is_input0
}

/// Normal (non-dump) main loop: build the virtual keyboard once, then
/// repeatedly discover the device and read reports from it, translating
/// button state changes into key events.
fn run(config: &Config) -> Result<()> {
    let device = build_virtual_device(config)?;
    let state = Arc::new(Mutex::new(SharedState {
        device,
        button_keys: config.buttons,
        held_mask: 0,
    }));

    install_signal_handler(Arc::clone(&state))?;

    let mut warned_missing = false;
    loop {
        match discover_device(config.vendor, config.product) {
            Some(path) => {
                warned_missing = false;
                info!("found device at {}", path.display());
                if let Err(e) = read_loop(&path, &state) {
                    warn!("device lost ({}): {e:#}", path.display());
                    // Throttle so a device that is found but immediately
                    // fails to open (e.g. a permissions problem) can't spin
                    // in a tight discover/fail loop.
                    std::thread::sleep(DISCOVERY_RETRY_INTERVAL);
                }
                info!("device lost, resuming discovery");
            }
            None => {
                if !warned_missing {
                    warn!(
                        "no Scimitar PRO RGB device found (vendor={:#06x} product={:#06x}), \
                         retrying every {}s",
                        config.vendor,
                        config.product,
                        DISCOVERY_RETRY_INTERVAL.as_secs()
                    );
                    warned_missing = true;
                }
                std::thread::sleep(DISCOVERY_RETRY_INTERVAL);
            }
        }
    }
}

/// Read reports from one discovered device until a read error occurs,
/// translating button state changes into uinput key events. Releases any
/// held keys before returning.
fn read_loop(path: &Path, state: &Arc<Mutex<SharedState>>) -> Result<()> {
    let mut file =
        fs::File::open(path).with_context(|| format!("failed to open {}", path.display()))?;
    let mut buf = [0u8; REPORT_LEN];
    let mut prev_mask: u16 = 0;
    // Preallocated once and reused across reports so the per-report path
    // does no heap allocation.
    let mut event_buf: Vec<InputEvent> = Vec::with_capacity(12);

    let outcome: Result<()> = loop {
        let n = match file.read(&mut buf) {
            Ok(0) => break Err(anyhow::anyhow!("hidraw device returned EOF")),
            Ok(n) => n,
            Err(e) => break Err(anyhow::Error::new(e).context("hidraw read failed")),
        };

        if buf[0] != BUTTON_REPORT_ID {
            continue;
        }

        let mask = extract_mask(&buf[..n]);
        let (pressed, released) = diff_mask(prev_mask, mask);
        if pressed == 0 && released == 0 {
            continue;
        }

        event_buf.clear();
        {
            let mut guard = state.lock().unwrap();
            for (i, key) in guard.button_keys.iter().enumerate() {
                let bit = 1u16 << i;
                let Some(key) = key else { continue };
                if pressed & bit != 0 {
                    debug!("G{} press", i + 1);
                    event_buf.push(InputEvent::from(KeyEvent::new(*key, 1)));
                } else if released & bit != 0 {
                    debug!("G{} release", i + 1);
                    event_buf.push(InputEvent::from(KeyEvent::new(*key, 0)));
                }
            }
            if !event_buf.is_empty() {
                if let Err(e) = guard.device.emit(&event_buf) {
                    warn!("failed to emit key events: {e}");
                }
            }
            guard.held_mask = mask;
        }
        prev_mask = mask;
    };

    if let Ok(mut guard) = state.lock() {
        guard.held_mask = prev_mask;
        guard.release_all();
    }

    outcome
}

/// --dump mode: print raw report byte changes instead of emitting key
/// events. Never touches uinput.
fn run_dump(config: &Config) -> Result<()> {
    install_signal_handler_dump()?;

    let mut warned_missing = false;
    loop {
        match discover_device(config.vendor, config.product) {
            Some(path) => {
                warned_missing = false;
                info!("found device at {}", path.display());
                if let Err(e) = dump_loop(&path) {
                    warn!("device lost ({}): {e:#}", path.display());
                    // Throttle so a device that is found but immediately
                    // fails to open (e.g. a permissions problem) can't spin
                    // in a tight discover/fail loop.
                    std::thread::sleep(DISCOVERY_RETRY_INTERVAL);
                }
                info!("device lost, resuming discovery");
            }
            None => {
                if !warned_missing {
                    warn!(
                        "no Scimitar PRO RGB device found (vendor={:#06x} product={:#06x}), \
                         retrying every {}s",
                        config.vendor,
                        config.product,
                        DISCOVERY_RETRY_INTERVAL.as_secs()
                    );
                    warned_missing = true;
                }
                std::thread::sleep(DISCOVERY_RETRY_INTERVAL);
            }
        }
    }
}

/// Print a line for every report whose bytes differ from the previous one,
/// listing non-zero bytes 1..=16 (1-indexed after the report ID byte).
fn dump_loop(path: &Path) -> Result<()> {
    let mut file =
        fs::File::open(path).with_context(|| format!("failed to open {}", path.display()))?;
    let mut buf = [0u8; REPORT_LEN];
    let mut prev = [0u8; REPORT_LEN];

    loop {
        let n = file
            .read(&mut buf)
            .context("hidraw read failed")?;
        if n == 0 {
            anyhow::bail!("hidraw device returned EOF");
        }
        if buf[0] != BUTTON_REPORT_ID {
            continue;
        }
        if buf[..n] == prev[..n] {
            continue;
        }

        let end = n.min(17); // bytes 1..=16
        let mut line = String::from("press/release:");
        for (i, byte) in buf[1..end].iter().enumerate() {
            if *byte != 0 {
                line.push_str(&format!(" byte{}=0x{byte:02X}", i + 1));
            }
        }
        println!("{line}");

        prev[..n].copy_from_slice(&buf[..n]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uevent_matches_real_scimitar_interface0() {
        let content = "DRIVER=corsair\n\
                        HID_ID=0003:00001B1C:00001B3E\n\
                        HID_NAME=Corsair Corsair Gaming SCIMITAR PRO RGB Mouse\n\
                        HID_PHYS=usb-0000:00:14.0-7.2/input0\n\
                        HID_UNIQ=\n";
        assert!(uevent_matches(content, 0x1b1c, 0x1b3e));
    }

    #[test]
    fn uevent_rejects_interface1() {
        let content = "DRIVER=corsair\n\
                        HID_ID=0003:00001B1C:00001B3E\n\
                        HID_PHYS=usb-0000:00:14.0-7.2/input1\n";
        assert!(!uevent_matches(content, 0x1b1c, 0x1b3e));
    }

    #[test]
    fn uevent_rejects_other_vendor_product() {
        let content = "HID_ID=0003:00003434:00000660\n\
                        HID_PHYS=usb-0000:00:14.0-7.4/input0\n";
        assert!(!uevent_matches(content, 0x1b1c, 0x1b3e));
    }
}
