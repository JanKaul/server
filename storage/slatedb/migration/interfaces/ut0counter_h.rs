//! Interface stub for `ut0counter_h`.
//!
//! C++ source: `storage/rocksdb/ut0counter.h` (203 LoC)
//! C++ class: `ib_counter_t<Type, N, Indexer>` (InnoDB-origin utility)
//!
//! ## Mapping
//! Sharded "fuzzy" counter where each shard sits on its own cache line so that
//! concurrent increments don't false-share. The C++ template parametrizes:
//! - the numeric type (we only use `u64` in practice — `ulonglong`),
//! - the slot count `N` (default 64), and
//! - the indexer policy (thread-id by default, sched_getcpu on Linux).
//!
//! In Rust this collapses to a `Box<[AtomicU64; N]>` with cache-line padding
//! around each element. Indexing is by thread id (we don't have stable CPU id
//! without nightly intrinsics), and reads sum across all shards — accepted as
//! approximate per the C++ docstring ("not 100% accurate, since it is not
//! atomic").
//!
//! Per _DESIGN.md §1 the perf-counter family is "Re-impl" — this is one of
//! the building blocks.
//!
//! ## Out-of-scope methods
//! None — it's a self-contained primitive.

use std::sync::atomic::{AtomicU64, Ordering};

/// Number of shards per counter. Matches C++ `IB_N_SLOTS = 64`. Chosen so the
/// counter array fits in a single 4 KiB page.
pub const IB_N_SLOTS: usize = 64;

/// CPU cache line size used to pad shards apart.
pub const UT_CACHE_LINE_SIZE: usize = 64;

/// One cache-line-padded `AtomicU64`. We rely on `#[repr(align(64))]` rather
/// than a manual `[u64; 8]` array — the C++ achieves the same effect via the
/// `offset()` stride trick on a flat array.
#[repr(align(64))]
#[derive(Default)]
struct Slot(AtomicU64);

/// Sharded relaxed-consistency counter — drop-in replacement for
/// `ib_counter_t<ulonglong, IB_N_SLOTS, thread_id_indexer_t>`.
///
/// Increment cost: one relaxed fetch_add on the shard picked by the calling
/// thread's id. Read cost: sum across all `IB_N_SLOTS` shards (read is racy by
/// design).
pub struct IbCounter {
    shards: Box<[Slot; IB_N_SLOTS]>,
}

impl IbCounter {
    pub fn new() -> Self {
        // Construct on the heap to avoid a 4 KiB stack allocation. Uses
        // `std::array::from_fn` (no fallible conversion, hence no `expect`
        // required by §0.4).
        let shards: Box<[Slot; IB_N_SLOTS]> =
            Box::new(std::array::from_fn(|_| Slot::default()));
        Self { shards }
    }

    /// Pick a shard by hashing the current thread id. Matches the C++
    /// `thread_id_indexer_t::get_rnd_index()` strategy.
    #[inline]
    fn shard_index() -> usize {
        // `ThreadId` doesn't expose a numeric form on stable; we hash it.
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};
        let mut h = DefaultHasher::new();
        std::thread::current().id().hash(&mut h);
        (h.finish() as usize) % IB_N_SLOTS
    }

    /// Increment by `n` using the auto-picked shard. C++ `add(Type n)`.
    pub fn add(&self, n: u64) {
        self.shards[Self::shard_index()].0.fetch_add(n, Ordering::Relaxed);
    }

    /// Increment by `n` on the explicit shard. C++ `add(size_t index, Type n)`.
    /// `index` is taken modulo `IB_N_SLOTS`.
    pub fn add_at(&self, index: usize, n: u64) {
        self.shards[index % IB_N_SLOTS].0.fetch_add(n, Ordering::Relaxed);
    }

    pub fn inc(&self) { self.add(1); }

    pub fn sub(&self, n: u64) {
        self.shards[Self::shard_index()].0.fetch_sub(n, Ordering::Relaxed);
    }

    pub fn sub_at(&self, index: usize, n: u64) {
        self.shards[index % IB_N_SLOTS].0.fetch_sub(n, Ordering::Relaxed);
    }

    pub fn dec(&self) { self.sub(1); }

    /// Sum across shards. Approximate by design — concurrent writers may
    /// commit during the walk. C++ `operator Type()`.
    pub fn get(&self) -> u64 {
        let mut total: u64 = 0;
        for s in self.shards.iter() {
            // Wrapping_add: matches the C++ `+=` on `ulonglong`.
            total = total.wrapping_add(s.0.load(Ordering::Relaxed));
        }
        total
    }
}

impl Default for IbCounter {
    fn default() -> Self { Self::new() }
}
