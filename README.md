# nettui

A native, compiled Rust terminal user interface (TUI) for inspecting and controlling macOS wired, wireless, VPN, and 802.1Q VLAN network settings.

Built with [`ratatui`](https://ratatui.rs/), Apple's `CoreWLAN` and `CoreLocation` frameworks (via `objc2`), `getifaddrs(3)`, `/usr/sbin/networksetup`, and `/usr/sbin/scutil`—with zero Python or shell scripts.

## Features

- **Wired & Wireless Interfaces (`1 Interfaces`)**
  - View all macOS network services, hardware ports, BSD device names (`en0`, `en5`, `bridge0`, `vlan0`, `utun*`), service order, link status, media speed/duplex, MAC address, MTU, live IPv4/IPv6 addresses, DNS servers, and search domains.
  - Configure IPv4: **DHCP**, **Manual** (IP, Subnet Mask, Router), or **Manual with DHCP Router**.
  - Edit **DNS servers** and **search domains** (or clear to use DHCP automatic).
  - Enable / disable network services, renew DHCP leases, toggle IPv6 (**Automatic** / **Off**), and configure hardware **MTU** (1280–9000).
- **VPN Tunnels (`1 Interfaces`)**
  - Detects macOS Network Extension & VPN services (Tailscale, WireGuard, IKEv2, L2TP, IPSec) via `scutil --nc`.
  - Displays live connection state, dynamic `utun*` device, tunnel IPv4, gateway, and VPN DNS servers.
  - Connect and disconnect VPN tunnels directly with `c` or `Enter`.
- **802.1Q VLAN Management (`1 Interfaces`)**
  - Lists existing VLAN interfaces (`vlan0`, etc.), their parent physical device, and 802.1Q tag.
  - Create new VLAN interfaces (`v`), reassign VLAN tags or parent interfaces (`g`), and delete VLANs (`x`).
- **Wi-Fi Control (`2 Wi-Fi`)**
  - Native `CoreWLAN` integration showing radio power, current SSID, BSSID, RSSI, noise, SNR, channel/band/width, link rate (Mbps), PHY mode (`802.11ax`, etc.), and security type.
  - Scan for nearby networks (`s`), join open or secured networks (`Enter`), join hidden networks by SSID (`c`), disconnect (`x`), toggle Wi-Fi power (`p`), and manage/forget saved networks (`Tab`, `f`).
  - Embedded `Info.plist` + `CoreLocation` (`L`) support so macOS 14+ reveals Wi-Fi SSIDs and BSSIDs to the binary.
- **Network Locations (`3 Locations`)**
  - View and switch active macOS Network Location profiles.
- **Built-in Privilege Escalation**
  - Read-only inspection runs unprivileged; mutating operations automatically prompt for your macOS `sudo` password in a modal dialog when needed.

## Building

```sh
cargo build --release
```

The optimized binary is written to `./target/release/nettui`.

## Usage

```sh
./target/release/nettui                       # Launch interactive TUI
./target/release/nettui --status              # Print non-interactive network status report
./target/release/nettui --scan                # Scan and print nearby Wi-Fi networks
./target/release/nettui --authorize-location  # Request macOS Location Services permission (for SSIDs)
./target/release/nettui --help                # Show CLI usage
```

## Keybindings

| Context | Key | Action |
|---|---|---|
| **Global** | `1` / `2` / `3` or `←` / `→` | Switch between **Interfaces**, **Wi-Fi**, and **Locations** tabs |
| | `R` / `F5` | Refresh all network state |
| | `L` | Request macOS Location Services access (needed to un-redact SSIDs) |
| | `?` | Open help overlay |
| | `q` / `Ctrl-C` | Quit |
| **1 Interfaces** | `↑` / `↓` or `j` / `k` | Select service |
| | `e` / `Enter` | Configure IPv4 (**DHCP**, **Manual**, **Manual + DHCP Router**) |
| | `c` (or `Enter` on VPN) | Connect / disconnect selected VPN service |
| | `v` | Create a new 802.1Q VLAN interface |
| | `g` | Edit VLAN tag, parent device, or name on selected VLAN |
| | `x` / `Del` / `Backspace` | Delete selected VLAN interface (or disconnect active VPN) |
| | `d` | Switch selected service to DHCP |
| | `n` / `s` | Edit DNS servers / search domains |
| | `t` / `Space` | Enable / disable network service |
| | `r` | Renew DHCP lease |
| | `6` | Toggle IPv6 (**Automatic** / **Off**) |
| | `m` | Set hardware MTU |
| **2 Wi-Fi** | `s` | Scan for nearby Wi-Fi networks |
| | `Tab` | Switch focus between **Nearby networks** and **Saved networks** |
| | `Enter` | Join selected network |
| | `c` | Join network by SSID (including hidden networks) |
| | `x` / `p` | Disconnect from Wi-Fi / toggle Wi-Fi power |
| | `f` / `Del` / `Backspace` | Forget selected saved network |
| **3 Locations** | `Enter` | Switch to selected macOS Network Location |
