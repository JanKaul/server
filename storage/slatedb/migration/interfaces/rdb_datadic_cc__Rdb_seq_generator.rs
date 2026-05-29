//! Interface stub for `Rdb_seq_generator`.
//!
//! C++ source: `storage/rocksdb/rdb_datadic.cc` (lines 5418..5437)
//! C++ header: `storage/rocksdb/rdb_datadic.h` (Rdb_seq_generator declarations)
//! v4 manifest sub-unit: `Rdb_seq_generator`
//! Parent unit: `rdb_datadic.cc`
//! Approx body LoC accounted for here: ~20 (small unit; tight contract)
//!
//! ## Mapping
//! Per _DESIGN.md §1 (system metadata via DictManager): monotonic ID allocator
//! for `cf_id` / `index_id` / `table_id`. The MyRocks implementation:
//!
//! ```cpp
//! uint res = m_next_number++;
//! dict->update_max_index_id(batch, res);
//! dict->commit(batch);
//! ```
//!
//! Translates to a Rust `AtomicU32` + a SlateDB-persisted "max_index_id" key.
//! The atomic counter is the fast path; the persisted key is the durable
//! seed used at startup recovery (`DictManager::get_max_index_id`).
//!
//! Concurrency: in MyRocks, `m_mutex` serializes the
//! "bump + persist + commit" critical section. In Rust we use a
//! `tokio::sync::Mutex` because the persist step is `async`. Multiple
//! callers contend on the mutex but each fetched ID is the result of one
//! committed batch — no gaps unless the server crashes mid-commit, in
//! which case startup recovery skips ahead based on the persisted value.
//!
//! ## Out-of-scope methods
//! None — this is a one-method class.

use slatedb::Error;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;
use tokio::sync::Mutex;

use crate::Rdb_dict_manager::DictManager;

/// `Rdb_seq_generator` — monotonic-ID allocator.
pub struct SeqGenerator {
    /// Fast-path counter. Initialized at startup from
    /// `DictManager::get_max_index_id` + 1.
    pub next_number: AtomicU32,
    /// Coarse-grained lock around the persist-and-bump critical section.
    pub mutex: Mutex<()>,
    pub dict: Arc<DictManager>,
}

impl SeqGenerator {
    /// Construct from the recovered "max_index_id" value. Pass `None` for
    /// a fresh-DB scenario; we'll start from
    /// `crate::rdb_global_h::DDL_USER_ID_START` (= 1 by convention; the
    /// concrete value lives in `rdb_datadic.h` `DDL_USER_ID_START`).
    pub fn new(_dict: Arc<DictManager>, _recovered_max: Option<u32>) -> Self {
        todo!("init next_number = recovered.unwrap_or(DDL_USER_ID_START) + 1")
    }

    /// Allocate the next ID. Bumps the counter, persists the new max into
    /// the SYSTEM region (atomic batch commit), and returns the freshly
    /// allocated value.
    ///
    /// Errors: `slatedb::Error::Transaction` (commit conflict — caller
    /// must retry) or `Unavailable` (I/O).
    ///
    /// C++: rdb_datadic.cc:5418.
    pub async fn get_and_update_next_number(&self) -> Result<u32, Error> {
        // Sketch of the translated flow:
        //   let _guard = self.mutex.lock().await;
        //   let res = self.next_number.fetch_add(1, Ordering::SeqCst);
        //   let mut batch = self.dict.begin();
        //   self.dict.update_max_index_id(&mut batch, res);
        //   self.dict.commit(batch, /*sync=*/ true).await?;
        //   Ok(res)
        let _ = Ordering::SeqCst;
        todo!("port C++ get_and_update_next_number at rdb_datadic.cc:5418")
    }
}
