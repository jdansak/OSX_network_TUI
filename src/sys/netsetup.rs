//! Wrappers around `/usr/sbin/networksetup` and `/usr/sbin/scutil` for reading
//! and changing network service, VPN, and VLAN configuration.

use std::collections::HashSet;

use super::cmd::{self, CmdError, CmdResult, NETWORKSETUP, SCUTIL};

#[derive(Debug, Clone, Default)]
pub struct Service {
    pub name: String,
    pub port: String,
    pub device: String,
    pub enabled: bool,
    /// Position in the service order (1-based), 0 when disabled.
    pub order: usize,
}

#[derive(Debug, Clone, Default)]
pub struct ServiceConfig {
    /// e.g. "DHCP", "Manual", "BOOTP", "Manual with DHCP router", "VPN", "Off".
    pub mode: String,
    pub ip: String,
    pub mask: String,
    pub router: String,
    pub client_id: String,
    pub ipv6: String,
    pub ipv6_ip: String,
    pub ipv6_router: String,
}

impl ServiceConfig {
    pub fn is_dhcp(&self) -> bool {
        self.mode.eq_ignore_ascii_case("DHCP")
    }
}

#[derive(Debug, Clone, Default)]
pub struct VpnInfo {
    /// "Connected", "Connecting", "Disconnecting", "Disconnected", etc.
    pub state: String,
    /// Tunnel interface when active, e.g. `utun6`.
    pub tunnel_device: Option<String>,
    pub ipv4: Option<String>,
    pub router: Option<String>,
    pub dns: Vec<String>,
    pub search_domains: Vec<String>,
}

impl VpnInfo {
    pub fn is_connected(&self) -> bool {
        self.state.eq_ignore_ascii_case("Connected")
    }
}

#[derive(Debug, Clone, Default)]
pub struct Vlan {
    /// VLAN user-defined name (matches the Hardware Port of the VLAN service).
    pub name: String,
    /// Parent physical interface, e.g. `en7`.
    pub parent_device: String,
    /// Virtual VLAN device, e.g. `vlan0`.
    pub device: String,
    /// 802.1Q VLAN tag (1..=4094).
    pub tag: u16,
}

#[derive(Debug, Clone)]
pub struct HardwarePort {
    pub name: String,
    pub device: String,
}

/// Read-only `networksetup` call (no elevation).
fn ns(args: &[&str]) -> CmdResult {
    cmd::run(NETWORKSETUP, args)
}

/// Mutating `networksetup` call (elevated through sudo when not root).
fn ns_priv(args: &[&str], pw: Option<&str>) -> CmdResult {
    cmd::run_privileged(NETWORKSETUP, args, pw)
}

pub fn list_services() -> Result<Vec<Service>, CmdError> {
    let out = ns(&["-listnetworkserviceorder"])?;
    let mut services = Vec::new();
    let mut current: Option<Service> = None;
    for line in out.lines() {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix("(Hardware Port: ") {
            if let Some(mut svc) = current.take() {
                let rest = rest.trim_end_matches(')');
                if let Some((port, dev)) = rest.rsplit_once(", Device: ") {
                    svc.port = port.to_string();
                    svc.device = dev.to_string();
                } else {
                    svc.port = rest.to_string();
                }
                services.push(svc);
            }
        } else if line.starts_with('(') {
            if let Some((tag, name)) = line.split_once(") ") {
                let tag = tag.trim_start_matches('(');
                let enabled = tag != "*";
                current = Some(Service {
                    name: name.to_string(),
                    enabled,
                    order: tag.parse().unwrap_or(0),
                    ..Default::default()
                });
            }
        }
    }
    Ok(services)
}

pub fn get_info(service: &str) -> Result<ServiceConfig, CmdError> {
    let out = ns(&["-getinfo", service])?;
    let mut cfg = ServiceConfig::default();
    for line in out.lines() {
        let line = line.trim();
        if line.starts_with("(Please note") {
            continue;
        }
        if !line.contains(':') {
            if let Some(mode) = line.strip_suffix(" Configuration") {
                cfg.mode = match mode {
                    "Manually Using DHCP Router" => "Manual with DHCP router".into(),
                    other => other.to_string(),
                };
            }
            continue;
        }
        let Some((k, v)) = line.split_once(':') else { continue };
        let v = v.trim();
        let v = if v == "(null)" || v == "none" { String::new() } else { v.to_string() };
        match k.trim() {
            "IP address" => cfg.ip = v,
            "Subnet mask" => cfg.mask = v,
            "Router" => cfg.router = v,
            "Client ID" => cfg.client_id = v,
            "IPv6" => cfg.ipv6 = v,
            "IPv6 IP address" => cfg.ipv6_ip = v,
            "IPv6 Router" => cfg.ipv6_router = v,
            _ => {}
        }
    }
    if cfg.mode.is_empty() {
        cfg.mode = if cfg.ip.is_empty() { "Off".into() } else { "Unknown".into() };
    }
    if cfg.ipv6.is_empty() {
        cfg.ipv6 = "Off".into();
    }
    Ok(cfg)
}

