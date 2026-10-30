//! Rendering.

use ratatui::Frame;
use ratatui::layout::{Alignment, Constraint, Flex, Layout, Rect};
use ratatui::style::{Color, Modifier, Style, Stylize};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, BorderType, Cell, Clear, List, ListItem, Paragraph, Row, Table, Tabs, Wrap};

use crate::app::{App, Modal, StatusKind, Tab, WifiFocus};
use crate::sys::cmd::is_root;
use crate::sys::location::LocAuth;
use crate::worker::ServiceView;

const ACCENT: Color = Color::Cyan;
const SPINNER: [&str; 10] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];

pub fn draw(f: &mut Frame, app: &mut App) {
    let [header, body, footer] =
        Layout::vertical([Constraint::Length(1), Constraint::Min(5), Constraint::Length(2)]).areas(f.area());
    draw_header(f, app, header);
    match app.tab {
        Tab::Interfaces => draw_interfaces(f, app, body),
        Tab::Wifi => draw_wifi(f, app, body),
        Tab::Locations => draw_locations(f, app, body),
    }
    draw_footer(f, app, footer);
    if app.modal.is_some() {
        draw_modal(f, app);
    }
}

fn draw_header(f: &mut Frame, app: &App, area: Rect) {
    let [left, right] = Layout::horizontal([Constraint::Min(30), Constraint::Length(52)]).areas(area);
    let titles: Vec<Line> = Tab::ALL.iter().map(|t| Line::from(format!(" {} ", t.title()))).collect();
    let idx = Tab::ALL.iter().position(|t| *t == app.tab).unwrap_or(0);
    let tabs = Tabs::new(titles)
        .select(idx)
        .style(Style::new().fg(Color::Gray))
        .highlight_style(Style::new().fg(Color::Black).bg(ACCENT).bold())
        .divider("")
        .padding("", "");
    let title = Span::styled(" nettui ", Style::new().fg(Color::Black).bg(Color::Magenta).bold());
    let [t, tb] = Layout::horizontal([Constraint::Length(9), Constraint::Min(10)]).areas(left);
    f.render_widget(Paragraph::new(title), t);
    f.render_widget(tabs, tb);

    let busy = app.busy > 0 || app.refreshing || app.scanning;
    let spin = if busy { SPINNER[(app.tick as usize) % SPINNER.len()] } else { " " };
    let loc_color = match app.loc_auth {
        LocAuth::Authorized => Color::Green,
        LocAuth::NotDetermined => Color::Yellow,
        _ => Color::Red,
    };
    let user = if is_root() { Span::styled(" root ", Style::new().fg(Color::Red).bold()) } else { Span::raw(" user ") };
    let line = Line::from(vec![
        Span::styled(spin, Style::new().fg(ACCENT)),
        Span::raw(" "),
        Span::styled(app.loc_auth.label(), Style::new().fg(loc_color)),
        Span::raw(" │"),
        user,
        Span::raw("│ loc: "),
        Span::styled(app.snap.current_location.clone(), Style::new().fg(ACCENT)),
    ])
    .alignment(Alignment::Right);
    f.render_widget(Paragraph::new(line), right);
}

