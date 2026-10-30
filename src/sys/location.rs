//! CoreLocation authorization. macOS only reveals Wi-Fi SSIDs/BSSIDs to
//! processes the user has authorized for Location Services.
//!
//! Must be used from the main thread; the main loop pumps the CFRunLoop so
//! that CoreLocation can deliver the authorization prompt and result.

use objc2::rc::Retained;
use objc2_core_foundation::{CFRunLoop, kCFRunLoopDefaultMode};
use objc2_core_location::{CLAuthorizationStatus, CLLocationManager};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LocAuth {
    ServicesOff,
    NotDetermined,
    Restricted,
    Denied,
    Authorized,
}

impl LocAuth {
    pub fn label(self) -> &'static str {
        match self {
            LocAuth::ServicesOff => "Location Services off",
            LocAuth::NotDetermined => "Location: not requested",
            LocAuth::Restricted => "Location: restricted",
            LocAuth::Denied => "Location: denied",
            LocAuth::Authorized => "Location: authorized",
        }
    }
}

pub struct Location {
    mgr: Retained<CLLocationManager>,
    requested: bool,
}

impl Location {
    pub fn new() -> Self {
        let mgr = unsafe { CLLocationManager::new() };
        Self { mgr, requested: false }
    }

    pub fn status(&self) -> LocAuth {
        unsafe {
            if !CLLocationManager::locationServicesEnabled_class() {
                return LocAuth::ServicesOff;
            }
            match self.mgr.authorizationStatus() {
                CLAuthorizationStatus::NotDetermined => LocAuth::NotDetermined,
                CLAuthorizationStatus::Restricted => LocAuth::Restricted,
                CLAuthorizationStatus::Denied => LocAuth::Denied,
                _ => LocAuth::Authorized,
            }
        }
    }

    /// Trigger the system authorization prompt (no-op once decided).
    pub fn request(&mut self) {
        unsafe {
            self.mgr.requestWhenInUseAuthorization();
            // On macOS, starting updates is what reliably surfaces the prompt.
            self.mgr.startUpdatingLocation();
        }
        self.requested = true;
    }

    /// Stop location updates once authorization has been decided; we never
    /// need actual coordinates.
    pub fn settle(&mut self) {
        if self.requested && self.status() != LocAuth::NotDetermined {
            unsafe { self.mgr.stopUpdatingLocation() };
            self.requested = false;
        }
    }
}

/// Service the main run loop briefly so CoreLocation callbacks are delivered.
pub fn pump_runloop() {
    let mode = unsafe { kCFRunLoopDefaultMode };
    CFRunLoop::run_in_mode(mode, 0.0, true);
}
