mod de;
mod result;
mod ser;
mod types;

pub use de::deserialize;
pub use result::{CordError, CordResult};
pub use ser::serialize;
pub use types::{Bytes, DateTime, Map, Set};

/// Serde helper modules for ergonomic canonical serialization of standard types.
pub mod cord {
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
            T: Serialize + Clone + Ord,
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

    pub mod datetime {
        use super::*;
        pub fn serialize<S>(
            dt: &chrono::DateTime<chrono::Utc>,
            serializer: S,
        ) -> Result<S::Ok, S::Error>
        where
            S: Serializer,
        {
            DateTime::from(*dt).serialize(serializer)
        }

        pub fn deserialize<'de, D>(
            deserializer: D,
        ) -> Result<chrono::DateTime<chrono::Utc>, D::Error>
        where
            D: Deserializer<'de>,
        {
            let dt = DateTime::deserialize(deserializer)?;
            Ok(dt.chrono)
        }
    }
}
