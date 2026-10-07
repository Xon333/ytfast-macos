//! Linux notification probes remain callable by the optional E2E driver.
//! macOS notifications are deliberately not claimed by this first port.
pub fn last_sent() -> Option<(u32, String)> { None }
