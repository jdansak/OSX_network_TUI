# nettui (OSX Network TUI)

[![macOS](https://img.shields.io/badge/platform-macOS-lightgrey.svg)](https://apple.com/macos)
[![Rust](https://img.shields.io/badge/language-Rust-orange.svg)](https://www.rust-lang.org)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](https://opensource.org/licenses/MIT)

A native, compiled Rust terminal user interface (TUI) for inspecting, controlling, and configuring macOS network settings: wired interfaces, Wi-Fi connections, VPN tunnels, and 802.1Q VLANs.

Built with [`ratatui`](https://ratatui.rs/), Apple's `CoreWLAN` and `CoreLocation` frameworks (via `objc2`), `getifaddrs(3)`, `/usr/sbin/networksetup`, and `/usr/sbin/scutil`. No Python or shell scripts are required or used.

```text
 nettui   1 Interfaces  2 Wi-Fi  3 Locations           Location: authorized │ user │ loc: Automatic
╭ Network services ──────────────────────────╮╭ Details ────────────────────────────────────────╮
│    Service                Device    Mode   ││● Wi-Fi (Connected)                              │
│▶ ● Wi-Fi                  en0       DHCP   ││Hardware      Wi-Fi                              │
│  ● USB 10/100/1G LAN      en7       Manual ││Device        en0                                │
│  ○ VLAN 10                vlan0·10  Manual ││Order         1                                  │
│  ● Tailscale              utun6     VPN    ││                                                 │
│  ◌ WireGuard                        VPN    ││IPv4 (configured)                                │
│                                            ││Configure     DHCP                               │
│                                            ││IP address    192.168.1.15                       │
│                                            ││Subnet mask   255.255.255.0                      │
│                                            ││Router        192.168.1.1                        │
╰────────────────────────────────────────────╯╰─────────────────────────────────────────────────╯
 ↑↓ select  e IPv4  d DHCP  n DNS  t on/off  c vpn  v +vlan  g tag  x del  r renew  m MTU  q quit
```

---

## Features

### 1. Wired & Wireless Network Services (`1 Interfaces`)
- **Service Overview**: Displays hardware ports, BSD device identifiers (`en0`, `en7`, `bridge0`, `vlan0`, `utun*`), priority order, link state, and live IP configuration.
- **IPv4 Configuration (`e` / `Enter`)**: Interactive modal to switch between **DHCP**, **Manual** (IP, Subnet Mask, Router), or **Manual with DHCP Router**.
- **DNS & Search Domains (`n`, `s`)**: View and modify custom DNS servers and search domains, or clear them to restore automatic DHCP resolvers.
- **Hardware Control**: Enable or disable network services (`t`), renew DHCP leases (`r`), toggle IPv6 (`6`), and tune MTU (`m`).

### 2. VPN Management (`1 Interfaces`)
- **Tunnel Discovery**: Integrates with `/usr/sbin/scutil --nc` to inspect all configured VPN connections (Tailscale, WireGuard, IKEv2, L2TP, Cisco IPSec).
- **Status & Metrics**: Displays live connection status (`Connected`, `Connecting`, `Disconnected`), dynamic tunnel devices (`utun*`), assigned tunnel IPv4 addresses, and tunnel DNS servers.
- **Instant Control (`c`)**: Connect or disconnect any selected VPN tunnel directly from the TUI.

### 3. 802.1Q VLAN Interface Management (`1 Interfaces`)
- **VLAN Inspection**: Lists active VLAN interfaces (`vlan0`, etc.), their physical parent devices, and assigned 802.1Q tags.
- **Create VLANs (`v`)**: Create virtual VLAN interfaces with custom names, selected parent physical interfaces, and tags (`1–4094`).
- **Edit VLANs (`g`)**: Reassign VLAN tags, rename VLANs, or migrate them across parent interfaces.
- **Delete VLANs (`x` / `Delete`)**: Delete VLAN interfaces safely with confirmation.

### 4. Interactive Wi-Fi Control (`2 Wi-Fi`)
- **Live Connection Stats**: Reports current SSID, BSSID, RSSI, noise floor, SNR, transmit rate (Mbps), channel width/band (2.4 GHz, 5 GHz, 6 GHz), PHY mode (e.g. `802.11ax` / Wi-Fi 6), and security protocol (WPA2/WPA3 Personal/Enterprise, OWE).
- **Network Scanner (`s`)**: Scans for surrounding access points with real-time signal bars, deduplicating BSSIDs while tracking AP counts.
- **Join Networks (`Enter` / `c`)**: Associate with scanned networks, join hidden SSIDs, or connect to saved networks using credentials from the macOS Keychain.
- **Radio & Preferred Networks**: Toggle Wi-Fi power (`p`), disconnect from current network (`x`), switch focus to preferred networks (`Tab`), and forget saved networks (`f`).

### 5. Network Locations (`3 Locations`)
- Switch active macOS Network Location profiles on the fly to swap multiple service configurations at once.

---

## Security & Architecture

- **No Shell Execution**: All external commands (`networksetup`, `scutil`, `ifconfig`) are spawned directly as child processes via `std::process::Command` with argument arrays—eliminating shell injection risks.
- **Safe Privilege Escalation**: Read operations execute unprivileged. Modifying operations probe cached `sudo` credentials first; if admin elevation is required, a secure in-TUI password dialog prompts the user without echoing characters.
- **Embedded `Info.plist` & CoreLocation**: Starting in macOS 14 (Sonoma), Apple redacts Wi-Fi SSIDs/BSSIDs unless the calling binary has Location Services permission. `build.rs` embeds an `Info.plist` into the binary's `__TEXT,__info_plist` Mach-O section, allowing `nettui` to request and obtain CoreLocation authorization.

---

## Installation & Requirements

### Requirements
- macOS 13+ (Ventura, Sonoma, Sequoia, or later)
- Rust toolchain (`cargo` 1.80+)

### Building from Source

```bash
# Clone the repository
git clone https://github.com/Jdansak/OSX_network_TUI.git
cd OSX_network_TUI

# Compile optimized release binary
cargo build --release

# Run nettui
./target/release/nettui
```

---

## CLI Options

`nettui` can also run non-interactively for headless checks and scripting:

```bash
# Launch interactive TUI
nettui

# Output formatted status report of all interfaces, Wi-Fi, and locations
nettui --status

# Perform a Wi-Fi scan and print nearby access points
nettui --scan

# Trigger the macOS Location Services permission prompt (to view SSIDs)
nettui --authorize-location

# Show version or help
nettui --version
nettui --help
```

---

## Keyboard Shortcuts

### Global
| Key | Action |
|---|---|
| `1` / `2` / `3` or `←` / `→` | Switch tabs (Interfaces / Wi-Fi / Locations) |
| `R` / `F5` | Refresh all network and Wi-Fi state |
| `L` | Request macOS Location Services authorization |
| `?` | Toggle keybinding help overlay |
| `q` / `Ctrl-C` | Exit application |

### Tab 1: Interfaces, VPNs & VLANs
| Key | Action |
|---|---|
| `↑` / `↓` or `k` / `j` | Select network service |
| `e` / `Enter` | Edit IPv4 configuration (or toggle VPN if selected) |
| `c` | Connect / disconnect selected VPN tunnel |
| `v` | Create a new 802.1Q VLAN interface |
| `g` | Edit VLAN tag or parent device |
| `x` / `Del` | Delete selected VLAN interface (or disconnect VPN) |
| `d` | Set selected interface to DHCP |
| `n` / `s` | Set custom DNS servers / search domains |
| `t` / `Space` | Toggle service enabled / disabled |
| `r` | Renew DHCP lease |
| `6` | Toggle IPv6 (Automatic / Off) |
| `m` | Configure hardware MTU |

### Tab 2: Wi-Fi
| Key | Action |
|---|---|
| `s` | Scan for nearby Wi-Fi networks |
| `Enter` | Join selected network (prompts for password if encrypted) |
| `c` | Join network by SSID (supports hidden networks) |
| `x` | Disconnect from active Wi-Fi network |
| `p` | Toggle Wi-Fi hardware power On / Off |
| `Tab` | Switch focus between Scanned Networks and Saved Networks |
| `f` / `Del` | Forget selected saved network |

### Tab 3: Locations
| Key | Action |
|---|---|
| `Enter` | Switch to selected macOS Network Location |

---

## License

MIT License. See [LICENSE](LICENSE) for details.
