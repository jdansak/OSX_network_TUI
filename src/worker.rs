//! Background worker thread. All blocking operations (process spawns, Wi-Fi
//! scans and joins) run here so the UI stays responsive.

use std::collections::HashMap;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::thread;

use crate::sys::cmd::{CmdError, CmdResult};
use crate::sys::ifaces::{self, IfaceLive};
use crate::sys::netsetup::{self, Service, ServiceConfig, Vlan, VpnInfo};
use crate::sys::wifi::{self, JoinError, ScanNet, WifiInfo};

#[derive(Debug, Clone)]
pub enum Action {
    SetDhcp { svc: String },
    SetManual { svc: String, ip: String, mask: String, router: String },
    SetManualDhcpRouter { svc: String, ip: String },
    SetDns { svc: String, servers: Vec<String> },
    SetSearch { svc: String, domains: Vec<String> },
    SetEnabled { svc: String, on: bool },
    SetV6 { svc: String, automatic: bool },
    SetMtu { dev: String, mtu: String },
    RenewDhcp { svc: String },
    VpnConnect { svc: String },
    VpnDisconnect { svc: String },
    CreateVlan { name: String, parent_device: String, tag: u16 },
    DeleteVlan { name: String, parent_device: String, tag: u16 },
    UpdateVlan { old: Vlan, name: String, parent_device: String, tag: u16 },
    WifiPower { dev: String, on: bool },
    WifiJoin { dev: String, ssid: String, password: Option<String> },
    WifiDisconnect { dev: String },
    WifiForget { dev: String, ssid: String },
    SwitchLocation { name: String },
}

impl Action {
    pub fn describe(&self) -> String {
        match self {
            Action::SetDhcp { svc } => format!("Set {svc} to DHCP"),
            Action::SetManual { svc, ip, .. } => format!("Set {svc} to manual {ip}"),
            Action::SetManualDhcpRouter { svc, ip } => format!("Set {svc} to {ip} with DHCP router"),
            Action::SetDns { svc, servers } if servers.is_empty() => format!("Reset DNS on {svc} to automatic"),
            Action::SetDns { svc, servers } => format!("Set DNS on {svc}: {}", servers.join(", ")),
            Action::SetSearch { svc, .. } => format!("Set search domains on {svc}"),
            Action::SetEnabled { svc, on } => format!("{} {svc}", if *on { "Enable" } else { "Disable" }),
            Action::SetV6 { svc, automatic } => {
                format!("IPv6 {} on {svc}", if *automatic { "automatic" } else { "off" })
            }
            Action::SetMtu { dev, mtu } => format!("Set MTU {mtu} on {dev}"),
            Action::RenewDhcp { svc } => format!("Renew DHCP lease on {svc}"),
            Action::VpnConnect { svc } => format!("Connect VPN \"{svc}\""),
            Action::VpnDisconnect { svc } => format!("Disconnect VPN \"{svc}\""),
            Action::CreateVlan { name, parent_device, tag } => {
                format!("Create VLAN \"{name}\" (tag {tag} on {parent_device})")
            }
            Action::DeleteVlan { name, parent_device, tag } => {
                format!("Delete VLAN \"{name}\" (tag {tag} on {parent_device})")
            }
            Action::UpdateVlan { name, parent_device, tag, .. } => {
                format!("Update VLAN \"{name}\" (tag {tag} on {parent_device})")
            }
            Action::WifiPower { on, .. } => format!("Wi-Fi power {}", if *on { "on" } else { "off" }),
            Action::WifiJoin { ssid, .. } => format!("Join \"{ssid}\""),
            Action::WifiDisconnect { .. } => "Disconnect Wi-Fi".into(),
            Action::WifiForget { ssid, .. } => format!("Forget \"{ssid}\""),
            Action::SwitchLocation { name } => format!("Switch to location \"{name}\""),
        }
    }
}

#[derive(Debug, Clone)]
pub struct ServiceView {
    pub service: Service,
    pub config: ServiceConfig,
    pub dns: Vec<String>,
    pub search: Vec<String>,
    pub mtu: Option<String>,
    pub live: Option<IfaceLive>,
    pub vpn: Option<VpnInfo>,
    pub vlan: Option<Vlan>,
}

#[derive(Debug, Clone, Default)]
pub struct Snapshot {
    pub services: Vec<ServiceView>,
    pub vlans: Vec<Vlan>,
    pub vlan_parents: Vec<(String, String)>,
    pub wifi_device: Option<String>,
    pub preferred: Vec<String>,
    pub locations: Vec<String>,
    pub current_location: String,
    pub error: Option<String>,
}

pub enum Job {
    Refresh,
    WifiInfo,
    Scan,
    Exec { action: Action, password: Option<String> },
}

pub enum Msg {
    Snapshot(Snapshot),
    Wifi(Option<WifiInfo>),
    Scan(Result<Vec<ScanNet>, String>),
    Done { action: Action, result: CmdResult },
}

