#!/usr/bin/env bash
set -euo pipefail

if [[ "${EUID}" -ne 0 ]]; then
    echo "error: uninstall.sh must be run as root (it manages systemd and removes files under /etc and /usr/local/bin)" >&2
    exit 1
fi

echo "Stopping and disabling scimkeys"
systemctl stop scimkeys 2>/dev/null || true
systemctl disable scimkeys 2>/dev/null || true

echo "Removing /usr/local/bin/scimkeys"
rm -f /usr/local/bin/scimkeys

echo "Removing /etc/scimkeys.toml"
rm -f /etc/scimkeys.toml

echo "Removing /etc/systemd/system/scimkeys.service"
rm -f /etc/systemd/system/scimkeys.service

echo "Reloading systemd"
systemctl daemon-reload

echo "Done."