fn draw_footer(f: &mut Frame, app: &App, area: Rect) {
    let [status, keys] = Layout::vertical([Constraint::Length(1), Constraint::Length(1)]).areas(area);
    if let Some((msg, kind, _)) = &app.status {
        let color = match kind {
            StatusKind::Info => Color::Gray,
            StatusKind::Ok => Color::Green,
            StatusKind::Err => Color::Red,
        };
        f.render_widget(Paragraph::new(Span::styled(format!(" {msg}"), Style::new().fg(color))), status);
    }
    let hints: &[(&str, &str)] = match app.tab {
        Tab::Interfaces => &[
            ("↑↓", "select"),
            ("e", "IPv4"),
            ("d", "DHCP"),
            ("n", "DNS"),
            ("t", "on/off"),
            ("c", "vpn"),
            ("v", "+vlan"),
            ("g", "tag"),
            ("x", "del"),
            ("r", "renew"),
            ("m", "MTU"),
        ],
        Tab::Wifi => &[
            ("s", "scan"),
            ("⏎", "join"),
            ("c", "join SSID"),
            ("x", "disconnect"),
            ("p", "power"),
            ("Tab", "focus"),
            ("f", "forget"),
            ("L", "location"),
        ],
        Tab::Locations => &[("↑↓", "select"), ("⏎", "switch")],
    };
    let mut spans = Vec::new();
    for (k, d) in hints.iter().chain([("R", "refresh"), ("?", "help"), ("q", "quit")].iter()) {
        spans.push(Span::styled(
            format!(" {k} "),
            Style::new().fg(Color::Black).bg(ACCENT).bold(),
        ));
        spans.push(Span::styled(format!(" {d} "), Style::new().fg(Color::White)));
    }
    f.render_widget(Paragraph::new(Line::from(spans)), keys);
}

fn block(title: &str, focused: bool) -> Block<'static> {
    let style = if focused { Style::new().fg(ACCENT) } else { Style::new().fg(Color::DarkGray) };
    Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(style)
        .title(Span::styled(format!(" {title} "), Style::new().bold().fg(if focused { ACCENT } else { Color::Gray })))
}

// ---------------------------------------------------------------- interfaces

fn svc_state(sv: &ServiceView) -> (&'static str, Color) {
    if !sv.service.enabled {
        ("○", Color::DarkGray)
    } else if let Some(vpn) = &sv.vpn {
        if vpn.is_connected() {
            ("●", Color::Green)
        } else {
            ("◌", Color::Blue)
        }
    } else if sv.live.as_ref().is_some_and(|l| l.is_active()) {
        ("●", Color::Green)
    } else if sv.service.device.is_empty() {
        ("◌", Color::Blue)
    } else {
        ("●", Color::Red)
    }
}

