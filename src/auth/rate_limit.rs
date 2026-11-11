//! A simple per-IP rate limit for pairing and login (planning.md §11).
//!
//! Each IP address gets `max_requests` per one-minute window. This is not
//! meant to stop a determined attacker on its own; pairing is already
//! limited to one guess per code. It stops casual hammering.

use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::Mutex;
use std::time::{Duration, Instant};

const WINDOW: Duration = Duration::from_secs(60);

/// Forget idle IPs once we're tracking this many, so memory stays bounded.
const MAX_TRACKED_IPS: usize = 10_000;

pub struct RateLimiter {
    max_requests: u32,
    windows: Mutex<HashMap<IpAddr, Window>>,
}

struct Window {
    started: Instant,
    count: u32,
}

impl RateLimiter {
    pub fn new(max_requests_per_minute: u32) -> Self {
        Self {
            max_requests: max_requests_per_minute,
            windows: Mutex::default(),
        }
    }

    /// Record a request from `ip`. Returns `false` if it's over the limit.
    pub fn allow(&self, ip: IpAddr) -> bool {
        let mut windows = self
            .windows
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let now = Instant::now();

        if windows.len() >= MAX_TRACKED_IPS {
            windows.retain(|_, window| now.duration_since(window.started) < WINDOW);
        }

        let window = windows.entry(ip).or_insert(Window {
            started: now,
            count: 0,
        });
        if now.duration_since(window.started) >= WINDOW {
            window.started = now;
            window.count = 0;
        }

        window.count += 1;
        window.count <= self.max_requests
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blocks_after_the_limit_per_ip() {
        let limiter = RateLimiter::new(3);
        let first_ip: IpAddr = "10.0.0.1".parse().unwrap();
        let second_ip: IpAddr = "10.0.0.2".parse().unwrap();

        assert!(limiter.allow(first_ip));
        assert!(limiter.allow(first_ip));
        assert!(limiter.allow(first_ip));
        assert!(!limiter.allow(first_ip));
        assert!(limiter.allow(second_ip));
    }
}
