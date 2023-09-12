#![cfg_attr(docsrs, feature(doc_cfg))]

#![doc = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/README.md"))]
//! <br><br>
//!
//! ## You're probably looking for:
//! * [`Logger`](Logger)
//! * [`LoggerHandle`](LoggerHandle)

pub mod prelude;
pub mod error;
pub(crate) mod levels;
#[cfg(feature = "singleton")]
pub(crate) mod sync;

pub use prelude::{
    Logger, LoggerHandle, SetupLogger, LoggerError
};