fn draw_interfaces(f: &mut Frame, app: &mut App, area: Rect) {
    let [list_area, detail_area] =
        Layout::horizontal([Constraint::Percentage(48), Constraint::Percentage(52)]).areas(area);

    let rows: Vec<Row> = app
        .snap
        .services
        .iter()
        .map(|sv| {
            let (dot, color) = svc_state(sv);
            let ip = sv
                .live
                .as_ref()
                .and_then(|l| l.ipv4.first().map(|(a, _)| a.to_string()))
                .or_else(|| sv.vpn.as_ref().and_then(|v| v.ipv4.clone()))
                .or_else(|| (!sv.config.ip.is_empty()).then(|| sv.config.ip.clone()))
                .unwrap_or_else(|| "-".into());
            let dev_col = if let Some(vl) = &sv.vlan {
                format!("{}·{}", sv.service.device, vl.tag)
            } else {
                sv.service.device.clone()
            };
            let name_style = if sv.service.enabled { Style::new() } else { Style::new().fg(Color::DarkGray) };
            Row::new(vec![
                Cell::from(Span::styled(dot, Style::new().fg(color))),
                Cell::from(Span::styled(sv.service.name.clone(), name_style)),
                Cell::from(dev_col),
                Cell::from(short_mode(&sv.config.mode)),
                Cell::from(ip),
            ])
        })
        .collect();
    let table = Table::new(
        rows,
        [Constraint::Length(1), Constraint::Min(14), Constraint::Length(9), Constraint::Length(7), Constraint::Length(15)],
    )
    .header(Row::new(["", "Service", "Device", "Mode", "IPv4"]).style(Style::new().fg(Color::Yellow).bold()))
    .block(block("Network services", true))
    .row_highlight_style(Style::new().bg(Color::Rgb(40, 50, 70)).add_modifier(Modifier::BOLD))
    .highlight_symbol("▶ ");
    f.render_stateful_widget(table, list_area, &mut app.svc_state);

    let Some(sv) = app.selected_service() else {
        let msg = if app.refreshing { "Loading…" } else { "No services" };
        f.render_widget(Paragraph::new(msg).block(block("Details", false)), detail_area);
        return;
    };
    let mut lines: Vec<Line> = Vec::new();
    let kv = |k: &str, v: String| {
        Line::from(vec![Span::styled(format!("{k:<14}"), Style::new().fg(Color::Gray)), Span::raw(v)])
    };
    let section = |t: &str| Line::from(Span::styled(t.to_string(), Style::new().fg(Color::Yellow).bold()));
    let (dot, color) = svc_state(sv);
    let state = if !sv.service.enabled {
        "Disabled"
    } else if let Some(vpn) = &sv.vpn {
        &vpn.state
    } else if sv.live.as_ref().is_some_and(|l| l.is_active()) {
        "Connected"
    } else {
        "Not connected"
    };
    lines.push(Line::from(vec![
        Span::styled(format!("{dot} "), Style::new().fg(color)),
        Span::styled(sv.service.name.clone(), Style::new().bold()),
        Span::raw(format!("  ({state})")),
    ]));
    lines.push(kv("Hardware", sv.service.port.clone()));
    lines.push(kv("Device", or_dash(&sv.service.device)));
    lines.push(kv("Order", if sv.service.order > 0 { sv.service.order.to_string() } else { "-".into() }));
    if let Some(vpn) = &sv.vpn {
        lines.push(Line::raw(""));
        lines.push(section("VPN Tunnel (press c or Enter to connect/disconnect)"));
        lines.push(kv("Status", vpn.state.clone()));
        lines.push(kv("Tunnel dev", vpn.tunnel_device.clone().unwrap_or_else(|| "-".into())));
        lines.push(kv("Tunnel IPv4", vpn.ipv4.clone().unwrap_or_else(|| "-".into())));
        lines.push(kv("Tunnel gw", vpn.router.clone().unwrap_or_else(|| "-".into())));
    }
    if let Some(vl) = &sv.vlan {
        lines.push(Line::raw(""));
        lines.push(section("802.1Q VLAN (g edit tag/parent · x delete · v new VLAN)"));
        lines.push(kv("VLAN name", vl.name.clone()));
        lines.push(kv("Parent dev", vl.parent_device.clone()));
        lines.push(kv("VLAN device", vl.device.clone()));
        lines.push(kv("VLAN tag", vl.tag.to_string()));
    }
    lines.push(Line::raw(""));
    lines.push(section("IPv4 (configured)"));
    lines.push(kv("Configure", sv.config.mode.clone()));
    lines.push(kv("IP address", or_dash(&sv.config.ip)));
    lines.push(kv("Subnet mask", or_dash(&sv.config.mask)));
    lines.push(kv("Router", or_dash(&sv.config.router)));
    if !sv.config.client_id.is_empty() {
        lines.push(kv("Client ID", sv.config.client_id.clone()));
    }
    lines.push(kv("IPv6", or_dash(&sv.config.ipv6)));
    lines.push(Line::raw(""));
    lines.push(section("DNS"));
    lines.push(kv("Servers", if sv.dns.is_empty() { "automatic".into() } else { sv.dns.join(", ") }));
    lines.push(kv("Search", if sv.search.is_empty() { "automatic".into() } else { sv.search.join(", ") }));
    if let Some(l) = &sv.live {
        lines.push(Line::raw(""));
        lines.push(section("Interface (live)"));
        lines.push(kv("Status", l.status.clone().unwrap_or_else(|| if l.up { "up".into() } else { "down".into() })));
        if let Some(m) = &l.media {
            lines.push(kv("Media", m.clone()));
        }
        if let Some(mac) = &l.mac {
            lines.push(kv("MAC", mac.clone()));
        }
        lines.push(kv("MTU", sv.mtu.clone().or(l.mtu.map(|m| m.to_string())).unwrap_or_else(|| "-".into())));
        for (a, m) in &l.ipv4 {
            lines.push(kv("inet", format!("{a}/{}", u32::from(*m).count_ones())));
        }
        for a in &l.ipv6 {
            lines.push(kv("inet6", a.to_string()));
        }
    }
    f.render_widget(
        Paragraph::new(Text::from(lines)).wrap(Wrap { trim: false }).block(block("Details", false)),
        detail_area,
    );
}

