//! Sharded "fuzzy" counter.
//!
//! Translated from `storage/rocksdb/ut0counter.h` (InnoDB-origin utility).
//! Each shard sits on its own cache line so concurrent increments don't false
//! share. Reads sum across shards and are approximate by design.

use std::sync::atomic::{AtomicU64, Ordering};

/// Number of shards. Matches C++ `IB_N_SLOTS = 64`.
pub const IB_N_SLOTS: usize = 64;

/// CPU cache-line size used to pad shards apart.
pub const UT_CACHE_LINE_SIZE: usize = 64;

#[repr(align(64))]
#[derive(Default)]
struct Slot(AtomicU64);

/// Sharded relaxed-consistency counter.
///
/// Increment cost: one relaxed `fetch_add` on the shard picked by the caller's
/// thread id. Read cost: sum across all `IB_N_SLOTS` shards (racy by design).
pub struct IbCounter {
    shards: Box<[Slot; IB_N_SLOTS]>,
}

impl IbCounter {
    pub fn new() -> Self {
        let shards: Box<[Slot; IB_N_SLOTS]> =
            Box::new(std::array::from_fn(|_| Slot::default()));
        Self { shards }
    }

    #[inline]
    fn shard_index() -> usize {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};
        let mut h = DefaultHasher::new();
        std::thread::current().id().hash(&mut h);
        (h.finish() as usize) % IB_N_SLOTS
    }

    pub fn add(&self, n: u64) {
        self.shards[Self::shard_index()]
            .0
            .fetch_add(n, Ordering::Relaxed);
    }

    pub fn add_at(&self, index: usize, n: u64) {
        self.shards[index % IB_N_SLOTS]
            .0
            .fetch_add(n, Ordering::Relaxed);
    }

    pub fn inc(&self) {
        self.add(1);
    }

    pub fn sub(&self, n: u64) {
        self.shards[Self::shard_index()]
            .0
            .fetch_sub(n, Ordering::Relaxed);
    }

    pub fn sub_at(&self, index: usize, n: u64) {
        self.shards[index % IB_N_SLOTS]
            .0
            .fetch_sub(n, Ordering::Relaxed);
    }

    pub fn dec(&self) {
        self.sub(1);
    }

    /// Sum across shards. Approximate by design.
    pub fn get(&self) -> u64 {
        let mut total: u64 = 0;
        for s in self.shards.iter() {
            total = total.wrapping_add(s.0.load(Ordering::Relaxed));
        }
        total
    }
}

impl Default for IbCounter {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn single_thread_round_trip() {
        let c = IbCounter::new();
        for _ in 0..1000 {
            c.inc();
        }
        assert_eq!(c.get(), 1000);
        c.add_at(0, 7);
        assert_eq!(c.get(), 1007);
        c.dec();
        assert_eq!(c.get(), 1006);
    }

    #[test]
    fn multi_thread_aggregates() {
        use std::sync::Arc;
        let c = Arc::new(IbCounter::new());
        let mut threads = Vec::new();
        for _ in 0..8 {
            let c = Arc::clone(&c);
            threads.push(std::thread::spawn(move || {
                for _ in 0..1000 {
                    c.inc();
                }
            }));
        }
        for t in threads {
            let _ = t.join();
        }
        assert_eq!(c.get(), 8 * 1000);
    }
}
