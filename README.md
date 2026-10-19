# scimkeys

The Corsair Scimitar PRO RGB mouse (USB vendor:product 1b1c:1b3e), when in hardware
mode, does not send its 12 side buttons (G1..G12) as keyboard keys. It streams their
state as a vendor HID report on USB interface 0: report ID 0x03, 63 bytes after the ID,
one bit per button. The Linux kernel's hid_corsair driver parses the descriptor, but
hid-input ignores vendor usages, so none of this reaches applications. scimkeys reads
the raw report from /dev/hidrawN and emits real key events through a uinput virtual
keyboard, driven by a TOML mapping. It replaces ckb-next for this one job.

## Install from a release (Debian/Ubuntu)

Download the `.deb` for the latest version from the
[Releases page](https://github.com/Velithar/scimkeys/releases), then:

```sh
sudo apt install ./scimkeys_*_amd64.deb
```

This installs the binary at `/usr/bin/scimkeys`, the config at
`/etc/scimkeys.toml`, and a systemd unit that is enabled and started
automatically.

## Install from source

Build the release binary, then run the install script as root:

```sh
cargo build --release
sudo ./packaging/install.sh
```

This copies the binary to `/usr/local/bin/scimkeys`, installs
`packaging/scimkeys.toml` to `/etc/scimkeys.toml` (only if that file does not
already exist, so it will not overwrite your edits), installs the systemd unit,
and enables and starts the service.

## Config

The config file is TOML, read from `/etc/scimkeys.toml` by default (override with
`--config PATH`). It has two sections:

```toml
[device]
vendor = 0x1b1c
product = 0x1b3e

[buttons]
g1 = "1"
g2 = "2"
...
```

`[device]` is optional and defaults to the Scimitar PRO RGB's real vendor/product IDs.
Only set it if you are pointing scimkeys at a different device.

`[buttons]` maps each side button (`g1` through `g12`) to a key name. Key names are
Linux evdev key names without the `KEY_` prefix, for example `"1"`, `"MINUS"`,
`"EQUAL"`, `"F13"`, `"LEFTCTRL"`. A button may be omitted from the file, or set to
`""`, to leave it unbound: no key is emitted for it, and it is not added to the
virtual keyboard's key set at all. An unrecognized key name is a startup error that
names the offending button and value.

## Re-capturing the button layout with --dump

The byte/bit layout in `src/layout.rs` (the `G_BITS` constant) was confirmed on a
Scimitar PRO RGB with firmware 3.14. If your unit reports differently, or you want to
check it:

1. Run `scimkeys --dump` (as a regular user is fine; this mode never touches
   uinput, so it does not need to run as root as long as your user can read the
   hidraw device).
2. Press each button on the mouse, one at a time, and watch the printed lines. Each
   line shows the non-zero bytes in the report, for example `press/release: byte2=0x01`.
3. Note which byte and bit each button sets, and compare against the current
   `G_BITS` table.
4. Edit `G_BITS` in `src/layout.rs` so each `(byte_index, bit_mask)` pair matches what
   you observed, then rebuild with `cargo build --release` and reinstall.

## Uninstall

```sh
sudo ./packaging/uninstall.sh
```

This stops and disables the service, and removes the installed binary, config file,
and systemd unit.
