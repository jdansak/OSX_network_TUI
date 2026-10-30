//! Application state and input handling.

use std::sync::mpsc::Sender;
use std::time::{Duration, Instant};

use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::widgets::{ListState, TableState};

use crate::sys::cmd::CmdError;
use crate::sys::location::LocAuth;
use crate::sys::netsetup::Vlan;
use crate::sys::wifi::{ScanNet, WifiInfo};
use crate::worker::{Action, Job, Msg, ServiceView, Snapshot};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    Interfaces,
    Wifi,
    Locations,
}

impl Tab {
    pub const ALL: [Tab; 3] = [Tab::Interfaces, Tab::Wifi, Tab::Locations];
    pub fn title(self) -> &'static str {
        match self {
            Tab::Interfaces => "1 Interfaces",
            Tab::Wifi => "2 Wi-Fi",
            Tab::Locations => "3 Locations",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WifiFocus {
    Scan,
    Preferred,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StatusKind {
    Info,
    Ok,
    Err,
}

#[derive(Debug, Clone)]
pub struct Field {
    pub label: &'static str,
    pub value: String,
    pub secret: bool,
    pub choices: Option<Vec<String>>,
    pub choice: usize,
}

impl Field {
    fn text(label: &'static str, value: impl Into<String>) -> Self {
        Self { label, value: value.into(), secret: false, choices: None, choice: 0 }
    }
    fn secret(label: &'static str) -> Self {
        Self { label, value: String::new(), secret: true, choices: None, choice: 0 }
    }
    fn choice(label: &'static str, choices: Vec<String>, choice: usize) -> Self {
        Self { label, value: String::new(), secret: false, choices: Some(choices), choice }
    }
    pub fn display_value(&self) -> String {
        if let Some(c) = &self.choices {
            let cur = c.get(self.choice).map(String::as_str).unwrap_or("-");
            format!("◀ {cur} ▶")
        } else if self.secret {
            "•".repeat(self.value.chars().count())
        } else {
            self.value.clone()
        }
    }
}

#[derive(Debug, Clone)]
pub enum FormKind {
    Ipv4 { svc: String },
    Dns { svc: String },
    Search { svc: String },
    Mtu { dev: String },
    VlanCreate { parents: Vec<String> },
    VlanEdit { old: Vlan, parents: Vec<String> },
    Join { dev: String },
    JoinKnown { dev: String, ssid: String },
}

#[derive(Debug, Clone)]
pub struct Form {
    pub title: String,
    pub help: String,
    pub fields: Vec<Field>,
    pub focus: usize,
    pub kind: FormKind,
    pub error: Option<String>,
}

pub enum Modal {
    Form(Form),
    Confirm { message: String, action: Action },
    Password { action: Action, input: String, error: Option<String> },
    Help,
}

pub struct App {
    pub tab: Tab,
    pub snap: Snapshot,
    pub wifi: Option<WifiInfo>,
    pub scan: Vec<ScanNet>,
    pub scanning: bool,
    pub svc_state: TableState,
    pub scan_state: TableState,
    pub pref_state: ListState,
    pub loc_state: ListState,
    pub wifi_focus: WifiFocus,
    pub modal: Option<Modal>,
    pub busy: usize,
    pub refreshing: bool,
    wifi_inflight: bool,
    pub status: Option<(String, StatusKind, Instant)>,
    password: Option<String>,
    pub loc_auth: LocAuth,
    pub want_location_request: bool,
    pub quit: bool,
    pub tick: u64,
    tx: Sender<Job>,
    last_refresh: Instant,
    last_wifi: Instant,
}

const REFRESH_EVERY: Duration = Duration::from_secs(15);
const WIFI_EVERY: Duration = Duration::from_secs(2);

impl App {
    pub fn new(tx: Sender<Job>, loc_auth: LocAuth) -> Self {
        let mut app = Self {
            tab: Tab::Interfaces,
            snap: Snapshot::default(),
            wifi: None,
            scan: Vec::new(),
            scanning: false,
            svc_state: TableState::default(),
            scan_state: TableState::default(),
            pref_state: ListState::default(),
            loc_state: ListState::default(),
            wifi_focus: WifiFocus::Scan,
            modal: None,
            busy: 0,
            refreshing: false,
            wifi_inflight: false,
            status: None,
            password: None,
            loc_auth,
            want_location_request: false,
            quit: false,
            tick: 0,
            tx,
            last_refresh: Instant::now(),
            last_wifi: Instant::now(),
        };
        app.refresh();
        app.request_wifi();
        app
    }

    // ------------------------------------------------------------- jobs

    pub fn refresh(&mut self) {
        if !self.refreshing {
            self.refreshing = true;
            self.last_refresh = Instant::now();
            let _ = self.tx.send(Job::Refresh);
        }
    }

    fn request_wifi(&mut self) {
        if !self.wifi_inflight {
            self.wifi_inflight = true;
            self.last_wifi = Instant::now();
            let _ = self.tx.send(Job::WifiInfo);
        }
    }

    fn start_scan(&mut self) {
        if !self.scanning {
            self.scanning = true;
            self.set_status("Scanning for Wi-Fi networks…", StatusKind::Info);
            let _ = self.tx.send(Job::Scan);
        }
    }

    fn exec(&mut self, action: Action) {
        self.busy += 1;
        self.set_status(format!("{}…", action.describe()), StatusKind::Info);
        let _ = self.tx.send(Job::Exec { action, password: self.password.clone() });
    }

    pub fn on_tick(&mut self) {
        self.tick = self.tick.wrapping_add(1);
        if self.last_wifi.elapsed() >= WIFI_EVERY {
            self.request_wifi();
        }
        if self.last_refresh.elapsed() >= REFRESH_EVERY && self.busy == 0 {
            self.refresh();
        }
    }

    pub fn set_status(&mut self, msg: impl Into<String>, kind: StatusKind) {
        self.status = Some((msg.into(), kind, Instant::now()));
    }

    pub fn on_msg(&mut self, msg: Msg) {
        match msg {
            Msg::Snapshot(snap) => {
                self.refreshing = false;
                let keep = self.selected_service().map(|s| s.service.name.clone());
                self.snap = snap;
                if let Some(err) = self.snap.error.clone() {
                    self.set_status(err, StatusKind::Err);
                }
                let idx = keep
                    .and_then(|n| self.snap.services.iter().position(|s| s.service.name == n))
                    .or(if self.snap.services.is_empty() { None } else { Some(0) });
                self.svc_state.select(idx);
                clamp_list(&mut self.pref_state, self.snap.preferred.len());
                clamp_list(&mut self.loc_state, self.snap.locations.len());
            }
            Msg::Wifi(info) => {
                self.wifi_inflight = false;
                self.wifi = info;
            }
            Msg::Scan(res) => {
                self.scanning = false;
                match res {
                    Ok(nets) => {
                        let n = nets.len();
                        self.scan = nets;
                        clamp_table(&mut self.scan_state, n);
                        self.set_status(format!("Found {n} networks"), StatusKind::Ok);
                    }
                    Err(e) => self.set_status(format!("Scan failed: {e}"), StatusKind::Err),
                }
            }
            Msg::Done { action, result } => {
                self.busy = self.busy.saturating_sub(1);
                match result {
                    Ok(_) => {
                        self.set_status(format!("✓ {}", action.describe()), StatusKind::Ok);
                        self.refresh();
                        self.request_wifi();
                    }
                    Err(CmdError::NeedPassword) => {
                        self.modal = Some(Modal::Password { action, input: String::new(), error: None });
                    }
                    Err(CmdError::BadPassword) => {
                        self.password = None;
                        self.modal = Some(Modal::Password {
                            action,
                            input: String::new(),
                            error: Some("Incorrect password, try again".into()),
                        });
                    }
                    Err(CmdError::Failed(m)) => {
                        let m = m.lines().next().unwrap_or("").to_string();
                        self.set_status(format!("✗ {}: {m}", action.describe()), StatusKind::Err);
                        self.refresh();
                    }
                }
            }
        }
    }

    // ---------------------------------------------------------- selection

    pub fn selected_service(&self) -> Option<&ServiceView> {
        self.svc_state.selected().and_then(|i| self.snap.services.get(i))
    }

    fn wifi_dev(&self) -> Option<String> {
        self.snap.wifi_device.clone().or_else(|| self.wifi.as_ref().map(|w| w.device.clone()))
    }

    // -------------------------------------------------------------- input

    pub fn on_key(&mut self, key: KeyEvent) {
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
            self.quit = true;
            return;
        }
        if self.modal.is_some() {
            self.on_modal_key(key);
            return;
        }
        match key.code {
            KeyCode::Char('q') => self.quit = true,
            KeyCode::Char('?') => self.modal = Some(Modal::Help),
            KeyCode::Char('1') => self.tab = Tab::Interfaces,
            KeyCode::Char('2') => self.tab = Tab::Wifi,
            KeyCode::Char('3') => self.tab = Tab::Locations,
            KeyCode::Right | KeyCode::Char(']') => self.tab = next_tab(self.tab, 1),
            KeyCode::Left | KeyCode::Char('[') => self.tab = next_tab(self.tab, 2),
            KeyCode::F(5) | KeyCode::Char('R') => {
                self.refresh();
                self.request_wifi();
                self.set_status("Refreshing…", StatusKind::Info);
            }
            KeyCode::Char('L') => {
                self.want_location_request = true;
                self.set_status(
                    "Requested Location Services access – approve the macOS prompt (needed to show SSIDs)",
                    StatusKind::Info,
                );
            }
            _ => match self.tab {
                Tab::Interfaces => self.on_interfaces_key(key),
                Tab::Wifi => self.on_wifi_key(key),
                Tab::Locations => self.on_locations_key(key),
            },
        }
    }

    fn on_interfaces_key(&mut self, key: KeyEvent) {
        let n = self.snap.services.len();
        match key.code {
            KeyCode::Up | KeyCode::Char('k') => table_step(&mut self.svc_state, n, -1),
            KeyCode::Down | KeyCode::Char('j') => table_step(&mut self.svc_state, n, 1),
            KeyCode::Char('v') => {
                self.open_create_vlan_form();
                return;
            }
            _ => {}
        }
        let Some(sv) = self.selected_service().cloned() else { return };
        let svc = sv.service.name.clone();
        match key.code {
            KeyCode::Char('c') => {
                if let Some(vpn) = &sv.vpn {
                    self.toggle_vpn(&svc, vpn.is_connected());
                } else {
                    self.set_status("Selected service is not a VPN tunnel", StatusKind::Err);
                }
            }
            KeyCode::Char('g') => {
                if let Some(vl) = &sv.vlan {
                    self.open_edit_vlan_form(vl.clone());
                } else {
                    self.open_create_vlan_form();
                }
            }
            KeyCode::Char('x') | KeyCode::Delete | KeyCode::Backspace => {
                if let Some(vl) = &sv.vlan {
                    self.modal = Some(Modal::Confirm {
                        message: format!(
                            "Delete VLAN \"{}\" (tag {} on {})?",
                            vl.name, vl.tag, vl.parent_device
                        ),
                        action: Action::DeleteVlan {
                            name: vl.name.clone(),
                            parent_device: vl.parent_device.clone(),
                            tag: vl.tag,
                        },
                    });
                } else if let Some(vpn) = &sv.vpn {
                    if vpn.is_connected() {
                        self.toggle_vpn(&svc, true);
                    }
                } else {
                    self.set_status("Only VLAN interfaces can be deleted here", StatusKind::Err);
                }
            }
            KeyCode::Enter if sv.vpn.is_some() => {
                if let Some(vpn) = &sv.vpn {
                    self.toggle_vpn(&svc, vpn.is_connected());
                }
            }
            KeyCode::Enter | KeyCode::Char('e') => {
                let c = &sv.config;
                let mode = match c.mode.as_str() {
                    "Manual" => 1,
                    "Manual with DHCP router" => 2,
                    _ => 0,
                };
                let (ip, mask, router) = if c.is_dhcp() {
                    (String::new(), "255.255.255.0".to_string(), String::new())
                } else {
                    (none_blank(&c.ip), none_blank(&c.mask), none_blank(&c.router))
                };
                self.modal = Some(Modal::Form(Form {
                    title: format!("IPv4 configuration – {svc}"),
                    help: "←/→ change mode · Tab next field · Enter apply · Esc cancel".into(),
                    fields: vec![
                        Field::choice(
                            "Configure",
                            vec!["DHCP".into(), "Manual".into(), "Manual + DHCP router".into()],
                            mode,
                        ),
                        Field::text("IP address", ip),
                        Field::text("Subnet mask", mask),
                        Field::text("Router", router),
                    ],
                    focus: 0,
                    kind: FormKind::Ipv4 { svc },
                    error: None,
                }));
            }
            KeyCode::Char('d') => self.modal = Some(Modal::Confirm {
                message: format!("Switch \"{svc}\" to DHCP?"),
                action: Action::SetDhcp { svc },
            }),
            KeyCode::Char('n') => {
                self.modal = Some(Modal::Form(Form {
                    title: format!("DNS servers – {svc}"),
                    help: "Space/comma separated IPv4/IPv6 addresses. Empty = automatic (DHCP).".into(),
                    fields: vec![Field::text("DNS servers", sv.dns.join(" "))],
                    focus: 0,
                    kind: FormKind::Dns { svc },
                    error: None,
                }))
            }
            KeyCode::Char('s') => {
                self.modal = Some(Modal::Form(Form {
                    title: format!("Search domains – {svc}"),
                    help: "Space/comma separated domains. Empty = automatic.".into(),
                    fields: vec![Field::text("Search domains", sv.search.join(" "))],
                    focus: 0,
                    kind: FormKind::Search { svc },
                    error: None,
                }))
            }
            KeyCode::Char('m') if !sv.service.device.is_empty() && sv.vpn.is_none() => {
                let cur = sv.live.as_ref().and_then(|l| l.mtu).map(|m| m.to_string()).unwrap_or_default();
                self.modal = Some(Modal::Form(Form {
                    title: format!("MTU – {} ({})", svc, sv.service.device),
                    help: "Typical: 1500 (Ethernet), 9000 (jumbo frames). Range 1280–9000.".into(),
                    fields: vec![Field::text("MTU", cur)],
                    focus: 0,
                    kind: FormKind::Mtu { dev: sv.service.device.clone() },
                    error: None,
                }))
            }
            KeyCode::Char('t') | KeyCode::Char(' ') => {
                let on = !sv.service.enabled;
                let action = Action::SetEnabled { svc: svc.clone(), on };
                if on {
                    self.exec(action);
                } else {
                    self.modal = Some(Modal::Confirm {
                        message: format!("Disable network service \"{svc}\"?"),
                        action,
                    });
                }
            }
            KeyCode::Char('r') => {
                if sv.config.is_dhcp() {
                    self.exec(Action::RenewDhcp { svc });
                } else {
                    self.set_status("Renew lease only applies to DHCP services", StatusKind::Err);
                }
            }
            KeyCode::Char('6') => {
                let automatic = !sv.config.ipv6.eq_ignore_ascii_case("automatic");
                self.exec(Action::SetV6 { svc, automatic });
            }
            _ => {}
        }
    }

    fn toggle_vpn(&mut self, svc: &str, connected: bool) {
        if connected {
            self.modal = Some(Modal::Confirm {
                message: format!("Disconnect VPN \"{svc}\"?"),
                action: Action::VpnDisconnect { svc: svc.to_string() },
            });
        } else {
            self.exec(Action::VpnConnect { svc: svc.to_string() });
        }
    }

    fn vlan_parent_choices(&self,selected_dev: Option<&str>) -> (Vec<String>, Vec<String>, usize) {
        let mut devs = Vec::new();
        let mut labels = Vec::new();
        for (dev, port) in &self.snap.vlan_parents {
            devs.push(dev.clone());
            if port == dev || port.is_empty() {
                labels.push(dev.clone());
            } else {
                labels.push(format!("{dev} ({port})"));
            }
        }
        let idx = selected_dev
            .and_then(|d| devs.iter().position(|x| x == d))
            .unwrap_or(0);
        (devs, labels, idx)
    }

    fn open_create_vlan_form(&mut self) {
        let cur_dev = self.selected_service().and_then(|sv| {
            sv.vlan
                .as_ref()
                .map(|v| v.parent_device.as_str())
                .or(Some(sv.service.device.as_str()))
        });
        let (parents, labels, idx) = self.vlan_parent_choices(cur_dev);
        if parents.is_empty() {
            self.set_status("No Ethernet parent interfaces found for VLAN creation", StatusKind::Err);
            return;
        }
        self.modal = Some(Modal::Form(Form {
            title: "Create VLAN interface".into(),
            help: "←/→ select parent device · Tab next field · Tag 1–4094 · Enter create".into(),
            fields: vec![
                Field::text("VLAN name", "VLAN 100"),
                Field::choice("Parent device", labels, idx),
                Field::text("VLAN tag", "100"),
            ],
            focus: 0,
            kind: FormKind::VlanCreate { parents },
            error: None,
        }));
    }

    fn open_edit_vlan_form(&mut self, vl: Vlan) {
        let (parents, labels, idx) = self.vlan_parent_choices(Some(&vl.parent_device));
        if parents.is_empty() {
            self.set_status("No Ethernet parent interfaces found", StatusKind::Err);
            return;
        }
        self.modal = Some(Modal::Form(Form {
            title: format!("Edit VLAN – {} ({})", vl.name, vl.device),
            help: "Change VLAN tag, parent device, or name · Enter apply · Esc cancel".into(),
            fields: vec![
                Field::text("VLAN name", vl.name.clone()),
                Field::choice("Parent device", labels, idx),
                Field::text("VLAN tag", vl.tag.to_string()),
            ],
            focus: 2,
            kind: FormKind::VlanEdit { old: vl, parents },
            error: None,
        }));
    }

    fn on_wifi_key(&mut self, key: KeyEvent) {
        let Some(dev) = self.wifi_dev() else {
            self.set_status("No Wi-Fi interface found", StatusKind::Err);
            return;
        };
        match key.code {
            KeyCode::Tab | KeyCode::BackTab => {
                self.wifi_focus = match self.wifi_focus {
                    WifiFocus::Scan => WifiFocus::Preferred,
                    WifiFocus::Preferred => WifiFocus::Scan,
                }
            }
            KeyCode::Up | KeyCode::Char('k') => match self.wifi_focus {
                WifiFocus::Scan => table_step(&mut self.scan_state, self.scan.len(), -1),
                WifiFocus::Preferred => list_step(&mut self.pref_state, self.snap.preferred.len(), -1),
            },
            KeyCode::Down | KeyCode::Char('j') => match self.wifi_focus {
                WifiFocus::Scan => table_step(&mut self.scan_state, self.scan.len(), 1),
                WifiFocus::Preferred => list_step(&mut self.pref_state, self.snap.preferred.len(), 1),
            },
            KeyCode::Char('s') => self.start_scan(),
            KeyCode::Char('p') => {
                let on = !self.wifi.as_ref().map(|w| w.power).unwrap_or(false);
                let action = Action::WifiPower { dev, on };
                if on {
                    self.exec(action)
                } else {
                    self.modal = Some(Modal::Confirm { message: "Turn Wi-Fi off?".into(), action })
                }
            }
            KeyCode::Char('x') => self.exec(Action::WifiDisconnect { dev }),
            KeyCode::Char('c') => self.open_join_form(dev, None),
            KeyCode::Enter => match self.wifi_focus {
                WifiFocus::Scan => {
                    let Some(net) = self.scan_state.selected().and_then(|i| self.scan.get(i)).cloned() else {
                        return;
                    };
                    match net.ssid {
                        None => {
                            self.set_status(
                                "SSID hidden/redacted – enter it manually (press L to grant Location access)",
                                StatusKind::Info,
                            );
                            self.open_join_form(dev, None);
                        }
                        Some(ssid) if net.open => {
                            self.exec(Action::WifiJoin { dev, ssid, password: None })
                        }
                        Some(ssid) => self.open_join_form(dev, Some(ssid)),
                    }
                }
                WifiFocus::Preferred => {
                    if let Some(ssid) = self.pref_state.selected().and_then(|i| self.snap.preferred.get(i)).cloned() {
                        // Saved networks use the keychain password.
                        self.exec(Action::WifiJoin { dev, ssid, password: None });
                    }
                }
            },
            KeyCode::Char('f') | KeyCode::Delete | KeyCode::Backspace if self.wifi_focus == WifiFocus::Preferred => {
                if let Some(ssid) = self.pref_state.selected().and_then(|i| self.snap.preferred.get(i)).cloned() {
                    self.modal = Some(Modal::Confirm {
                        message: format!("Forget saved network \"{ssid}\"?"),
                        action: Action::WifiForget { dev, ssid },
                    });
                }
            }
            _ => {}
        }
    }

    fn open_join_form(&mut self, dev: String, ssid: Option<String>) {
        let form = match ssid {
            Some(ssid) => {
                let saved = self.snap.preferred.contains(&ssid);
                Form {
                    title: format!("Join \"{ssid}\""),
                    help: if saved {
                        "Saved network – leave password empty to use the keychain.".into()
                    } else {
                        "Enter the network password. Enter to join · Esc cancel".into()
                    },
                    fields: vec![Field::secret("Password")],
                    focus: 0,
                    kind: FormKind::JoinKnown { dev, ssid },
                    error: None,
                }
            }
            None => Form {
                title: "Join network".into(),
                help: "Works for hidden networks too. Leave password empty for open networks.".into(),
                fields: vec![Field::text("SSID", ""), Field::secret("Password")],
                focus: 0,
                kind: FormKind::Join { dev },
                error: None,
            },
        };
        self.modal = Some(Modal::Form(form));
    }

    fn on_locations_key(&mut self, key: KeyEvent) {
        let n = self.snap.locations.len();
        match key.code {
            KeyCode::Up | KeyCode::Char('k') => list_step(&mut self.loc_state, n, -1),
            KeyCode::Down | KeyCode::Char('j') => list_step(&mut self.loc_state, n, 1),
            KeyCode::Enter => {
                if let Some(name) = self.loc_state.selected().and_then(|i| self.snap.locations.get(i)).cloned() {
                    if name != self.snap.current_location {
                        self.modal = Some(Modal::Confirm {
                            message: format!("Switch network location to \"{name}\"?"),
                            action: Action::SwitchLocation { name },
                        });
                    }
                }
            }
            _ => {}
        }
    }

    // ------------------------------------------------------------- modals

    fn on_modal_key(&mut self, key: KeyEvent) {
        let Some(modal) = self.modal.take() else { return };
        match modal {
            Modal::Help => {
                // any key closes
            }
            Modal::Confirm { message, action } => match key.code {
                KeyCode::Char('y') | KeyCode::Char('Y') | KeyCode::Enter => self.exec(action),
                KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => {}
                _ => self.modal = Some(Modal::Confirm { message, action }),
            },
            Modal::Password { action, mut input, error } => match key.code {
                KeyCode::Esc => self.set_status("Cancelled", StatusKind::Info),
                KeyCode::Enter => {
                    self.password = Some(input);
                    self.exec(action);
                }
                KeyCode::Backspace => {
                    input.pop();
                    self.modal = Some(Modal::Password { action, input, error });
                }
                KeyCode::Char(c) => {
                    input.push(c);
                    self.modal = Some(Modal::Password { action, input, error });
                }
                _ => self.modal = Some(Modal::Password { action, input, error }),
            },
            Modal::Form(mut form) => {
                let nf = form.fields.len();
                match key.code {
                    KeyCode::Esc => return,
                    KeyCode::Tab | KeyCode::Down => form.focus = (form.focus + 1) % nf,
                    KeyCode::BackTab | KeyCode::Up => form.focus = (form.focus + nf - 1) % nf,
                    KeyCode::Left | KeyCode::Right => {
                        let f = &mut form.fields[form.focus];
                        if let Some(c) = &f.choices {
                            let n = c.len();
                            f.choice = if key.code == KeyCode::Right { (f.choice + 1) % n } else { (f.choice + n - 1) % n };
                        }
                    }
                    KeyCode::Backspace => {
                        form.fields[form.focus].value.pop();
                    }
                    KeyCode::Char(c) => {
                        let f = &mut form.fields[form.focus];
                        if f.choices.is_none() {
                            f.value.push(c);
                        }
                    }
                    KeyCode::Enter => match build_action(&form) {
                        Ok(action) => {
                            self.exec(action);
                            return;
                        }
                        Err(e) => form.error = Some(e),
                    },
                    _ => {}
                }
                self.modal = Some(Modal::Form(form));
            }
        }
    }
}

// ----------------------------------------------------------------- helpers

fn build_action(form: &Form) -> Result<Action, String> {
    let v = |i: usize| form.fields[i].value.trim().to_string();
    match &form.kind {
        FormKind::Ipv4 { svc } => {
            let svc = svc.clone();
            match form.fields[0].choice {
                0 => Ok(Action::SetDhcp { svc }),
                1 => {
                    let ip = v(1);
                    let mask = v(2);
                    let router = v(3);
                    ip.parse::<std::net::Ipv4Addr>().map_err(|_| "Invalid IP address")?;
                    let m: std::net::Ipv4Addr = mask.parse().map_err(|_| "Invalid subnet mask")?;
                    let bits = u32::from(m);
                    if bits.leading_ones() + bits.trailing_zeros() != 32 {
                        return Err("Subnet mask must be contiguous (e.g. 255.255.255.0)".into());
                    }
                    if !router.is_empty() {
                        router.parse::<std::net::Ipv4Addr>().map_err(|_| "Invalid router address")?;
                    }
                    Ok(Action::SetManual { svc, ip, mask, router })
                }
                _ => {
                    let ip = v(1);
                    ip.parse::<std::net::Ipv4Addr>().map_err(|_| "Invalid IP address")?;
                    Ok(Action::SetManualDhcpRouter { svc, ip })
                }
            }
        }
        FormKind::Dns { svc } => {
            let servers = split_list(&v(0));
            for s in &servers {
                s.parse::<std::net::IpAddr>().map_err(|_| format!("Invalid DNS server: {s}"))?;
            }
            Ok(Action::SetDns { svc: svc.clone(), servers })
        }
        FormKind::Search { svc } => Ok(Action::SetSearch { svc: svc.clone(), domains: split_list(&v(0)) }),
        FormKind::Mtu { dev } => {
            let mtu: u32 = v(0).parse().map_err(|_| "MTU must be a number")?;
            if !(1280..=9000).contains(&mtu) {
                return Err("MTU must be between 1280 and 9000".into());
            }
            Ok(Action::SetMtu { dev: dev.clone(), mtu: mtu.to_string() })
        }
        FormKind::VlanCreate { parents } => {
            let name = v(0);
            if name.is_empty() {
                return Err("VLAN name is required".into());
            }
            let parent_device = parents
                .get(form.fields[1].choice)
                .cloned()
                .ok_or("Select a parent device")?;
            let tag: u16 = v(2).parse().map_err(|_| "VLAN tag must be an integer (1–4094)")?;
            if !(1..=4094).contains(&tag) {
                return Err("VLAN tag must be between 1 and 4094".into());
            }
            Ok(Action::CreateVlan { name, parent_device, tag })
        }
        FormKind::VlanEdit { old, parents } => {
            let name = v(0);
            if name.is_empty() {
                return Err("VLAN name is required".into());
            }
            let parent_device = parents
                .get(form.fields[1].choice)
                .cloned()
                .ok_or("Select a parent device")?;
            let tag: u16 = v(2).parse().map_err(|_| "VLAN tag must be an integer (1–4094)")?;
            if !(1..=4094).contains(&tag) {
                return Err("VLAN tag must be between 1 and 4094".into());
            }
            Ok(Action::UpdateVlan { old: old.clone(), name, parent_device, tag })
        }
        FormKind::Join { dev } => {
            let ssid = form.fields[0].value.clone();
            if ssid.trim().is_empty() {
                return Err("SSID is required".into());
            }
            let pw = form.fields[1].value.clone();
            Ok(Action::WifiJoin { dev: dev.clone(), ssid, password: (!pw.is_empty()).then_some(pw) })
        }
        FormKind::JoinKnown { dev, ssid } => {
            let pw = form.fields[0].value.clone();
            Ok(Action::WifiJoin { dev: dev.clone(), ssid: ssid.clone(), password: (!pw.is_empty()).then_some(pw) })
        }
    }
}

fn split_list(s: &str) -> Vec<String> {
    s.split(|c: char| c == ',' || c.is_whitespace())
        .filter(|x| !x.is_empty())
        .map(String::from)
        .collect()
}

fn none_blank(s: &str) -> String {
    if s.eq_ignore_ascii_case("none") { String::new() } else { s.to_string() }
}

fn next_tab(t: Tab, step: usize) -> Tab {
    let i = Tab::ALL.iter().position(|x| *x == t).unwrap_or(0);
    Tab::ALL[(i + step) % Tab::ALL.len()]
}

fn step_index(cur: Option<usize>, n: usize, d: isize) -> Option<usize> {
    if n == 0 {
        return None;
    }
    let i = cur.unwrap_or(0) as isize + d;
    Some(i.clamp(0, n as isize - 1) as usize)
}

fn table_step(s: &mut TableState, n: usize, d: isize) {
    s.select(step_index(s.selected(), n, d));
}

fn list_step(s: &mut ListState, n: usize, d: isize) {
    s.select(step_index(s.selected(), n, d));
}

fn clamp_table(s: &mut TableState, n: usize) {
    s.select(if n == 0 { None } else { Some(s.selected().unwrap_or(0).min(n - 1)) });
}

fn clamp_list(s: &mut ListState, n: usize) {
    s.select(if n == 0 { None } else { Some(s.selected().unwrap_or(0).min(n - 1)) });
}
