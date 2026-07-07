//! Internal helpers used by the `cord-derive` proc macro.
//!
//! **This module is not part of the public API.** It is `#[doc(hidden)]` and
//! may change at any time. Do not depend on it directly.

use crate::de::CordDeserializer;
use crate::{CordError, Evolving, Width};
use serde::de;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::fmt::Formatter;
use std::marker::PhantomData;
// ---------------------------------------------------------------------------
// Sentinel constants — single source of truth for all encoding hint names
// ---------------------------------------------------------------------------

pub(crate) const SENTINEL_VARINT: &str = "__cord_varint";
pub(crate) const SENTINEL_WIDTH8: &str = "__cord_width8";
pub(crate) const SENTINEL_WIDTH16: &str = "__cord_width16";
pub(crate) const SENTINEL_WIDTH64: &str = "__cord_width64";
pub(crate) const SENTINEL_SET: &str = "__cord_set";
#[cfg(feature = "datetime")]
pub(crate) const SENTINEL_DATETIME: &str = "__cord_datetime";
#[cfg(feature = "decimal")]
pub(crate) const SENTINEL_DECIMAL: &str = "__cord_decimal";
#[cfg(feature = "uuid")]
pub(crate) const SENTINEL_UUID: &str = "__cord_uuid";
pub(crate) const SENTINEL_EVOLVING8: &str = "__cord_evolving8";
pub(crate) const SENTINEL_EVOLVING16: &str = "__cord_evolving16";
pub(crate) const SENTINEL_EVOLVING32: &str = "__cord_evolving32";
pub(crate) const SENTINEL_EVOLVING8_RAW: &str = "__cord_evolving8_raw";
pub(crate) const SENTINEL_EVOLVING16_RAW: &str = "__cord_evolving16_raw";
pub(crate) const SENTINEL_EVOLVING32_RAW: &str = "__cord_evolving32_raw";

// ---------------------------------------------------------------------------
// EncodingHint
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum EncodingHint {
    Default,
    VarInt,
    Width8,
    Width16,
    Width64,
}

impl EncodingHint {
    pub(crate) fn width(self) -> Width {
        match self {
            EncodingHint::Width8 => Width::W8,
            EncodingHint::Width16 => Width::W16,
            EncodingHint::Width64 => Width::W64,
            _ => Width::W32,
        }
    }

    pub(crate) fn is_active(self) -> bool {
        self != EncodingHint::Default
    }
}

pub(crate) fn sentinel_to_hint(name: &str) -> Option<EncodingHint> {
    match name {
        SENTINEL_VARINT => Some(EncodingHint::VarInt),
        SENTINEL_WIDTH8 => Some(EncodingHint::Width8),
        SENTINEL_WIDTH16 => Some(EncodingHint::Width16),
        SENTINEL_WIDTH64 => Some(EncodingHint::Width64),
        _ => None,
    }
}

pub(crate) fn evolving_width(name: &str) -> Option<Width> {
    match name {
        SENTINEL_EVOLVING8 | SENTINEL_EVOLVING8_RAW => Some(Width::W8),
        SENTINEL_EVOLVING16 | SENTINEL_EVOLVING16_RAW => Some(Width::W16),
        SENTINEL_EVOLVING32 | SENTINEL_EVOLVING32_RAW => Some(Width::W32),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// PreSerialized — writes raw bytes verbatim through serde
// ---------------------------------------------------------------------------

pub(crate) struct PreSerialized<'a>(pub(crate) &'a [u8]);

impl Serialize for PreSerialized<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeTuple;
        let mut tup = serializer.serialize_tuple(self.0.len())?;
        for byte in self.0 {
            tup.serialize_element(byte)?;
        }
        tup.end()
    }
}

// ---------------------------------------------------------------------------
// Serialization wrappers — hold a reference, serialize through sentinel
// ---------------------------------------------------------------------------

pub trait SentinelHint {
    const NAME: &'static str;
}

pub struct HintWidth8;
pub struct HintWidth16;
pub struct HintWidth64;
pub struct HintVarInt;

impl SentinelHint for HintWidth8 {
    const NAME: &'static str = SENTINEL_WIDTH8;
}
impl SentinelHint for HintWidth16 {
    const NAME: &'static str = SENTINEL_WIDTH16;
}
impl SentinelHint for HintWidth64 {
    const NAME: &'static str = SENTINEL_WIDTH64;
}
impl SentinelHint for HintVarInt {
    const NAME: &'static str = SENTINEL_VARINT;
}

pub struct SentinelSer<'a, H, T: ?Sized>(pub &'a T, PhantomData<H>);

impl<'a, H, T: ?Sized> SentinelSer<'a, H, T> {
    pub fn new(value: &'a T) -> Self {
        Self(value, PhantomData)
    }
}

impl<H: SentinelHint, T: ?Sized + Serialize> Serialize for SentinelSer<'_, H, T> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_newtype_struct(H::NAME, self.0)
    }
}

pub type Width8Ser<'a, T> = SentinelSer<'a, HintWidth8, T>;
pub type Width16Ser<'a, T> = SentinelSer<'a, HintWidth16, T>;
pub type Width64Ser<'a, T> = SentinelSer<'a, HintWidth64, T>;
pub type VarIntSer<'a, T> = SentinelSer<'a, HintVarInt, T>;

