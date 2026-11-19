//! How many downloads (and hash computations) may run at the same time.
//!
//! Each one holds a [`DownloadSlot`] for as long as it runs. The slot is
//! given back automatically when it is dropped: when the last byte is
//! sent, when the device disconnects, or when an error stops the transfer.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

pub struct DownloadLimiter {
    max_total: usize,
    max_per_device: usize,
    active: Arc<Mutex<ActiveDownloads>>,
}

#[derive(Default)]
struct ActiveDownloads {
    total: usize,
    /// Device ID → how many downloads that device is running.
    per_device: HashMap<String, usize>,
}

/// Permission to run one download. Dropping it frees the slot.
pub struct DownloadSlot {
    active: Arc<Mutex<ActiveDownloads>>,
    device_id: String,
}

impl DownloadLimiter {
    pub fn new(max_total: usize, max_per_device: usize) -> Self {
        Self {
            max_total,
            max_per_device,
            active: Arc::default(),
        }
    }

    /// A slot for `device_id`, or `None` if the device or the whole host
    /// is already at its limit.
    pub fn try_start(&self, device_id: &str) -> Option<DownloadSlot> {
        let mut active = self.active.lock().unwrap();

        let running_for_device = active.per_device.get(device_id).copied().unwrap_or(0);
        if active.total >= self.max_total || running_for_device >= self.max_per_device {
            return None;
        }

        active.total += 1;
        active
            .per_device
            .insert(device_id.to_owned(), running_for_device + 1);

        Some(DownloadSlot {
            active: Arc::clone(&self.active),
            device_id: device_id.to_owned(),
        })
    }
}

impl Drop for DownloadSlot {
    fn drop(&mut self) {
        let mut active = self.active.lock().unwrap();
        active.total -= 1;

        if let Some(running) = active.per_device.get_mut(&self.device_id) {
            *running -= 1;
            if *running == 0 {
                active.per_device.remove(&self.device_id);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn enforces_per_device_limit() {
        let limiter = DownloadLimiter::new(10, 2);

        let first = limiter.try_start("phone").unwrap();
        let _second = limiter.try_start("phone").unwrap();
        assert!(
            limiter.try_start("phone").is_none(),
            "third is over the limit"
        );
        assert!(
            limiter.try_start("laptop").is_some(),
            "other devices unaffected"
        );

        drop(first);
        assert!(limiter.try_start("phone").is_some(), "slot was given back");
    }

    #[test]
    fn enforces_total_limit() {
        let limiter = DownloadLimiter::new(2, 5);

        let _a = limiter.try_start("phone").unwrap();
        let b = limiter.try_start("laptop").unwrap();
        assert!(limiter.try_start("tablet").is_none());

        drop(b);
        assert!(limiter.try_start("tablet").is_some());
    }
}
