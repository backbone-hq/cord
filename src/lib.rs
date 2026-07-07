mod de;
pub mod encode;
mod private;
mod result;
mod ser;
mod types;
pub(crate) mod varint;
mod width;
pub(crate) mod wire;

pub use cord_derive::Cord;
pub use de::deserialize;
pub use encode::{CordDecode, CordEncode};
pub use result::{CordError, CordResult};
pub use ser::serialize;
pub use width::Width;

/// Compute a canonical SHA3-256 hash.
///
/// Requires the `hash` feature.
#[cfg(feature = "hash")]
pub fn hash<T: serde::Serialize>(value: &T) -> CordResult<[u8; 32]> {
    use sha3::{Digest, Sha3_256};
    let bytes = serialize(value)?;
    Ok(Sha3_256::digest(&bytes).into())
}

#[cfg(feature = "datetime")]
pub use types::DateTime;
#[cfg(feature = "decimal")]
pub use types::Decimal;
#[cfg(feature = "uuid")]
pub use types::Uuid;
pub use types::{Bytes, Evolving, Map, Set};

/// Internal helpers for the `cord-derive` proc macro.
///
/// **Not part of the public API.** Do not use directly.
#[doc(hidden)]
pub mod __private {
    pub use crate::private::*;
}

// Allow `::cord` to resolve within this crate, so that `#[derive(Cord)]`
// generated code works in internal tests.
extern crate self as cord;
