#!/usr/bin/env bash
set -euo pipefail

if [[ "${EUID}" -ne 0 ]]; then
    echo "error: install.sh must be run as root (it writes to /usr/local/bin, /etc, and manages systemd)" >&2
    exit 1
fi

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(cd "${script_dir}/.." && pwd)"

binary="${repo_root}/target/release/scimkeys"
if [[ ! -f "${binary}" ]]; then
    echo "error: ${binary} not found; run 'cargo build --release' first" >&2
    exit 1
fi

echo "Installing ${binary} to /usr/local/bin/scimkeys"
install -m 0755 "${binary}" /usr/local/bin/scimkeys

if [[ -e /etc/scimkeys.toml ]]; then
    echo "Leaving existing /etc/scimkeys.toml in place (not overwriting)"
else
    echo "Installing ${script_dir}/scimkeys.toml to /etc/scimkeys.toml"
    install -m 0644 "${script_dir}/scimkeys.toml" /etc/scimkeys.toml
fi

echo "Installing ${script_dir}/scimkeys.service to /etc/systemd/system/scimkeys.service"
install -m 0644 "${script_dir}/scimkeys.service" /etc/systemd/system/scimkeys.service

echo "Reloading systemd and enabling scimkeys"
systemctl daemon-reload
systemctl enable --now scimkeys

echo
systemctl status scimkeys --no-pager | tail -n 10
