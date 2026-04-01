#!/usr/bin/env bash
set -euo pipefail

SERVICE_NAME="framework-desktop-fanledcontrol"
INSTALL_DIR="/usr/local/bin"
SERVICE_DIR="/etc/systemd/system"
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

# Check for cargo
if ! command -v cargo &>/dev/null; then
    echo "Error: cargo is not installed. Install Rust via https://rustup.rs/"
    exit 1
fi

# Build as current user (before sudo)
echo "Building release binary..."
cargo build --release --manifest-path "$SCRIPT_DIR/Cargo.toml"

# Ensure we have root privileges for install
if [ "$EUID" -ne 0 ]; then
    echo "Requesting elevated privileges..."
    NEED_SUDO=sudo
else
    NEED_SUDO=
fi

# Stop service if running (binary can't be overwritten while in use)
$NEED_SUDO systemctl stop "$SERVICE_NAME" 2>/dev/null || true

# Install binary and service
echo "Installing binary to $INSTALL_DIR..."
$NEED_SUDO cp "$SCRIPT_DIR/target/release/$SERVICE_NAME" "$INSTALL_DIR/"

echo "Installing systemd service..."
$NEED_SUDO cp "$SCRIPT_DIR/$SERVICE_NAME.service" "$SERVICE_DIR/"
$NEED_SUDO systemctl daemon-reload
$NEED_SUDO systemctl enable "$SERVICE_NAME"
$NEED_SUDO systemctl restart "$SERVICE_NAME"

echo "Done! Service is running."
echo "  Status:  sudo systemctl status $SERVICE_NAME"
echo "  Logs:    sudo journalctl -u $SERVICE_NAME -f"
echo "  Stop:    sudo systemctl disable --now $SERVICE_NAME"
