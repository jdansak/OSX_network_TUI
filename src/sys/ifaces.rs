//! Live interface state: addresses via `getifaddrs(3)`, link status/media via
//! `ifconfig`.

use std::collections::HashMap;
use std::ffi::CStr;
use std::net::{Ipv4Addr, Ipv6Addr};

use super::cmd::{self, IFCONFIG};

#[derive(Debug, Clone, Default)]
pub struct IfaceLive {
    pub up: bool,
    pub running: bool,
    pub ipv4: Vec<(Ipv4Addr, Ipv4Addr)>, // (addr, netmask)
    pub ipv6: Vec<Ipv6Addr>,
    pub mac: Option<String>,
    /// "active" / "inactive" from ifconfig, when reported.
    pub status: Option<String>,
    pub media: Option<String>,
    pub mtu: Option<u32>,
}

impl IfaceLive {
    pub fn is_active(&self) -> bool {
        match self.status.as_deref() {
            Some(s) => s == "active",
            None => self.up && self.running && !self.ipv4.is_empty(),
        }
    }
}

pub fn snapshot() -> HashMap<String, IfaceLive> {
    let mut map: HashMap<String, IfaceLive> = HashMap::new();
    unsafe {
        let mut head: *mut libc::ifaddrs = std::ptr::null_mut();
        if libc::getifaddrs(&mut head) == 0 {
            let mut cur = head;
            while !cur.is_null() {
                let ifa = &*cur;
                let name = CStr::from_ptr(ifa.ifa_name).to_string_lossy().into_owned();
                let entry = map.entry(name).or_default();
                let flags = ifa.ifa_flags as i32;
                entry.up = flags & libc::IFF_UP != 0;
                entry.running = flags & libc::IFF_RUNNING != 0;
                if !ifa.ifa_addr.is_null() {
                    match (*ifa.ifa_addr).sa_family as i32 {
                        libc::AF_INET => {
                            let sin = &*(ifa.ifa_addr as *const libc::sockaddr_in);
                            let addr = Ipv4Addr::from(u32::from_be(sin.sin_addr.s_addr));
                            let mask = if ifa.ifa_netmask.is_null() {
                                Ipv4Addr::UNSPECIFIED
                            } else {
                                let m = &*(ifa.ifa_netmask as *const libc::sockaddr_in);
                                Ipv4Addr::from(u32::from_be(m.sin_addr.s_addr))
                            };
                            entry.ipv4.push((addr, mask));
                        }
                        libc::AF_INET6 => {
                            let sin6 = &*(ifa.ifa_addr as *const libc::sockaddr_in6);
                            entry.ipv6.push(Ipv6Addr::from(sin6.sin6_addr.s6_addr));
                        }
                        libc::AF_LINK => {
                            let sdl = &*(ifa.ifa_addr as *const libc::sockaddr_dl);
                            if sdl.sdl_alen == 6 {
                                let base = sdl.sdl_data.as_ptr().add(sdl.sdl_nlen as usize) as *const u8;
                                let b = std::slice::from_raw_parts(base, 6);
                                entry.mac = Some(format!(
                                    "{:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}",
                                    b[0], b[1], b[2], b[3], b[4], b[5]
                                ));
                            }
                        }
                        _ => {}
                    }
                }
                cur = ifa.ifa_next;
            }
            libc::freeifaddrs(head);
        }
    }

    // Link status and media are only exposed via ioctls that ifconfig wraps;
    // parse its output rather than re-implementing SIOCGIFMEDIA.
    if let Ok(out) = cmd::run(IFCONFIG, &["-a"]) {
        let mut cur: Option<String> = None;
        for line in out.lines() {
            if !line.starts_with(['\t', ' ']) {
                cur = line.split_once(':').map(|(n, _)| n.to_string());
                if let (Some(n), Some(idx)) = (&cur, line.find(" mtu ")) {
                    let mtu = line[idx + 5..].split_whitespace().next().and_then(|s| s.parse().ok());
                    map.entry(n.clone()).or_default().mtu = mtu;
                }
                continue;
            }
            let Some(name) = &cur else { continue };
            let l = line.trim();
            if let Some(s) = l.strip_prefix("status: ") {
                map.entry(name.clone()).or_default().status = Some(s.to_string());
            } else if let Some(m) = l.strip_prefix("media: ") {
                map.entry(name.clone()).or_default().media = Some(m.to_string());
            }
        }
    }
    map
}