fn short_mode(m: &str) -> String {
    match m {
        "Manual with DHCP router" => "Man+R".into(),
        "Manual" => "Manual".into(),
        "DHCP" => "DHCP".into(),
        "BOOTP" => "BOOTP".into(),
        "" => "-".into(),
        other => other.chars().take(7).collect(),
    }
}

// --------------------------------------------------------------------- wifi

fn bars(rssi: i64) -> (&'static str, Color) {
    match rssi {
        0 => ("    ", Color::DarkGray),
        r if r >= -55 => ("▂▄▆█", Color::Green),
        r if r >= -67 => ("▂▄▆ ", Color::Green),
        r if r >= -75 => ("▂▄  ", Color::Yellow),
        _ => ("▂   ", Color::Red),
    }
}

fn draw_wifi(f: &mut Frame, app: &mut App, area: Rect) {
    let [top, bottom] = Layout::vertical([Constraint::Length(8), Constraint::Min(5)]).areas(area);
    let [scan_area, pref_area] =
        Layout::horizontal([Constraint::Percentage(66), Constraint::Percentage(34)]).areas(bottom);

    // Current connection
    let mut lines = Vec::new();
    match &app.wifi {
        None => lines.push(Line::raw("No Wi-Fi interface detected")),
        Some(w) => {
            let power = if w.power {
                Span::styled("On", Style::new().fg(Color::Green).bold())
            } else {
                Span::styled("Off", Style::new().fg(Color::Red).bold())
            };
            let ssid = match (&w.ssid, w.associated) {
                (Some(s), _) => Span::styled(s.clone(), Style::new().fg(ACCENT).bold()),
                (None, true) => Span::styled("<redacted – press L>", Style::new().fg(Color::Yellow)),
                (None, false) => Span::styled("not connected", Style::new().fg(Color::DarkGray)),
            };
            let (b, bc) = bars(if w.associated { w.rssi } else { 0 });
            lines.push(Line::from(vec![
                Span::styled("Power  ", Style::new().fg(Color::Gray)),
                power,
                Span::styled(format!("   Device {}", w.device), Style::new().fg(Color::Gray)),
                Span::styled(format!("   MAC {}", w.hw_addr.clone().unwrap_or_default()), Style::new().fg(Color::Gray)),
            ]));
            lines.push(Line::from(vec![Span::styled("SSID   ", Style::new().fg(Color::Gray)), ssid]));
            if w.associated {
                lines.push(Line::from(vec![
                    Span::styled("Signal ", Style::new().fg(Color::Gray)),
                    Span::styled(b, Style::new().fg(bc)),
                    Span::raw(format!(
                        " {} dBm  noise {} dBm  SNR {} dB",
                        w.rssi,
                        w.noise,
                        w.rssi - w.noise
                    )),
                ]));
                lines.push(Line::from(vec![
                    Span::styled("Link   ", Style::new().fg(Color::Gray)),
                    Span::raw(format!(
                        "{}  ch {}  {:.0} Mbps",
                        w.phy,
                        w.channel.as_ref().map(|c| c.to_string()).unwrap_or_else(|| "-".into()),
                        w.tx_rate
                    )),
                ]));
                lines.push(Line::from(vec![
                    Span::styled("Secur. ", Style::new().fg(Color::Gray)),
                    Span::raw(format!(
                        "{}   BSSID {}   Country {}",
                        w.security,
                        w.bssid.clone().unwrap_or_else(|| "-".into()),
                        w.country.clone().unwrap_or_else(|| "-".into())
                    )),
                ]));
            }
        }
    }
    f.render_widget(Paragraph::new(lines).block(block("Current connection", false)), top);

    // Scan results
    let rows: Vec<Row> = app
        .scan
        .iter()
        .map(|n| {
            let (b, bc) = bars(n.rssi);
            let ssid = match &n.ssid {
                Some(s) if s.is_empty() => Span::styled("<hidden>", Style::new().fg(Color::DarkGray)),
                Some(s) => {
                    let connected = app.wifi.as_ref().and_then(|w| w.ssid.as_ref()) == Some(s);
                    let st = if connected { Style::new().fg(Color::Green).bold() } else { Style::new() };
                    let mut label = s.clone();
                    if n.aps > 1 {
                        label.push_str(&format!(" ×{}", n.aps));
                    }
                    if connected {
                        label.push_str(" ✓");
                    }
                    Span::styled(label, st)
                }
                None => Span::styled("<redacted>", Style::new().fg(Color::DarkGray)),
            };
            let sec_style = if n.open { Style::new().fg(Color::Green) } else { Style::new().fg(Color::Yellow) };
            Row::new(vec![
                Cell::from(ssid),
                Cell::from(Span::styled(b, Style::new().fg(bc))),
                Cell::from(format!("{}", n.rssi)),
                Cell::from(n.channel.as_ref().map(|c| c.number.to_string()).unwrap_or_default()),
                Cell::from(n.channel.as_ref().map(|c| c.band).unwrap_or("")),
                Cell::from(Span::styled(n.security.clone(), sec_style)),
            ])
        })
        .collect();
    let title = if app.scanning {
        "Nearby networks (scanning…)".to_string()
    } else if app.scan.is_empty() {
        "Nearby networks (press s to scan)".to_string()
    } else {
        format!("Nearby networks ({})", app.scan.len())
    };
    let table = Table::new(
        rows,
        [
            Constraint::Min(16),
            Constraint::Length(4),
            Constraint::Length(4),
            Constraint::Length(4),
            Constraint::Length(7),
            Constraint::Length(22),
        ],
    )
    .header(Row::new(["SSID", "Sig", "dBm", "Ch", "Band", "Security"]).style(Style::new().fg(Color::Yellow).bold()))
    .block(block(&title, app.wifi_focus == WifiFocus::Scan))
    .row_highlight_style(Style::new().bg(Color::Rgb(40, 50, 70)).add_modifier(Modifier::BOLD))
    .highlight_symbol("▶ ");
    f.render_stateful_widget(table, scan_area, &mut app.scan_state);

    // Preferred networks
    let items: Vec<ListItem> = app.snap.preferred.iter().map(|s| ListItem::new(s.clone())).collect();
    let list = List::new(items)
        .block(block(
            &format!("Saved networks ({})", app.snap.preferred.len()),
            app.wifi_focus == WifiFocus::Preferred,
        ))
        .highlight_style(Style::new().bg(Color::Rgb(40, 50, 70)).add_modifier(Modifier::BOLD))
        .highlight_symbol("▶ ");
    f.render_stateful_widget(list, pref_area, &mut app.pref_state);
}

