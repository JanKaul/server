#![forbid(unsafe_op_in_unsafe_fn)]
#![warn(clippy::unwrap_used, clippy::expect_used)]
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]

pub mod alter;
pub mod bridge;
pub mod codec;
pub mod engine;
pub mod error;
pub mod globals;
pub mod handler;
pub mod runtime;
pub mod table_handler;
pub mod task;
pub mod utils;
