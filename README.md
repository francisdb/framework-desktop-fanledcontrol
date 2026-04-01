# Framework Desktop Fan LED Control

Controls the RGB LEDs on the Framework Desktop fan based on CPU load.

- **Blue** = low load
- **Purple** = medium load  
- **Red** = high load

Each LED represents a group of CPU cores, showing the maximum load within that group.

## Requirements

- Framework Desktop with RGB fan
- Linux with `/dev/cros_ec` available (cros_ec kernel module)
- Rust toolchain

## Build

```sh
cargo build --release
```

## Usage

Requires access to `/dev/cros_ec` (typically root):

```sh
sudo ./target/release/framework-desktop-fanledcontrol
```

Press `Ctrl+C` to stop. LEDs will be turned off on exit.

### Dry run

Test without hardware access (shows colored blocks in terminal):

```sh
./target/release/framework-desktop-fanledcontrol --dry-run
```

## Install as systemd service

```sh
./install.sh
```

This builds the release binary, installs it to `/usr/local/bin/`, and enables the systemd service. It will prompt for sudo if needed. Run it again to update after pulling new changes.

Stop and start the service:

```sh
sudo systemctl stop framework-desktop-fanledcontrol
sudo systemctl start framework-desktop-fanledcontrol
```

Check status and logs:

```sh
sudo systemctl status framework-desktop-fanledcontrol
sudo journalctl -u framework-desktop-fanledcontrol -f
```

To uninstall:

```sh
./uninstall.sh
```

## See also

- [framework-system](https://github.com/FrameworkComputer/framework-system/) - Official Framework tool for interacting with the system (including LED control)
