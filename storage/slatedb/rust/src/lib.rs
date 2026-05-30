#![forbid(unsafe_op_in_unsafe_fn)]
#![warn(clippy::unwrap_used, clippy::expect_used)]
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]

pub mod bridge;
pub mod error;
pub mod globals;
pub mod task;
pub mod utils;
