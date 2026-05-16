//! Internal helpers used by the `cord-derive` proc macro.
//!
//! **This module is not part of the public API.** It is `#[doc(hidden)]` and
//! may change at any time. Do not depend on it directly.

use crate::de::CordDeserializer;
use crate::result::CordError;
use crate::Evolving;
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
// EncodingHint — replaces ad-hoc varint_mode + width state
// ---------------------------------------------------------------------------

/// Encoding mode set by sentinel newtype wrappers.
/// Replaces the ad-hoc `varint_mode: bool` + `width: Option<u8>` state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum EncodingHint {
    /// Default: no special encoding
    Default,
    /// Use varint encoding for the next integer
    VarInt,
    /// Use 1-byte length prefix
    Width8,
    /// Use 2-byte length prefix
    Width16,
    /// Use 8-byte length prefix
    Width64,
}

impl EncodingHint {
    /// Returns the width for length-prefix hints, or the default (W32).
    pub(crate) fn width(self) -> crate::schema::Width {
        match self {
            EncodingHint::Width8 => crate::schema::Width::W8,
            EncodingHint::Width16 => crate::schema::Width::W16,
            EncodingHint::Width64 => crate::schema::Width::W64,
            _ => crate::schema::Width::W32,
        }
    }

    /// Returns true if this hint is not the default.
    pub(crate) fn is_active(self) -> bool {
        self != EncodingHint::Default
    }
}

/// Map a sentinel name to an encoding hint (for width/varint sentinels only).
pub(crate) fn sentinel_to_hint(name: &str) -> Option<EncodingHint> {
    match name {
        SENTINEL_VARINT => Some(EncodingHint::VarInt),
        SENTINEL_WIDTH8 => Some(EncodingHint::Width8),
        SENTINEL_WIDTH16 => Some(EncodingHint::Width16),
        SENTINEL_WIDTH64 => Some(EncodingHint::Width64),
        _ => None,
    }
}

/// Returns true if the given name is any known cord sentinel.
pub(crate) fn is_sentinel(name: &str) -> bool {
    if matches!(
        name,
        SENTINEL_VARINT
            | SENTINEL_WIDTH8
            | SENTINEL_WIDTH16
            | SENTINEL_WIDTH64
            | SENTINEL_SET
            | SENTINEL_EVOLVING8
            | SENTINEL_EVOLVING16
            | SENTINEL_EVOLVING32
            | SENTINEL_EVOLVING8_RAW
            | SENTINEL_EVOLVING16_RAW
            | SENTINEL_EVOLVING32_RAW
    ) {
        return true;
    }
    #[cfg(feature = "datetime")]
    if name == SENTINEL_DATETIME {
        return true;
    }
    #[cfg(feature = "decimal")]
    if name == SENTINEL_DECIMAL {
        return true;
    }
    #[cfg(feature = "uuid")]
    if name == SENTINEL_UUID {
        return true;
    }
    false
}

/// Returns true if the name is an evolving sentinel (known variant).
pub(crate) fn is_evolving_known(name: &str) -> bool {
    matches!(
        name,
        SENTINEL_EVOLVING8 | SENTINEL_EVOLVING16 | SENTINEL_EVOLVING32
    )
}

/// Returns true if the name is an evolving raw sentinel (unknown variant).
pub(crate) fn is_evolving_raw(name: &str) -> bool {
    matches!(
        name,
        SENTINEL_EVOLVING8_RAW | SENTINEL_EVOLVING16_RAW | SENTINEL_EVOLVING32_RAW
    )
}

/// Map an evolving sentinel name to its wire width.
/// Returns `None` if the name is not an evolving sentinel.
pub(crate) fn evolving_width(name: &str) -> Option<crate::schema::Width> {
    match name {
        SENTINEL_EVOLVING8 | SENTINEL_EVOLVING8_RAW => Some(crate::schema::Width::W8),
        SENTINEL_EVOLVING16 | SENTINEL_EVOLVING16_RAW => Some(crate::schema::Width::W16),
        SENTINEL_EVOLVING32 | SENTINEL_EVOLVING32_RAW => Some(crate::schema::Width::W32),
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
        /// The `.0` field is the deserialized `Evolving<T>`.
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
// ---------------------------------------------------------------------------

/// Encode a value using varint encoding.
pub fn encode_varint<T: crate::varint::VarIntEncoding>(
    buf: &mut Vec<u8>,
    v: T,
) -> crate::CordResult<()> {
    crate::wire::write_varint(buf, v);
    Ok(())
}

/// Encode a CordEncode value with a specific width for length/variant prefix.
pub fn encode_with_width<T: crate::encode::CordEncode>(
    buf: &mut Vec<u8>,
    v: &T,
    width: crate::schema::Width,
) -> crate::CordResult<()> {
    v.encode_cord_with_width(buf, width)
}

/// Encode an Evolving wrapper with the given width.
pub fn encode_evolving<T: crate::encode::CordEncode>(
    buf: &mut Vec<u8>,
    v: &crate::Evolving<T>,
    width: crate::schema::Width,
) -> crate::CordResult<()> {
    match v {
        crate::Evolving::Known(inner) => {
            let mut payload = Vec::new();
            inner.encode_cord(&mut payload)?;
            crate::wire::write_length(buf, payload.len(), width)?;
            buf.extend_from_slice(&payload);
            Ok(())
        }
        crate::Evolving::Unknown(bytes) => {
            crate::wire::write_length(buf, bytes.len(), width)?;
            buf.extend_from_slice(bytes);
            Ok(())
        }
    }
}

/// Decode a varint value.
pub fn decode_varint<T: crate::varint::VarIntEncoding>(input: &mut &[u8]) -> crate::CordResult<T> {
    crate::wire::read_varint(input)
}

/// Decode a CordDecode value with a specific width.
pub fn decode_with_width<T: crate::encode::CordDecode>(
    input: &mut &[u8],
    width: crate::schema::Width,
) -> crate::CordResult<T> {
    T::decode_cord_with_width(input, width)
}

/// Decode an Evolving wrapper with the given width.
pub fn decode_evolving<T: crate::encode::CordDecode>(
    input: &mut &[u8],
    width: crate::schema::Width,
) -> crate::CordResult<crate::Evolving<T>> {
    let len = crate::wire::read_length(input, width, crate::de::DEFAULT_MAX_LENGTH)?;
    let payload = crate::wire::read_bytes(input, len)?;
    let mut sub_input = payload;
    match T::decode_cord(&mut sub_input) {
        Ok(value) if sub_input.is_empty() => Ok(crate::Evolving::Known(value)),
        _ => Ok(crate::Evolving::Unknown(payload.to_vec())),
    }
}

/// Encode a variant index with the given width.
pub fn encode_variant_index(
    buf: &mut Vec<u8>,
    idx: u32,
    width: crate::schema::Width,
) -> crate::CordResult<()> {
    crate::wire::write_variant_index(buf, idx, width)
}

/// Decode a variant index with the given width.
pub fn decode_variant_index(
    input: &mut &[u8],
    width: crate::schema::Width,
) -> crate::CordResult<u32> {
    crate::wire::read_variant_index(input, width)
}
