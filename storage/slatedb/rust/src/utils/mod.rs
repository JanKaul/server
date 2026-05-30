//! Leaf utility modules.
//!
//! Per `_DESIGN.md §8`: small, dependency-free helpers translated from the
//! corresponding MyRocks headers. None of these modules touch SlateDB APIs;
//! they manipulate raw bytes, integers, and process-local atomic state.

pub mod atomic_stat;
pub mod buff;
pub mod counter;
pub mod dbug;
pub mod mariadb_port;
pub mod names;
pub mod parse;
pub mod psi;
pub mod server_port;
