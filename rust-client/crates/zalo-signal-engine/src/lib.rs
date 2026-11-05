//! Local compatibility candidate. No server, filesystem, database or UI access.
//! Publication and distribution remain outside this crate's authorization.

mod types;
pub use types::*;

#[cfg(feature = "embedded")]
mod embedded;
#[cfg(feature = "embedded")]
pub use embedded::{Engine, OperationTicket};
