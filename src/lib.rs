#![doc = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/README.md"))]
//! <br><br>
//!
//! ## You're probably looking for:
//! * [`Logger`](prelude::Logger)
//! * [`LoggerHandle`](prelude::LoggerHandle)

pub mod prelude;
pub mod error;
pub(crate) mod levels;
#[cfg(feature = "singleton")]
pub(crate) mod sync;