fn parse_list(out: &str) -> Vec<String> {
    if out.contains("There aren't any") {
        return Vec::new();
    }
    out.lines().map(str::trim).filter(|l| !l.is_empty()).map(String::from).collect()
}

pub fn get_dns(service: &str) -> Vec<String> {
    ns(&["-getdnsservers", service]).map(|o| parse_list(&o)).unwrap_or_default()
}

pub fn get_search_domains(service: &str) -> Vec<String> {
    ns(&["-getsearchdomains", service]).map(|o| parse_list(&o)).unwrap_or_default()
}

pub fn get_mtu(device: &str) -> Option<String> {
    let out = ns(&["-getMTU", device]).ok()?;
    // "Active MTU: 1500 (Current Setting: 1500)"
    out.lines()
        .find_map(|l| l.trim().strip_prefix("Active MTU:").map(|s| s.trim().to_string()))
}

pub fn list_locations() -> Vec<String> {
    ns(&["-listlocations"]).map(|o| parse_list(&o)).unwrap_or_default()
}

pub fn current_location() -> String {
    ns(&["-getcurrentlocation"]).map(|o| o.trim().to_string()).unwrap_or_default()
}

pub fn preferred_wifi(device: &str) -> Vec<String> {
    match ns(&["-listpreferredwirelessnetworks", device]) {
        Ok(out) => out
            .lines()
            .skip_while(|l| !l.starts_with("Preferred networks"))
            .skip(1)
            .map(str::trim)
            .filter(|l| !l.is_empty())
            .map(String::from)
            .collect(),
        Err(_) => Vec::new(),
    }
}

/// All hardware ports reported by `networksetup -listallhardwareports`.
pub fn list_hardware_ports() -> Vec<HardwarePort> {
    let Ok(out) = ns(&["-listallhardwareports"]) else {
        return Vec::new();
    };
    let mut ports = Vec::new();
    let mut cur_name: Option<String> = None;
    for line in out.lines() {
        let line = line.trim();
        if let Some(p) = line.strip_prefix("Hardware Port: ") {
            cur_name = Some(p.trim().to_string());
        } else if let Some(d) = line.strip_prefix("Device: ") {
            if let Some(name) = cur_name.take() {
                let device = d.trim().to_string();
                if !device.is_empty() {
                    ports.push(HardwarePort { name, device });
                }
            }
        }
    }
    ports
}

/// Find the hardware device name for the Wi-Fi port (usually `en0`).
pub fn wifi_device() -> Option<String> {
    list_hardware_ports()
        .into_iter()
        .find(|p| p.name == "Wi-Fi" || p.name == "AirPort")
        .map(|p| p.device)
}

/// Candidate Ethernet / Thunderbolt devices that can act as a VLAN parent.
pub fn list_vlan_parent_devices(services: &[Service]) -> Vec<(String, String)> {
    let mut seen = HashSet::new();
    let mut out = Vec::new();

    if let Ok(text) = ns(&["-listdevicesthatsupportVLAN"]) {
        for line in text.lines() {
            let d = line.trim();
            if !d.is_empty() && !d.starts_with("There ") && seen.insert(d.to_string()) {
                out.push((d.to_string(), d.to_string()));
            }
        }
    }
    for s in services {
        if s.device.starts_with("en")
            && s.port != "Wi-Fi"
            && s.port != "AirPort"
            && seen.insert(s.device.clone())
        {
            out.push((s.device.clone(), s.port.clone()));
        }
    }
    for p in list_hardware_ports() {
        if p.device.starts_with("en")
            && p.name != "Wi-Fi"
            && p.name != "AirPort"
            && seen.insert(p.device.clone())
        {
            out.push((p.device, p.name));
        }
    }
    out
}