pub fn spawn() -> (Sender<Job>, Receiver<Msg>) {
    let (job_tx, job_rx) = channel::<Job>();
    let (msg_tx, msg_rx) = channel::<Msg>();
    thread::Builder::new()
        .name("nettui-worker".into())
        .spawn(move || {
            let mut wifi_dev: Option<String> = None;
            while let Ok(job) = job_rx.recv() {
                objc2::rc::autoreleasepool(|_| {
                    let msg = match job {
                        Job::Refresh => {
                            let snap = refresh();
                            wifi_dev = snap.wifi_device.clone();
                            Msg::Snapshot(snap)
                        }
                        Job::WifiInfo => Msg::Wifi(wifi::info(wifi_dev.as_deref())),
                        Job::Scan => Msg::Scan(wifi::scan(wifi_dev.as_deref())),
                        Job::Exec { action, password } => {
                            let result = execute(&action, password.as_deref());
                            Msg::Done { action, result }
                        }
                    };
                    let _ = msg_tx.send(msg);
                });
            }
        })
        .expect("spawn worker");
    (job_tx, msg_rx)
}

pub(crate) fn refresh() -> Snapshot {
    let mut snap = Snapshot::default();
    let live: HashMap<String, IfaceLive> = ifaces::snapshot();
    let vlans = netsetup::list_vlans();
    match netsetup::list_services() {
        Ok(services) => {
            snap.vlan_parents = netsetup::list_vlan_parent_devices(&services);
            for mut service in services {
                let mut config = netsetup::get_info(&service.name).unwrap_or_default();
                let mut dns = netsetup::get_dns(&service.name);
                let mut search = netsetup::get_search_domains(&service.name);
                let vpn = if service.device.is_empty() {
                    netsetup::get_vpn_info(&service.name)
                } else {
                    None
                };
                if let Some(v) = &vpn {
                    config.mode = "VPN".into();
                    if let Some(td) = &v.tunnel_device {
                        service.device = td.clone();
                    }
                    if config.ip.is_empty() {
                        if let Some(ip) = &v.ipv4 {
                            config.ip = ip.clone();
                        }
                    }
                    if config.router.is_empty() {
                        if let Some(r) = &v.router {
                            config.router = r.clone();
                        }
                    }
                    if dns.is_empty() && !v.dns.is_empty() {
                        dns = v.dns.clone();
                    }
                    if search.is_empty() && !v.search_domains.is_empty() {
                        search = v.search_domains.clone();
                    }
                }
                let vlan = vlans
                    .iter()
                    .find(|vl| (!vl.device.is_empty() && vl.device == service.device) || vl.name == service.port)
                    .cloned();
                let mtu = if service.device.is_empty() || vpn.is_some() {
                    None
                } else {
                    netsetup::get_mtu(&service.device)
                };
                let l = live.get(&service.device).cloned();
                snap.services.push(ServiceView { service, config, dns, search, mtu, live: l, vpn, vlan });
            }
        }
        Err(e) => snap.error = Some(e.to_string()),
    }
    snap.vlans = vlans;
    snap.wifi_device = wifi::default_device().or_else(netsetup::wifi_device);
    if let Some(dev) = &snap.wifi_device {
        snap.preferred = netsetup::preferred_wifi(dev);
    }
    snap.locations = netsetup::list_locations();
    snap.current_location = netsetup::current_location();
    snap
}

fn execute(action: &Action, pw: Option<&str>) -> CmdResult {
    use netsetup as ns;
    match action {
        Action::SetDhcp { svc } => ns::set_dhcp(svc, pw),
        Action::SetManual { svc, ip, mask, router } => ns::set_manual(svc, ip, mask, router, pw),
        Action::SetManualDhcpRouter { svc, ip } => ns::set_manual_with_dhcp_router(svc, ip, pw),
        Action::SetDns { svc, servers } => ns::set_dns(svc, servers, pw),
        Action::SetSearch { svc, domains } => ns::set_search_domains(svc, domains, pw),
        Action::SetEnabled { svc, on } => ns::set_service_enabled(svc, *on, pw),
        Action::SetV6 { svc, automatic } => ns::set_v6(svc, *automatic, pw),
        Action::SetMtu { dev, mtu } => ns::set_mtu(dev, mtu, pw),
        Action::RenewDhcp { svc } => ns::renew_dhcp(svc, pw),
        Action::VpnConnect { svc } => ns::connect_vpn(svc, pw),
        Action::VpnDisconnect { svc } => ns::disconnect_vpn(svc, pw),
        Action::CreateVlan { name, parent_device, tag } => ns::create_vlan(name, parent_device, *tag, pw),
        Action::DeleteVlan { name, parent_device, tag } => ns::delete_vlan(name, parent_device, *tag, pw),
        Action::UpdateVlan { old, name, parent_device, tag } => {
            ns::update_vlan(old, name, parent_device, *tag, pw)
        }
        Action::WifiPower { dev, on } => match wifi::set_power(Some(dev), *on) {
            Ok(()) => Ok(String::new()),
            Err(_) => ns::set_wifi_power(dev, *on, pw),
        },
        Action::WifiJoin { dev, ssid, password } => {
            match wifi::associate(Some(dev), ssid, password.as_deref()) {
                Ok(()) => Ok(format!("Joined {ssid}")),
                // SSID not visible to CoreWLAN (redacted without Location
                // permission, or hidden): let networksetup try by name.
                Err(JoinError::NotFound) => ns::join_wifi(dev, ssid, password.as_deref(), pw),
                Err(JoinError::Failed(m)) => Err(CmdError::Failed(m)),
            }
        }
        Action::WifiDisconnect { dev } => wifi::disassociate(Some(dev)).map(|_| String::new()).map_err(CmdError::Failed),
        Action::WifiForget { dev, ssid } => ns::remove_preferred_wifi(dev, ssid, pw),
        Action::SwitchLocation { name } => ns::switch_location(name, pw),
    }
}