// ---------------------------------------------------------------- locations

fn draw_locations(f: &mut Frame, app: &mut App, area: Rect) {
    let [list_area, info_area] =
        Layout::horizontal([Constraint::Percentage(40), Constraint::Percentage(60)]).areas(area);
    let items: Vec<ListItem> = app
        .snap
        .locations
        .iter()
        .map(|l| {
            if *l == app.snap.current_location {
                ListItem::new(Line::from(vec![
                    Span::styled("● ", Style::new().fg(Color::Green)),
                    Span::styled(l.clone(), Style::new().bold()),
                    Span::styled("  (current)", Style::new().fg(Color::Gray)),
                ]))
            } else {
                ListItem::new(format!("  {l}"))
            }
        })
        .collect();
    let list = List::new(items)
        .block(block("Network locations", true))
        .highlight_style(Style::new().bg(Color::Rgb(40, 50, 70)).add_modifier(Modifier::BOLD))
        .highlight_symbol("▶ ");
    f.render_stateful_widget(list, list_area, &mut app.loc_state);
    let text = "A location is a saved set of network service configurations.\n\n\
                Switching location applies all of its services' settings at once \
                (e.g. a static \"Office\" profile vs. DHCP \"Automatic\").\n\n\
                Create or rename locations in System Settings ▸ Network ▸ ··· ▸ Locations.";
    f.render_widget(
        Paragraph::new(text).wrap(Wrap { trim: true }).block(block("About", false)),
        info_area,
    );
}

