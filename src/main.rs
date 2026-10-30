//! nettui – a terminal UI for macOS network configuration.
//!
//! Controls wired and wireless network services: IPv4 (DHCP / manual),
//! DNS, search domains, service enable/disable, MTU, IPv6, Wi-Fi power,
//! scanning, joining (incl. hidden networks), saved networks and network
//! locations.

mod app;
mod sys;
mod ui;
mod worker;

use std::time::{Duration, Instant};

use ratatui::crossterm::event::{self, Event, KeyEventKind};

use crate::app::App;
use crate::sys::location::{self, LocAuth, Location};

const TICK: Duration = Duration::from_millis(100);

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("-h" | "--help") => {
            println!(
                "nettui {}\n\nUsage:\n  nettui            launch the TUI\n  nettui --status   print a non-interactive status report\n  nettui --scan     scan and list nearby Wi-Fi networks\n  nettui --authorize-location\n                    request Location Services access (needed to see SSIDs)\n  nettui --version  print version",
                env!("CARGO_PKG_VERSION")
            );
            return Ok(());
        }
        Some("-V" | "--version") => {
            println!("nettui {}", env!("CARGO_PKG_VERSION"));
            return Ok(());
        }
        Some("--status") => return status_report(),
        Some("--scan") => return scan_report(),
        Some("--authorize-location") => return authorize_location(),
        Some(other) => anyhow::bail!("unknown argument: {other} (try --help)"),
        None => {}
    }

    let mut loc = Location::new();
    // Ask once at startup if the user has never been asked.
    if loc.status() == LocAuth::NotDetermined {
        loc.request();
    }

    let (tx, rx) = worker::spawn();
    let mut app = App::new(tx, loc.status());

    let mut terminal = ratatui::init();
    let result = (|| -> anyhow::Result<()> {
        let mut last_tick = Instant::now();
        while !app.quit {
            terminal.draw(|f| ui::draw(f, &mut app))?;

            let timeout = TICK.saturating_sub(last_tick.elapsed());
            if event::poll(timeout)? {
                if let Event::Key(key) = event::read()? {
                    if key.kind == KeyEventKind::Press {
                        app.on_key(key);
                    }
                }
            }
            while let Ok(msg) = rx.try_recv() {
                app.on_msg(msg);
            }
            if last_tick.elapsed() >= TICK {
                last_tick = Instant::now();
                // Keep CoreLocation alive on the main thread.
                location::pump_runloop();
                if app.want_location_request {
                    app.want_location_request = false;
                    loc.request();
                }
                loc.settle();
                let st = loc.status();
                if st != app.loc_auth {
                    app.loc_auth = st;
                    if st == LocAuth::Authorized {
                        app.set_status("Location access granted – SSIDs now visible", app::StatusKind::Ok);
                    }
                }
                app.on_tick();
            }
        }
        Ok(())
    })();
    ratatui::restore();
    result
}

fn status_report() -> anyhow::Result<()> {
    let loc = Location::new();
    println!("{}", loc.status().label());
    let wifi_dev = sys::wifi::default_device();
    if let Some(w) = sys::wifi::info(wifi_dev.as_deref()) {
        println!(
            "Wi-Fi {}: power={} associated={} ssid={} bssid={} rssi={} noise={} ch={} rate={} sec={} phy={}",
            w.device,
            w.power,
            w.associated,
            w.ssid.as_deref().unwrap_or("<redacted>"),
            w.bssid.as_deref().unwrap_or("-"),
            w.rssi,
            w.noise,
            w.channel.map(|c| c.to_string()).unwrap_or_default(),
            w.tx_rate,
            w.security,
            w.phy
        );
    }
    let snap = worker::refresh();
    for sv in &snap.services {
        let s = &sv.service;
        let cfg = &sv.config;
        let status = if let Some(vpn) = &sv.vpn {
            vpn.state.clone()
        } else {
            sv.live
                .as_ref()
                .and_then(|l| l.status.clone())
                .unwrap_or_else(|| "-".into())
        };
        let dev = if let Some(vl) = &sv.vlan {
            format!("{}·{}(->{})", s.device, vl.tag, vl.parent_device)
        } else {
            s.device.clone()
        };
        println!(
            "{} {:<40} {:<15} {:<8} ip={:<15} mask={:<15} router={:<15} status={} dns={:?}",
            if s.enabled { "*" } else { " " },
            s.name,
            dev,
            cfg.mode,
            cfg.ip,
            cfg.mask,
            cfg.router,
            status,
            sv.dns,
        );
    }
    if let Some(d) = wifi_dev {
        println!("Saved networks: {:?}", sys::netsetup::preferred_wifi(&d));
    }
    println!(
        "Locations: {:?} (current: {})",
        sys::netsetup::list_locations(),
        sys::netsetup::current_location()
    );
    Ok(())
}

fn authorize_location() -> anyhow::Result<()> {
    let mut loc = Location::new();
    let start = loc.status();
    println!("{}", start.label());
    if start != LocAuth::NotDetermined {
        if start != LocAuth::Authorized {
            println!(
                "Enable it in System Settings ▸ Privacy & Security ▸ Location Services (entry: nettui)."
            );
        }
        return Ok(());
    }
    loc.request();
    println!("Waiting for you to answer the macOS prompt (60s)…");
    let deadline = Instant::now() + Duration::from_secs(60);
    while Instant::now() < deadline {
        location::pump_runloop();
        std::thread::sleep(Duration::from_millis(100));
        let st = loc.status();
        if st != LocAuth::NotDetermined {
            loc.settle();
            println!("{}", st.label());
            return Ok(());
        }
    }
    println!("No answer received; status still: {}", loc.status().label());
    Ok(())
}

fn scan_report() -> anyhow::Result<()> {
    let nets = sys::wifi::scan(None).map_err(|e| anyhow::anyhow!(e))?;
    for n in nets {
        println!(
            "{:<32} {:<18} {:>4} dBm  ch {:<20} {}",
            n.ssid.as_deref().unwrap_or("<redacted>"),
            n.bssid.as_deref().unwrap_or("-"),
            n.rssi,
            n.channel.map(|c| c.to_string()).unwrap_or_default(),
            n.security
        );
    }
    Ok(())
}
