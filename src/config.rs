//! TOML config parsing and validation for scimkeys.

use std::fs;
use std::path::Path;

use anyhow::{Context, Result};
use evdev::KeyCode;
use serde::Deserialize;

/// Default vendor ID for the Corsair Scimitar PRO RGB (USB VID 0x1b1c).
pub const DEFAULT_VENDOR: u16 = 0x1b1c;
/// Default product ID for the Corsair Scimitar PRO RGB (USB PID 0x1b3e).
pub const DEFAULT_PRODUCT: u16 = 0x1b3e;

const BUTTON_NAMES: [&str; 12] = [
    "g1", "g2", "g3", "g4", "g5", "g6", "g7", "g8", "g9", "g10", "g11", "g12",
];

#[derive(Debug, Deserialize, Default)]
#[serde(default)]
struct RawConfig {
    device: RawDevice,
    buttons: RawButtons,
}

#[derive(Debug, Deserialize)]
#[serde(default)]
struct RawDevice {
    vendor: u16,
    product: u16,
}

impl Default for RawDevice {
    fn default() -> Self {
        RawDevice {
            vendor: DEFAULT_VENDOR,
            product: DEFAULT_PRODUCT,
        }
    }
}

#[derive(Debug, Deserialize, Default)]
struct RawButtons {
    #[serde(default)]
    g1: Option<String>,
    #[serde(default)]
    g2: Option<String>,
    #[serde(default)]
    g3: Option<String>,
    #[serde(default)]
    g4: Option<String>,
    #[serde(default)]
    g5: Option<String>,
    #[serde(default)]
    g6: Option<String>,
    #[serde(default)]
    g7: Option<String>,
    #[serde(default)]
    g8: Option<String>,
    #[serde(default)]
    g9: Option<String>,
    #[serde(default)]
    g10: Option<String>,
    #[serde(default)]
    g11: Option<String>,
    #[serde(default)]
    g12: Option<String>,
}

impl RawButtons {
    fn as_array(&self) -> [Option<&str>; 12] {
        [
            self.g1.as_deref(),
            self.g2.as_deref(),
            self.g3.as_deref(),
            self.g4.as_deref(),
            self.g5.as_deref(),
            self.g6.as_deref(),
            self.g7.as_deref(),
            self.g8.as_deref(),
            self.g9.as_deref(),
            self.g10.as_deref(),
            self.g11.as_deref(),
            self.g12.as_deref(),
        ]
    }
}

/// Resolved, validated configuration. `buttons[i]` is the evdev key bound
/// to button G(i+1), or `None` if that button is left unbound.
#[derive(Debug, Clone)]
pub struct Config {
    pub vendor: u16,
    pub product: u16,
    pub buttons: [Option<KeyCode>; 12],
}

/// Parse one button's configured key name into an evdev `KeyCode`. An
/// absent or empty value means the button is intentionally left unbound.
fn parse_button_key(button_name: &str, value: Option<&str>) -> Result<Option<KeyCode>> {
    let value = value.unwrap_or("");
    if value.is_empty() {
        return Ok(None);
    }
    let full_name = format!("KEY_{value}");
    full_name.parse::<KeyCode>().map(Some).map_err(|_| {
        anyhow::anyhow!(
            "invalid key name {value:?} for button '{button_name}': not a recognized \
             evdev key name (use a Linux evdev key name without the KEY_ prefix, e.g. \
             \"1\", \"MINUS\", \"F13\", \"LEFTCTRL\")"
        )
    })
}

/// Parse a config from a TOML string.
pub fn parse(input: &str) -> Result<Config> {
    let raw: RawConfig = toml::from_str(input).context("failed to parse config TOML")?;
    let values = raw.buttons.as_array();

    let mut buttons: [Option<KeyCode>; 12] = [None; 12];
    for (i, (name, value)) in BUTTON_NAMES.iter().zip(values.iter()).enumerate() {
        buttons[i] = parse_button_key(name, *value)?;
    }

    Ok(Config {
        vendor: raw.device.vendor,
        product: raw.device.product,
        buttons,
    })
}

/// Load and parse a config file from disk.
pub fn load(path: &Path) -> Result<Config> {
    let input = fs::read_to_string(path)
        .with_context(|| format!("failed to read config file {}", path.display()))?;
    parse(&input).with_context(|| format!("invalid config file {}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn omitted_and_empty_buttons_are_unbound() {
        let toml = r#"
            [buttons]
            g1 = "1"
            g2 = ""
        "#;
        let config = parse(toml).unwrap();
        assert_eq!(config.buttons[0], Some(KeyCode::KEY_1));
        assert_eq!(config.buttons[1], None); // explicitly empty
        assert_eq!(config.buttons[2], None); // g3 omitted entirely
    }

    #[test]
    fn all_buttons_bound() {
        let toml = r#"
            [buttons]
            g1 = "1"
            g2 = "2"
            g3 = "3"
            g4 = "4"
            g5 = "5"
            g6 = "6"
            g7 = "7"
            g8 = "8"
            g9 = "9"
            g10 = "0"
            g11 = "MINUS"
            g12 = "EQUAL"
        "#;
        let config = parse(toml).unwrap();
        assert_eq!(config.buttons[9], Some(KeyCode::KEY_0));
        assert_eq!(config.buttons[10], Some(KeyCode::KEY_MINUS));
        assert_eq!(config.buttons[11], Some(KeyCode::KEY_EQUAL));
    }

    #[test]
    fn invalid_key_name_errors_and_names_the_bad_button() {
        let toml = r#"
            [buttons]
            g5 = "NOT_A_REAL_KEY"
        "#;
        let err = parse(toml).unwrap_err();
        let msg = format!("{err}");
        assert!(
            msg.contains("g5"),
            "error message should name the bad button: {msg}"
        );
        assert!(
            msg.contains("NOT_A_REAL_KEY"),
            "error message should include the bad value: {msg}"
        );
    }

    #[test]
    fn default_device_ids_when_section_absent() {
        let config = parse("").unwrap();
        assert_eq!(config.vendor, DEFAULT_VENDOR);
        assert_eq!(config.product, DEFAULT_PRODUCT);
    }

    #[test]
    fn device_section_overrides_defaults() {
        let toml = r#"
            [device]
            vendor = 0x1234
            product = 0x5678
        "#;
        let config = parse(toml).unwrap();
        assert_eq!(config.vendor, 0x1234);
        assert_eq!(config.product, 0x5678);
    }
}
