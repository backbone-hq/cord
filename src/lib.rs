mod de;
pub mod dynamic;
pub mod encode;
mod private;
mod result;
pub mod schema;
mod ser;
mod types;
mod value;
pub(crate) mod varint;
pub(crate) mod wire;

pub use cord_derive::Cord;
pub use de::{
    deserialize, deserialize_prefix, DeserializeOptions, DEFAULT_MAX_DEPTH, DEFAULT_MAX_LENGTH,
};
pub use encode::{CordDecode, CordEncode};
pub use result::{CordError, CordResult};
pub use schema::{CordSchema, IntoValue, Schema, Value, VariantSchema, VariantValue, Width};
pub use ser::serialize;

/// Compute a canonical SHA3-256 hash of any serializable value.
///
/// Since Cord guarantees deterministic serialization, the same value always
/// produces the same hash regardless of when or where it is computed.
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
pub use value::{from_value, to_value};

/// Internal helpers for the `cord-derive` proc macro.
///
/// **Not part of the public API.** Do not use directly.
#[doc(hidden)]
pub mod __private {
    pub use crate::private::*;
}

/// Construct a [`Value`] using JSON-like syntax.
///
/// - Objects `{ "key": value, ... }` become `Value::Struct`
/// - Arrays `[a, b, c]` become `Value::Seq`
/// - Scalars are converted via [`IntoValue`] — Rust's type inference selects
///   the right variant (unsuffixed integers default to `i32`, use `30_u32` etc.
///   for explicit types)
/// - Parenthesized expressions `(expr)` allow embedding variables and function calls
///
/// # Examples
///
/// ```
/// use cord::cord_value;
///
/// let value = cord_value!({
///     "name": "Alice",
///     "age": 30_u32,
///     "tags": ["admin", "user"],
///     "active": true,
/// });
/// ```
#[macro_export]
macro_rules! cord_value {
    // Object: { "key": value, ... }
    ({}) => {
        $crate::Value::Struct(::std::vec![])
    };
    ({ $( $key:literal : $val:tt ),+ $(,)? }) => {
        $crate::Value::Struct(::std::vec![
            $( ( ::std::string::String::from($key), $crate::cord_value!($val) ) ),+
        ])
    };

    // Array: [a, b, c]
    ([]) => {
        $crate::Value::Seq(::std::vec![])
    };
    ([ $( $elem:tt ),+ $(,)? ]) => {
        $crate::Value::Seq(::std::vec![
            $( $crate::cord_value!($elem) ),+
        ])
    };

    // Parenthesized expression: (expr) — for variables, function calls, etc.
    (( $e:expr )) => {
        $crate::IntoValue::into_value($e)
    };

    // Bare literal or expression
    ($e:expr) => {
        $crate::IntoValue::into_value($e)
    };
}

// Allow `::cord` to resolve within this crate, so that `#[derive(Cord)]`
// generated code works in internal tests.
extern crate self as cord;

/// Serde helper modules for ergonomic canonical serialization of standard types.
pub mod encoding {
    use super::*;
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    pub mod bytes {
        use super::*;
        pub fn serialize<S>(v: &[u8], serializer: S) -> Result<S::Ok, S::Error>
        where
            S: Serializer,
        {
            serializer.serialize_bytes(v)
        }

        pub fn deserialize<'de, D>(deserializer: D) -> Result<Vec<u8>, D::Error>
        where
            D: Deserializer<'de>,
        {
            let bytes = Bytes::deserialize(deserializer)?;
            Ok(bytes.into())
        }
    }

    pub mod set {
        use super::*;
        use std::collections::HashSet;
        pub fn serialize<T, S>(set: &HashSet<T>, serializer: S) -> Result<S::Ok, S::Error>
        where
            T: Serialize + Clone + std::hash::Hash + Eq,
            S: Serializer,
        {
            Set::from(set.clone()).serialize(serializer)
        }

        pub fn deserialize<'de, T, D>(deserializer: D) -> Result<HashSet<T>, D::Error>
        where
            T: Deserialize<'de> + std::hash::Hash + Eq + Serialize,
            D: Deserializer<'de>,
        {
            let set = Set::<T>::deserialize(deserializer)?;
            Ok(set.into())
        }
    }

    macro_rules! encoding_module {
        ($mod_name:ident, $wrapper:ty, $inner:ty, $to:expr, $from:expr) => {
            pub mod $mod_name {
                use super::*;
                pub fn serialize<S: Serializer>(v: &$inner, s: S) -> Result<S::Ok, S::Error> {
                    let w: $wrapper = $to(v);
                    w.serialize(s)
                }
                pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<$inner, D::Error> {
                    let w = <$wrapper>::deserialize(d)?;
                    Ok($from(w))
                }
            }
        };
    }

    #[cfg(feature = "decimal")]
    encoding_module!(
        decimal,
        Decimal,
        Decimal,
        |v: &Decimal| v.clone(),
        |v: Decimal| v
    );

    #[cfg(feature = "uuid")]
    encoding_module!(
        uuid,
        Uuid,
        ::uuid::Uuid,
        |v: &::uuid::Uuid| Uuid::from(*v),
        |w: Uuid| w.into_inner()
    );

    #[cfg(feature = "datetime")]
    encoding_module!(
        datetime,
        DateTime,
        chrono::DateTime<chrono::Utc>,
        |dt: &chrono::DateTime<chrono::Utc>| DateTime::from(*dt),
        |dt: DateTime| dt.chrono
    );
}
