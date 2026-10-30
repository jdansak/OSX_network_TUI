//! Native Wi-Fi control through Apple's CoreWLAN framework.
//!
//! Note: since macOS 14, SSID/BSSID values are `nil` unless the process holds
//! Location Services authorization (see `location.rs`).

use objc2::rc::Retained;
use objc2_core_wlan::{
    CWChannel, CWChannelBand, CWChannelWidth, CWInterface, CWNetwork, CWPHYMode, CWSecurity,
    CWWiFiClient,
};
use objc2_foundation::{NSError, NSString};

#[derive(Debug, Clone, Default)]
pub struct Channel {
    pub number: i64,
    pub band: &'static str,
    pub width: &'static str,
}

impl std::fmt::Display for Channel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} ({}, {})", self.number, self.band, self.width)
    }
}

#[derive(Debug, Clone, Default)]
pub struct WifiInfo {
    pub device: String,
    pub power: bool,
    pub associated: bool,
    pub ssid: Option<String>,
    pub bssid: Option<String>,
    pub rssi: i64,
    pub noise: i64,
    pub tx_rate: f64,
    pub channel: Option<Channel>,
    pub security: String,
    pub phy: String,
    pub country: Option<String>,
    pub hw_addr: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct ScanNet {
    pub ssid: Option<String>,
    pub bssid: Option<String>,
    pub rssi: i64,
    pub channel: Option<Channel>,
    pub security: String,
    pub open: bool,
    /// Number of access points (BSSIDs) seen advertising this SSID.
    pub aps: usize,
}

fn err_str(e: &NSError) -> String {
    format!("{} (code {})", e.localizedDescription(), e.code())
}

fn interface(device: Option<&str>) -> Option<Retained<CWInterface>> {
    unsafe {
        let client = CWWiFiClient::sharedWiFiClient();
        match device {
            Some(d) => client.interfaceWithName(Some(&NSString::from_str(d))),
            None => client.interface(),
        }
    }
}

/// Name of the default Wi-Fi interface, e.g. `en0`.
pub fn default_device() -> Option<String> {
    let iface = interface(None)?;
    unsafe { iface.interfaceName() }.map(|s| s.to_string())
}

fn channel(c: Option<Retained<CWChannel>>) -> Option<Channel> {
    let c = c?;
    unsafe {
        let band = match c.channelBand() {
            CWChannelBand::Band2GHz => "2.4 GHz",
            CWChannelBand::Band5GHz => "5 GHz",
            CWChannelBand::Band6GHz => "6 GHz",
            _ => "?",
        };
        let width = match c.channelWidth() {
            CWChannelWidth::Width20MHz => "20 MHz",
            CWChannelWidth::Width40MHz => "40 MHz",
            CWChannelWidth::Width80MHz => "80 MHz",
            CWChannelWidth::Width160MHz => "160 MHz",
            _ => "?",
        };
        Some(Channel { number: c.channelNumber() as i64, band, width })
    }
}

fn security_name(s: CWSecurity) -> &'static str {
    match s {
        CWSecurity::None => "Open",
        CWSecurity::WEP => "WEP",
        CWSecurity::WPAPersonal => "WPA Personal",
        CWSecurity::WPAPersonalMixed => "WPA/WPA2 Personal",
        CWSecurity::WPA2Personal => "WPA2 Personal",
        CWSecurity::Personal => "Personal",
        CWSecurity::DynamicWEP => "Dynamic WEP",
        CWSecurity::WPAEnterprise => "WPA Enterprise",
        CWSecurity::WPAEnterpriseMixed => "WPA/WPA2 Enterprise",
        CWSecurity::WPA2Enterprise => "WPA2 Enterprise",
        CWSecurity::Enterprise => "Enterprise",
        CWSecurity::WPA3Personal => "WPA3 Personal",
        CWSecurity::WPA3Enterprise => "WPA3 Enterprise",
        CWSecurity::WPA3Transition => "WPA2/WPA3 Personal",
        CWSecurity::OWE => "OWE",
        CWSecurity::OWETransition => "OWE Transition",
        _ => "Unknown",
    }
}

fn phy_name(p: CWPHYMode) -> &'static str {
    match p {
        CWPHYMode::Mode11a => "802.11a",
        CWPHYMode::Mode11b => "802.11b",
        CWPHYMode::Mode11g => "802.11g",
        CWPHYMode::Mode11n => "802.11n",
        CWPHYMode::Mode11ac => "802.11ac",
        CWPHYMode::Mode11ax => "802.11ax",
        _ => "-",
    }
}