// ------------------------------------------------------------------- modals

fn centered(area: Rect, w: u16, h: u16) -> Rect {
    let [v] = Layout::vertical([Constraint::Length(h.min(area.height))]).flex(Flex::Center).areas(area);
    let [r] = Layout::horizontal([Constraint::Length(w.min(area.width))]).flex(Flex::Center).areas(v);
    r
}

fn draw_modal(f: &mut Frame, app: &App) {
    let Some(modal) = &app.modal else { return };
    match modal {
        Modal::Confirm { message, .. } => {
            let r = centered(f.area(), (message.len() as u16 + 8).max(36), 5);
            f.render_widget(Clear, r);
            let text = vec![
                Line::raw(message.clone()).bold(),
                Line::raw(""),
                Line::from(vec![
                    Span::styled(" y ", Style::new().fg(Color::Black).bg(Color::Green)),
                    Span::raw(" yes   "),
                    Span::styled(" n ", Style::new().fg(Color::Black).bg(Color::Red)),
                    Span::raw(" no"),
                ]),
            ];
            f.render_widget(
                Paragraph::new(text).alignment(Alignment::Center).block(block("Confirm", true)),
                r,
            );
        }
        Modal::Password { action, input, error } => {
            let r = centered(f.area(), 60, 8);
            f.render_widget(Clear, r);
            let mut text = vec![
                Line::raw(format!("\"{}\" requires administrator rights.", action.describe())),
                Line::raw("Enter your macOS password (used for sudo, kept in memory only):"),
                Line::raw(""),
                Line::from(vec![
                    Span::styled("Password: ", Style::new().fg(Color::Gray)),
                    Span::styled("•".repeat(input.chars().count()), Style::new().fg(ACCENT)),
                    Span::styled("█", Style::new().fg(ACCENT)),
                ]),
            ];
            if let Some(e) = error {
                text.push(Line::styled(e.clone(), Style::new().fg(Color::Red)));
            }
            f.render_widget(
                Paragraph::new(text).wrap(Wrap { trim: false }).block(block("Authentication", true)),
                r,
            );
        }
        Modal::Form(form) => {
            let h = form.fields.len() as u16 + 6 + form.error.is_some() as u16;
            let r = centered(f.area(), 64, h);
            f.render_widget(Clear, r);
            let mut lines = vec![Line::styled(form.help.clone(), Style::new().fg(Color::Gray)), Line::raw("")];
            for (i, fld) in form.fields.iter().enumerate() {
                let focused = i == form.focus;
                let label_style = if focused { Style::new().fg(ACCENT).bold() } else { Style::new().fg(Color::Gray) };
                let mut spans = vec![
                    Span::styled(if focused { "▶ " } else { "  " }, Style::new().fg(ACCENT)),
                    Span::styled(format!("{:<14}", fld.label), label_style),
                    Span::styled(
                        fld.display_value(),
                        if focused { Style::new().bg(Color::Rgb(40, 50, 70)) } else { Style::new() },
                    ),
                ];
                if focused && fld.choices.is_none() {
                    spans.push(Span::styled("█", Style::new().fg(ACCENT)));
                }
                lines.push(Line::from(spans));
            }
            if let Some(e) = &form.error {
                lines.push(Line::raw(""));
                lines.push(Line::styled(format!("✗ {e}"), Style::new().fg(Color::Red)));
            }
            f.render_widget(Paragraph::new(lines).block(block(&form.title, true)), r);
        }
        Modal::Help => {
            let r = centered(f.area(), 76, 29);
            f.render_widget(Clear, r);
            let h = |k: &str, d: &str| {
                Line::from(vec![Span::styled(format!("  {k:<12}"), Style::new().fg(ACCENT)), Span::raw(d.to_string())])
            };
            let s = |t: &str| Line::styled(t.to_string(), Style::new().fg(Color::Yellow).bold());
            let lines = vec![
                s("Global"),
                h("1 2 3 / ← →", "switch tabs"),
                h("R / F5", "refresh everything"),
                h("L", "request Location Services access (reveals SSIDs)"),
                h("q / Ctrl-C", "quit"),
                s("Interfaces, VPNs & VLANs"),
                h("e / Enter", "edit IPv4 (or connect/disconnect if VPN selected)"),
                h("c", "connect / disconnect selected VPN service"),
                h("v", "create a new 802.1Q VLAN interface"),
                h("g", "edit VLAN tag / parent device on selected VLAN"),
                h("x / Del", "delete selected VLAN interface (or disconnect VPN)"),
                h("d", "switch to DHCP"),
                h("n / s", "edit DNS servers / search domains"),
                h("t / Space", "enable / disable service"),
                h("r / 6 / m", "renew DHCP lease / toggle IPv6 / set MTU"),
                s("Wi-Fi"),
                h("s", "scan for networks"),
                h("Enter", "join selected (scan list or saved network)"),
                h("c", "join by SSID (incl. hidden networks)"),
                h("x / p", "disconnect / toggle Wi-Fi power"),
                h("Tab", "switch between scan list and saved networks"),
                h("f / Del", "forget saved network"),
                s("Locations"),
                h("Enter", "switch to selected network location"),
                Line::raw(""),
                Line::styled("  Changes that need admin rights prompt for your sudo password.", Style::new().fg(Color::Gray)),
            ];
            f.render_widget(Paragraph::new(lines).block(block("Help – press any key", true)), r);
        }
    }
}