pub fn list_vlans() -> Vec<Vlan> {
    let Ok(out) = ns(&["-listVLANs"]) else {
        return Vec::new();
    };
    let mut vlans = Vec::new();
    let mut cur: Option<Vlan> = None;
    for line in out.lines() {
        let line = line.trim();
        if let Some(name) = line.strip_prefix("VLAN User Defined Name: ") {
            if let Some(v) = cur.take() {
                vlans.push(v);
            }
            cur = Some(Vlan { name: name.trim().to_string(), ..Default::default() });
        } else if let Some(p) = line.strip_prefix("Parent Device: ") {
            if let Some(v) = &mut cur {
                v.parent_device = p.trim().to_string();
            }
        } else if let Some(d) = line.strip_prefix("Device (\"Hardware\" Port): ") {
            if let Some(v) = &mut cur {
                v.device = d.trim().to_string();
            }
        } else if let Some(t) = line.strip_prefix("Tag: ") {
            if let Some(v) = &mut cur {
                v.tag = t.trim().parse().unwrap_or(0);
            }
        }
    }
    if let Some(v) = cur {
        vlans.push(v);
    }
    vlans
}

// ---------------------------------------------------------------- VPN (scutil)

pub fn get_vpn_info(service: &str) -> Option<VpnInfo> {
    let out = cmd::run(SCUTIL, &["--nc", "status", service]).ok()?;
    let mut lines = out.lines();
    let first = lines.next()?.trim();
    if first.is_empty() || first.starts_with("No service") {
        return None;
    }
    let mut info = VpnInfo { state: first.to_string(), ..Default::default() };

    #[derive(Clone, Copy, PartialEq, Eq)]
    enum Section {
        None,
        DnsServers,
        SearchDomains,
        Ipv4,
        Ipv4Addresses,
    }
    let mut sec = Section::None;

    for raw in lines {
        let line = raw.trim();
        if line.starts_with("DNSServers : <array>") {
            sec = Section::DnsServers;
            continue;
        } else if line.starts_with("DNSSearchDomains : <array>") {
            sec = Section::SearchDomains;
            continue;
        } else if line.starts_with("IPv4 : <dictionary>") {
            sec = Section::Ipv4;
            continue;
        } else if sec == Section::Ipv4 && line.starts_with("Addresses : <array>") {
            sec = Section::Ipv4Addresses;
            continue;
        } else if line == "}" {
            sec = match sec {
                Section::Ipv4Addresses => Section::Ipv4,
                Section::Ipv4 => {
                    // Only exit Ipv4 when the dictionary itself closes (4-space indent)
                    if raw.starts_with("  }") {
                        Section::None
                    } else {
                        Section::Ipv4
                    }
                }
                _ => Section::None,
            };
            continue;
        }

        if let Some((k, v)) = line.split_once(" : ") {
            let k = k.trim();
            let v = v.trim();
            match sec {
                Section::DnsServers => {
                    if !v.is_empty() {
                        info.dns.push(v.to_string());
                    }
                }
                Section::SearchDomains => {
                    let dom = v.trim_end_matches('.');
                    if !dom.is_empty() {
                        info.search_domains.push(dom.to_string());
                    }
                }
                Section::Ipv4Addresses => {
                    if info.ipv4.is_none() && !v.is_empty() {
                        info.ipv4 = Some(v.to_string());
                    }
                }
                Section::Ipv4 => match k {
                    "InterfaceName" if !v.is_empty() => info.tunnel_device = Some(v.to_string()),
                    "Router" if !v.is_empty() => info.router = Some(v.to_string()),
                    _ => {}
                },
                Section::None => {
                    if k == "InterfaceName" && info.tunnel_device.is_none() && !v.is_empty() {
                        info.tunnel_device = Some(v.to_string());
                    }
                }
            }
        }
    }
    Some(info)
}

pub fn connect_vpn(service: &str, pw: Option<&str>) -> CmdResult {
    match cmd::run(SCUTIL, &["--nc", "start", service]) {
        Ok(o) => Ok(o),
        Err(CmdError::Failed(m)) if cmd::looks_like_permission_error(&m) => {
            cmd::run_privileged(SCUTIL, &["--nc", "start", service], pw)
        }
        Err(_) => match ns(&["-connectpppoeservice", service]) {
            Err(CmdError::Failed(m)) if cmd::looks_like_permission_error(&m) => {
                ns_priv(&["-connectpppoeservice", service], pw)
            }
            other => other,
        },
    }
}

pub fn disconnect_vpn(service: &str, pw: Option<&str>) -> CmdResult {
    match cmd::run(SCUTIL, &["--nc", "stop", service]) {
        Ok(o) => Ok(o),
        Err(CmdError::Failed(m)) if cmd::looks_like_permission_error(&m) => {
            cmd::run_privileged(SCUTIL, &["--nc", "stop", service], pw)
        }
        Err(_) => match ns(&["-disconnectpppoeservice", service]) {
            Err(CmdError::Failed(m)) if cmd::looks_like_permission_error(&m) => {
                ns_priv(&["-disconnectpppoeservice", service], pw)
            }
            other => other,
        },
    }
}

