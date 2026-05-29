//! Interface stub for `ha_rocksdb_cc__Rdb_trx_info_aggregator`.
//!
//! C++ source: `storage/rocksdb/ha_rocksdb.cc` (lines 4501..4569, ~69 LoC)
//! v4 manifest sub-unit: `ha_rocksdb_cc__Rdb_trx_info_aggregator`
//!
//! ## Mapping
//! `Rdb_trx_info_aggregator : public Rdb_tx_list_walker` is the visitor that
//! `walk_tx_list` invokes to populate `information_schema.rocksdb_trx`.
//! For each `Rdb_transaction` it extracts:
//!   - `name`, `trx_id` (from the underlying `rocksdb::Transaction`)
//!   - write/lock/timeout counts
//!   - state ("STARTED" / "PREPARED" / etc. from `rocksdb::Transaction::GetState`)
//!   - waiting key + CF (from `rdb_trx->GetWaitingTxns`)
//!   - thread id, query string, is_replication flag
//!
//! Per _DESIGN.md §1 row "information_schema": this is re-implemented from
//! our `RdbTransaction` registry. We can supply most fields directly; the
//! "waiting key / waiting CF" pair is replaced with **our SSI-detected
//! conflict info** (`slatedb::ErrorKind::Transaction` caught at commit time)
//! since SlateDB has no pessimistic-lock wait graph.
//!
//! For `Rdb_writebatch_impl` txns the C++ code reports `trx_id = 0` and
//! `is_replication = 1`; we preserve that.
//!
//! Per `rdb_global_h::TrxInfo` for the row type.
//!
//! ## Out-of-scope methods
//! - State enum mapping for `AWAITING_PREPARE` etc. — only `STARTED`,
//!   `PREPARED`, `COMMITTED`, `ROLLED_BACK` map to our model; other RocksDB
//!   intermediate states (`AWAITING_*`) collapse to `STARTED`.

use slatedb::Error;

use crate::ha_rocksdb_cc__Rdb_transaction::{RdbTransaction, TxListWalker};
use crate::rdb_global_h::TrxInfo;

pub struct RdbTrxInfoAggregator<'a> {
    pub out: &'a mut Vec<TrxInfo>,
}

impl<'a> RdbTrxInfoAggregator<'a> {
    pub fn new(out: &'a mut Vec<TrxInfo>) -> Self { Self { out } }

    /// Map an `RdbTransactionImpl`-style txn to a row. Reads SSI conflict
    /// snapshot (if any) instead of MyRocks' pessimistic waiting-key.
    fn process_full_txn(&mut self, _tx: &dyn RdbTransaction) -> Result<TrxInfo, Error> {
        todo!("build TrxInfo from tx: name=Uuid.to_string(), trx_id=Uuid.as_u128() truncated, state from inner.state(), waiting_* from our SSI conflict cache")
    }

    /// Map an `RdbWritebatchImpl` to a row. Always `is_replication=1`,
    /// `trx_id=0`, `skip_trx_api=1`.
    /// Original: ha_rocksdb.cc:4525.
    fn process_writebatch_txn(&mut self, _tx: &dyn RdbTransaction) -> Result<TrxInfo, Error> {
        todo!("fixed-shape TrxInfo with replication flag, empty waiting fields")
    }
}

impl<'a> TxListWalker for RdbTrxInfoAggregator<'a> {
    /// Dispatch by `is_writebatch_trx()`.
    /// Original: ha_rocksdb.cc:4509.
    fn process_tran(&mut self, tx: &dyn RdbTransaction) {
        let row = if tx.is_writebatch_trx() {
            self.process_writebatch_txn(tx)
        } else {
            self.process_full_txn(tx)
        };
        match row {
            Ok(r) => self.out.push(r),
            Err(_e) => { /* TODO(human): log; skip this txn */ }
        }
    }
}

/// Convenience: walk all live transactions and return a `Vec<TrxInfo>`.
/// This is the public entry called by I_S population — replaces the C++
/// `rdb_get_all_trx_info` free function (ha_rocksdb.cc:4575).
pub fn aggregate_all_trx_info() -> Vec<TrxInfo> {
    let mut out = Vec::new();
    let mut agg = RdbTrxInfoAggregator::new(&mut out);
    crate::ha_rocksdb_cc__Rdb_transaction::walk_tx_list(&mut agg);
    out
}