fn or_dash(s: &str) -> String {
    if s.is_empty() { "-".into() } else { s.to_string() }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    fn dump(term: &Terminal<TestBackend>) -> String {
        let buf = term.backend().buffer();
        let mut s = String::new();
        for y in 0..buf.area.height {
            for x in 0..buf.area.width {
                s.push_str(buf[(x, y)].symbol());
            }
            s.push('\n');
        }
        s
    }

    /// Renders every screen with live system data. Run with
    /// `cargo test render_all -- --nocapture` to eyeball the layout.
    #[test]
    fn render_all() {
        let (tx, _rx) = std::sync::mpsc::channel();
        let mut app = App::new(tx, LocAuth::NotDetermined);
        app.on_msg(crate::worker::Msg::Snapshot(crate::worker::refresh()));
        app.on_msg(crate::worker::Msg::Wifi(crate::sys::wifi::info(None)));
        app.on_msg(crate::worker::Msg::Scan(crate::sys::wifi::scan(None)));
        let mut term = Terminal::new(TestBackend::new(130, 38)).unwrap();
        let key = |c| KeyEvent::new(c, KeyModifiers::NONE);

        term.draw(|f| draw(f, &mut app)).unwrap();
        println!("{}", dump(&term));
        app.on_key(key(KeyCode::Char('e')));
        term.draw(|f| draw(f, &mut app)).unwrap();
        println!("{}", dump(&term));
        app.on_key(key(KeyCode::Esc));
        app.on_key(key(KeyCode::Char('2')));
        term.draw(|f| draw(f, &mut app)).unwrap();
        println!("{}", dump(&term));
        app.on_key(key(KeyCode::Char('c')));
        term.draw(|f| draw(f, &mut app)).unwrap();
        println!("{}", dump(&term));
        app.on_key(key(KeyCode::Esc));
        app.on_key(key(KeyCode::Char('3')));
        term.draw(|f| draw(f, &mut app)).unwrap();
        println!("{}", dump(&term));
        app.on_key(key(KeyCode::Char('?')));
        term.draw(|f| draw(f, &mut app)).unwrap();
        println!("{}", dump(&term));
    }
}
