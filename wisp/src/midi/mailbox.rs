//! Fixed MIDI CC slots: no locks, allocations or runtime queue waits in the callback.
use std::sync::atomic::{AtomicU64, Ordering};
#[derive(Clone, Debug)]
pub struct MidiObservation {
    pub target: u64,
    pub channel: u8,
    pub controller: u8,
    pub value: f32,
}
pub struct MidiMailbox {
    learning: AtomicU64,
    mappings: Box<[AtomicU64]>,
    values: Box<[AtomicU64]>,
    allowed: Box<[AtomicU64]>,
}
impl Default for MidiMailbox {
    fn default() -> Self {
        Self {
            learning: AtomicU64::new(0),
            mappings: (0..2048).map(|_| AtomicU64::new(0)).collect(),
            values: (0..2048).map(|_| AtomicU64::new(0)).collect(),
            allowed: (0..128).map(|_| AtomicU64::new(0)).collect(),
        }
    }
}
impl MidiMailbox {
    /// Nonzero 56-bit session token, at most 128 targets. Control-thread API.
    pub fn learn(&self, target: u64) -> bool {
        if target == 0 || target >= 1 << 56 {
            return false;
        }
        self.cancel();
        if !self
            .allowed
            .iter()
            .any(|v| v.load(Ordering::SeqCst) == target)
            && !self.allowed.iter().any(|v| {
                v.compare_exchange(0, target, Ordering::SeqCst, Ordering::SeqCst)
                    .is_ok()
            })
        {
            return false;
        }
        self.learning.store(target, Ordering::SeqCst);
        true
    }
    pub fn targets(&self) -> Vec<u64> {
        self.allowed
            .iter()
            .map(|v| v.load(Ordering::SeqCst))
            .filter(|v| *v != 0)
            .collect()
    }
    pub fn cancel(&self) {
        let target = self.learning.swap(0, Ordering::SeqCst);
        if target != 0
            && !self
                .mappings
                .iter()
                .any(|v| v.load(Ordering::SeqCst) == target)
        {
            self.remove(target);
        }
    }
    pub fn remove(&self, target: u64) {
        for allowed in &self.allowed {
            let _ = allowed.compare_exchange(target, 0, Ordering::SeqCst, Ordering::SeqCst);
        }
        let _ = self
            .learning
            .compare_exchange(target, 0, Ordering::SeqCst, Ordering::SeqCst);
        for mapping in &self.mappings {
            let _ = mapping.compare_exchange(target, 0, Ordering::SeqCst, Ordering::SeqCst);
        }
    }
    pub fn cc(&self, channel: u8, controller: u8, value: u8) {
        if channel >= 16 || controller >= 128 || value >= 128 {
            return;
        }
        let slot = channel as usize * 128 + controller as usize;
        let target = self.learning.swap(0, Ordering::SeqCst);
        if target != 0 {
            self.mappings[slot].store(target, Ordering::SeqCst);
        }
        let target = self.mappings[slot].load(Ordering::SeqCst);
        // Pair the value with the token atomically: remapping a CC cannot
        // misattribute a previously queued value to its new target.
        self.values[slot].store((target << 8) | (value as u64 + 1), Ordering::Release);
    }
    pub fn poll(&self) -> Vec<MidiObservation> {
        let mut result = Vec::new();
        for slot in 0..2048 {
            let packed = self.values[slot].swap(0, Ordering::AcqRel);
            let target = packed >> 8;
            let value = packed & 255;
            if value != 0
                && target != 0
                && self
                    .allowed
                    .iter()
                    .any(|v| v.load(Ordering::SeqCst) == target)
            {
                result.push(MidiObservation {
                    target,
                    channel: (slot / 128) as u8,
                    controller: (slot % 128) as u8,
                    value: (value - 1) as f32 / 127.,
                });
            }
        }
        result
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn learn_cancel_and_latest_without_traffic() {
        let m = MidiMailbox::default();
        m.learn(1);
        m.cancel();
        m.cc(0, 7, 127);
        assert!(m.poll().is_empty());
        m.learn(2);
        m.cc(1, 7, 0);
        m.cc(1, 7, 64);
        m.cc(1, 7, 127);
        let v = m.poll();
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].target, 2);
        assert_eq!(v[0].value, 1.);
        m.remove(2);
        m.cc(1, 7, 1);
        assert!(m.poll().is_empty());
    }
}