// ---------------------------------------------------------------------------
// Deserialization wrappers — owned, deserialize through sentinel
// ---------------------------------------------------------------------------

pub struct SentinelDe<H, T>(pub T, PhantomData<H>);

struct SentinelDeVisitor<H, T>(PhantomData<(H, T)>);

impl<'de, H: SentinelHint, T: Deserialize<'de>> de::Visitor<'de> for SentinelDeVisitor<H, T> {
    type Value = SentinelDe<H, T>;

    fn expecting(&self, formatter: &mut Formatter) -> std::fmt::Result {
        formatter.write_str(H::NAME)
    }

    fn visit_newtype_struct<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
    where
        D: de::Deserializer<'de>,
    {
        T::deserialize(deserializer).map(|v| SentinelDe(v, PhantomData))
    }
}

impl<'de, H: SentinelHint, T: Deserialize<'de>> Deserialize<'de> for SentinelDe<H, T> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_newtype_struct(H::NAME, SentinelDeVisitor(PhantomData))
    }
}

pub type Width8De<T> = SentinelDe<HintWidth8, T>;
pub type Width16De<T> = SentinelDe<HintWidth16, T>;
pub type Width64De<T> = SentinelDe<HintWidth64, T>;
pub type VarIntDe<T> = SentinelDe<HintVarInt, T>;

// ---------------------------------------------------------------------------
// Evolving serialization wrappers
// ---------------------------------------------------------------------------

macro_rules! evolving_ser_wrapper {
    ($name:ident, $sentinel:expr, $sentinel_raw:expr) => {
        pub struct $name<'a, T>(pub &'a Evolving<T>);

        impl<'a, T> $name<'a, T> {
            pub fn new(value: &'a Evolving<T>) -> Self {
                Self(value)
            }
        }

        impl<'a, T: Serialize> Serialize for $name<'a, T> {
            fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
                match self.0 {
                    Evolving::Known(ref t) => serializer.serialize_newtype_struct($sentinel, t),
                    Evolving::Unknown(ref bytes) => {
                        serializer.serialize_newtype_struct($sentinel_raw, &PreSerialized(bytes))
                    }
                }
            }
        }
    };
}

evolving_ser_wrapper!(Evolving8Ser, SENTINEL_EVOLVING8, SENTINEL_EVOLVING8_RAW);
evolving_ser_wrapper!(Evolving16Ser, SENTINEL_EVOLVING16, SENTINEL_EVOLVING16_RAW);
evolving_ser_wrapper!(Evolving32Ser, SENTINEL_EVOLVING32, SENTINEL_EVOLVING32_RAW);

// ---------------------------------------------------------------------------
// Evolving deserialization wrappers
// ---------------------------------------------------------------------------

macro_rules! evolving_de_wrapper {
    ($name:ident, $sentinel:expr, $visitor:ident) => {
        /// Deserialization wrapper that produces `Evolving<T>`.
        pub struct $name<T>(pub Evolving<T>);

        struct $visitor<T> {
            marker: PhantomData<T>,
        }

        impl<'de, T: Deserialize<'de>> de::Visitor<'de> for $visitor<T> {
            type Value = $name<T>;

            fn expecting(&self, formatter: &mut Formatter) -> std::fmt::Result {
                formatter.write_str("evolving enum")
            }

            fn visit_borrowed_bytes<E: de::Error>(
                self,
                payload: &'de [u8],
            ) -> Result<Self::Value, E> {
                let mut sub_de = CordDeserializer::new(payload);
                match T::deserialize(&mut sub_de) {
                    Ok(value) => {
                        if sub_de.input.is_empty() {
                            Ok($name(Evolving::Known(value)))
                        } else {
                            Err(E::custom("trailing bytes in evolving payload"))
                        }
                    }
                    Err(CordError::UnknownVariant(_)) => {
                        Ok($name(Evolving::Unknown(payload.to_vec())))
                    }
                    Err(e) => Err(E::custom(e)),
                }
            }

            fn visit_byte_buf<E: de::Error>(self, payload: Vec<u8>) -> Result<Self::Value, E> {
                Ok($name(Evolving::Unknown(payload)))
            }

            fn visit_newtype_struct<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
            where
                D: de::Deserializer<'de>,
            {
                T::deserialize(deserializer).map(|v| $name(Evolving::Known(v)))
            }
        }

        impl<'de, T: Deserialize<'de>> Deserialize<'de> for $name<T> {
            fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
                deserializer.deserialize_newtype_struct(
                    $sentinel,
                    $visitor {
                        marker: PhantomData,
                    },
                )
            }
        }
    };
}

evolving_de_wrapper!(Evolving8De, SENTINEL_EVOLVING8, Evolving8DeVisitor);
evolving_de_wrapper!(Evolving16De, SENTINEL_EVOLVING16, Evolving16DeVisitor);
evolving_de_wrapper!(Evolving32De, SENTINEL_EVOLVING32, Evolving32DeVisitor);

// ---------------------------------------------------------------------------
// CordEncode/CordDecode helpers for derive macro