/// Strongest security mode a scanned network advertises.
fn network_security(n: &CWNetwork) -> (String, bool) {
    // Ordered strongest-first so the first match is the best description.
    const ORDER: [CWSecurity; 15] = [
        CWSecurity::WPA3Enterprise,
        CWSecurity::WPA2Enterprise,
        CWSecurity::WPAEnterpriseMixed,
        CWSecurity::WPAEnterprise,
        CWSecurity::DynamicWEP,
        CWSecurity::WPA3Transition,
        CWSecurity::WPA3Personal,
        CWSecurity::WPA2Personal,
        CWSecurity::WPAPersonalMixed,
        CWSecurity::WPAPersonal,
        CWSecurity::WEP,
        CWSecurity::OWETransition,
        CWSecurity::OWE,
        CWSecurity::Personal,
        CWSecurity::Enterprise,
    ];
    for s in ORDER {
        if unsafe { n.supportsSecurity(s) } {
            // OWE ("Enhanced Open") encrypts but needs no password.
            let open = matches!(s, CWSecurity::OWE | CWSecurity::OWETransition);
            return (security_name(s).to_string(), open);
        }
    }
    ("Open".to_string(), true)
}

pub fn info(device: Option<&str>) -> Option<WifiInfo> {
    let iface = interface(device)?;
    unsafe {
        let power = iface.powerOn();
        let ssid = iface.ssid().map(|s| s.to_string());
        let bssid = iface.bssid().map(|s| s.to_string());
        let rssi = iface.rssiValue() as i64;
        let ch = channel(iface.wlanChannel());
        // `ssid` may be redacted (nil) even when associated, so infer
        // association from a live channel / RSSI.
        let associated = power && (ssid.is_some() || (ch.is_some() && rssi != 0));
        Some(WifiInfo {
            device: iface.interfaceName().map(|s| s.to_string()).unwrap_or_default(),
            power,
            associated,
            ssid,
            bssid,
            rssi,
            noise: iface.noiseMeasurement() as i64,
            tx_rate: iface.transmitRate(),
            channel: ch,
            security: if associated { security_name(iface.security()).to_string() } else { "-".into() },
            phy: phy_name(iface.activePHYMode()).to_string(),
            country: iface.countryCode().map(|s| s.to_string()),
            hw_addr: iface.hardwareAddress().map(|s| s.to_string()),
        })
    }
}

fn to_scan(n: &CWNetwork) -> ScanNet {
    let (security, open) = network_security(n);
    unsafe {
        ScanNet {
            ssid: n.ssid().map(|s| s.to_string()),
            bssid: n.bssid().map(|s| s.to_string()),
            rssi: n.rssiValue() as i64,
            channel: channel(n.wlanChannel()),
            security,
            open,
            aps: 1,
        }
    }
}

/// Blocking scan (a few seconds). Results are sorted strongest-first.
pub fn scan(device: Option<&str>) -> Result<Vec<ScanNet>, String> {
    let iface = interface(device).ok_or("no Wi-Fi interface")?;
    let set = unsafe { iface.scanForNetworksWithName_includeHidden_error(None, true) }
        .map_err(|e| err_str(&e))?;
    let mut nets: Vec<ScanNet> = set.iter().map(|n| to_scan(&n)).collect();
    nets.sort_by(|a, b| b.rssi.cmp(&a.rssi));
    // Collapse multiple BSSIDs of the same named network into one row,
    // keeping the strongest. Redacted (nil) SSIDs can't be grouped.
    let mut out: Vec<ScanNet> = Vec::with_capacity(nets.len());
    for n in nets {
        match &n.ssid {
            Some(s) if !s.is_empty() => {
                if let Some(existing) = out.iter_mut().find(|e| e.ssid.as_ref() == Some(s)) {
                    existing.aps += 1;
                } else {
                    out.push(n);
                }
            }
            _ => out.push(n),
        }
    }
    let nets = out;
    Ok(nets)
}

pub enum JoinError {
    /// CoreWLAN could not see the SSID (redacted or out of range); caller may fall back.
    NotFound,
    Failed(String),
}

/// Join `ssid` using CoreWLAN. Performs a directed scan (finds hidden
/// networks too) and associates to the strongest matching BSS.
pub fn associate(device: Option<&str>, ssid: &str, password: Option<&str>) -> Result<(), JoinError> {
    let iface = interface(device).ok_or_else(|| JoinError::Failed("no Wi-Fi interface".into()))?;
    let name = NSString::from_str(ssid);
    let set = unsafe { iface.scanForNetworksWithName_includeHidden_error(Some(&name), true) }
        .map_err(|e| JoinError::Failed(err_str(&e)))?;
    let best = set
        .iter()
        .filter(|n| unsafe { n.ssid() }.map(|s| s.to_string()).as_deref() == Some(ssid))
        .max_by_key(|n| unsafe { n.rssiValue() });
    let Some(net) = best else { return Err(JoinError::NotFound) };
    let pw = password.filter(|p| !p.is_empty()).map(NSString::from_str);
    unsafe { iface.associateToNetwork_password_error(&net, pw.as_deref()) }
        .map_err(|e| JoinError::Failed(err_str(&e)))
}

pub fn disassociate(device: Option<&str>) -> Result<(), String> {
    let iface = interface(device).ok_or("no Wi-Fi interface")?;
    unsafe { iface.disassociate() };
    Ok(())
}

pub fn set_power(device: Option<&str>, on: bool) -> Result<(), String> {
    let iface = interface(device).ok_or("no Wi-Fi interface")?;
    unsafe { iface.setPower_error(on) }.map_err(|e| err_str(&e))
}