// ---------------------------------------------------------------- VLANs

pub fn create_vlan(name: &str, parent_device: &str, tag: u16, pw: Option<&str>) -> CmdResult {
    let tag_s = tag.to_string();
    ns_priv(&["-createVLAN", name, parent_device, &tag_s], pw)
}

pub fn delete_vlan(name: &str, parent_device: &str, tag: u16, pw: Option<&str>) -> CmdResult {
    let tag_s = tag.to_string();
    ns_priv(&["-deleteVLAN", name, parent_device, &tag_s], pw)
}

pub fn update_vlan(
    old: &Vlan,
    new_name: &str,
    new_parent: &str,
    new_tag: u16,
    pw: Option<&str>,
) -> CmdResult {
    delete_vlan(&old.name, &old.parent_device, old.tag, pw)?;
    create_vlan(new_name, new_parent, new_tag, pw)
}

// ---------------------------------------------------------------- mutations

pub fn set_dhcp(service: &str, pw: Option<&str>) -> CmdResult {
    ns_priv(&["-setdhcp", service], pw)
}

pub fn set_manual(service: &str, ip: &str, mask: &str, router: &str, pw: Option<&str>) -> CmdResult {
    if router.is_empty() {
        ns_priv(&["-setmanual", service, ip, mask], pw)
    } else {
        ns_priv(&["-setmanual", service, ip, mask, router], pw)
    }
}

pub fn set_manual_with_dhcp_router(service: &str, ip: &str, pw: Option<&str>) -> CmdResult {
    ns_priv(&["-setmanualwithdhcprouter", service, ip], pw)
}

/// Empty list resets to automatic (DHCP-provided) DNS servers.
pub fn set_dns(service: &str, servers: &[String], pw: Option<&str>) -> CmdResult {
    let mut args = vec!["-setdnsservers", service];
    if servers.is_empty() {
        args.push("Empty");
    } else {
        args.extend(servers.iter().map(String::as_str));
    }
    ns_priv(&args, pw)
}

pub fn set_search_domains(service: &str, domains: &[String], pw: Option<&str>) -> CmdResult {
    let mut args = vec!["-setsearchdomains", service];
    if domains.is_empty() {
        args.push("Empty");
    } else {
        args.extend(domains.iter().map(String::as_str));
    }
    ns_priv(&args, pw)
}

pub fn set_service_enabled(service: &str, on: bool, pw: Option<&str>) -> CmdResult {
    ns_priv(&["-setnetworkserviceenabled", service, if on { "on" } else { "off" }], pw)
}

pub fn set_v6(service: &str, automatic: bool, pw: Option<&str>) -> CmdResult {
    if automatic {
        ns_priv(&["-setv6automatic", service], pw)
    } else {
        ns_priv(&["-setv6off", service], pw)
    }
}

pub fn set_mtu(device: &str, mtu: &str, pw: Option<&str>) -> CmdResult {
    ns_priv(&["-setMTU", device, mtu], pw)
}

pub fn switch_location(name: &str, pw: Option<&str>) -> CmdResult {
    ns_priv(&["-switchtolocation", name], pw)
}

pub fn remove_preferred_wifi(device: &str, ssid: &str, pw: Option<&str>) -> CmdResult {
    ns_priv(&["-removepreferredwirelessnetwork", device, ssid], pw)
}

/// Fallback Wi-Fi join (used when CoreWLAN cannot resolve the SSID, e.g. no
/// Location Services permission). Tries unprivileged first.
pub fn join_wifi(device: &str, ssid: &str, password: Option<&str>, pw: Option<&str>) -> CmdResult {
    let mut args = vec!["-setairportnetwork", device, ssid];
    if let Some(p) = password.filter(|p| !p.is_empty()) {
        args.push(p);
    }
    match ns(&args) {
        Err(CmdError::Failed(m)) if cmd::looks_like_permission_error(&m) => ns_priv(&args, pw),
        other => other,
    }
}

pub fn set_wifi_power(device: &str, on: bool, pw: Option<&str>) -> CmdResult {
    let args = ["-setairportpower", device, if on { "on" } else { "off" }];
    match ns(&args) {
        Err(CmdError::Failed(m)) if cmd::looks_like_permission_error(&m) => ns_priv(&args, pw),
        other => other,
    }
}

/// Renew the DHCP lease for a service. Re-applying the DHCP configuration
/// through configd restarts the DHCP client for the underlying interface
/// without fighting System Configuration (unlike `ipconfig set`).
pub fn renew_dhcp(service: &str, pw: Option<&str>) -> CmdResult {
    ns_priv(&["-setdhcp", service], pw)
}
