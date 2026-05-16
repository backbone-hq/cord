#[cfg(feature = "datetime")]
use crate::private::SENTINEL_DATETIME;
#[cfg(feature = "decimal")]
use crate::private::SENTINEL_DECIMAL;
#[cfg(feature = "uuid")]
use crate::private::SENTINEL_UUID;
use crate::private::{
    evolving_width, sentinel_to_hint, EncodingHint, SENTINEL_EVOLVING32, SENTINEL_SET,
};
use crate::result::{CordError, CordResult};
use crate::wire;
use crate::Bytes;
#[cfg(feature = "datetime")]
use crate::DateTime;
use crate::Map;
use crate::Set;
use serde::de::IntoDeserializer;
use serde::{de, Deserialize, Deserializer, Serialize};
use std::fmt::Formatter;
use std::marker::PhantomData;

/// Default maximum nesting depth for deserialization.
pub const DEFAULT_MAX_DEPTH: usize = 128;

/// Default maximum length for variable-length types (strings, bytes, sequences, maps).
/// Set to 16 MiB worth of elements.
pub const DEFAULT_MAX_LENGTH: usize = 16 * 1024 * 1024;

pub fn deserialize<'a, T>(bytes: &'a [u8]) -> CordResult<T>
where
    T: Deserialize<'a>,
{
    let mut deserializer = CordDeserializer::new(bytes);
    let result = T::deserialize(&mut deserializer)?;
    deserializer.end()?;
    Ok(result)
}

/// Options for configuring the Cord deserializer.
///
/// Use the builder methods to override the default limits, then call
/// [`deserialize`](DeserializeOptions::deserialize) to decode a value.
///
/// # Examples
///
/// ```
/// use cord::DeserializeOptions;
///
/// let encoded = cord::serialize(&42u32).unwrap();
/// let options = DeserializeOptions::new()
///     .max_depth(16)
///     .max_length(1024);
///
/// let value: u32 = options.deserialize(&encoded).unwrap();
/// assert_eq!(value, 42);
/// ```
#[derive(Debug, Clone)]
pub struct DeserializeOptions {
    max_depth: usize,
    max_length: usize,
}

impl Default for DeserializeOptions {
    fn default() -> Self {
        Self {
            max_depth: DEFAULT_MAX_DEPTH,
            max_length: DEFAULT_MAX_LENGTH,
        }
    }
}

impl DeserializeOptions {
    /// Create a new `DeserializeOptions` with default limits.
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the maximum nesting depth (default: 128).
    pub fn max_depth(mut self, max_depth: usize) -> Self {
        self.max_depth = max_depth;
        self
    }

    /// Set the maximum length for variable-length types in elements (default: 16 MiB).
    pub fn max_length(mut self, max_length: usize) -> Self {
        self.max_length = max_length;
        self
    }

    /// Deserialize a value from bytes using these options.
    pub fn deserialize<'a, T>(&self, bytes: &'a [u8]) -> CordResult<T>
    where
        T: Deserialize<'a>,
    {
        let mut deserializer =
            CordDeserializer::with_options(bytes, self.max_depth, self.max_length);
        let result = T::deserialize(&mut deserializer)?;
        deserializer.end()?;
        Ok(result)
    }
}

/// Deserialize a value from the beginning of a byte slice, returning the
/// value and the number of bytes consumed. Unlike [`deserialize`], this
/// does **not** require that all bytes are consumed.
pub fn deserialize_prefix<'a, T>(bytes: &'a [u8]) -> CordResult<(T, usize)>
where
    T: Deserialize<'a>,
{
    let mut deserializer = CordDeserializer::new(bytes);
    let result = T::deserialize(&mut deserializer)?;
    let consumed = bytes.len() - deserializer.input.len();
    Ok((result, consumed))
}

pub(crate) struct CordDeserializer<'de> {
    pub(crate) input: &'de [u8],
    hint: EncodingHint,
    depth: usize,
    max_depth: usize,
    max_length: usize,
}

impl<'de> CordDeserializer<'de> {
    pub(crate) fn new(input: &'de [u8]) -> Self {
        CordDeserializer {
            input,
            hint: EncodingHint::Default,
            depth: 0,
            max_depth: DEFAULT_MAX_DEPTH,
            max_length: DEFAULT_MAX_LENGTH,
        }
    }

    pub(crate) fn with_options(input: &'de [u8], max_depth: usize, max_length: usize) -> Self {
        CordDeserializer {
            input,
            hint: EncodingHint::Default,
            depth: 0,
            max_depth,
            max_length,
        }
    }

    fn enter_nested(&mut self) -> CordResult<()> {
        self.depth += 1;
        if self.depth > self.max_depth {
            Err(CordError::DepthLimitExceeded)
        } else {
            Ok(())
        }
    }

    fn leave_nested(&mut self) {
        self.depth -= 1;
    }

    fn end(&mut self) -> CordResult<()> {
        if self.input.is_empty() {
            Ok(())
        } else {
            Err(CordError::TrailingBytes)
        }
    }

    /// Reset all sentinel state to defaults.
    ///
    /// Called before each field/element/key/value deserialization to match
    /// the serializer's behavior of creating a fresh CordSerializer per element.
    fn reset_sentinels(&mut self) {
        self.hint = EncodingHint::Default;
    }
}

impl<'de> CordDeserializer<'de> {
    fn next(&mut self) -> CordResult<u8> {
        Ok(wire::read_bytes(&mut self.input, 1)?[0])
    }

    fn read_bytes(&mut self, size: usize) -> CordResult<&'de [u8]> {
        wire::read_bytes(&mut self.input, size)
    }

    fn parse_bool(&mut self) -> CordResult<bool> {
        wire::read_bool(&mut self.input)
    }

    fn parse_varint<T: crate::varint::VarIntEncoding>(&mut self) -> CordResult<T> {
        wire::read_varint(&mut self.input)
    }

    fn parse_length(&mut self) -> CordResult<usize> {
        let width = self.hint.width();
        self.hint = EncodingHint::Default;
        wire::read_length(&mut self.input, width, self.max_length)
    }

    fn parse_variant_index(&mut self) -> CordResult<u32> {
        let width = self.hint.width();
        self.hint = EncodingHint::Default;
        wire::read_variant_index(&mut self.input, width)
    }

    fn parse_bytes(&mut self) -> CordResult<&'de [u8]> {
        let len = self.parse_length()?;
        self.read_bytes(len)
    }

    fn parse_string(&mut self) -> CordResult<&'de str> {
        let width = self.hint.width();
        self.hint = EncodingHint::Default;
        wire::read_str(&mut self.input, width, self.max_length)
    }
}

macro_rules! deserialize_fixed {
    ($(($int:ty, $deserialize:ident, $visit:ident, $size:expr)),*) => {
        $(
            fn $deserialize<V>(self, visitor: V) -> CordResult<V::Value>
            where
                V: de::Visitor<'de>,
            {

                if self.hint == EncodingHint::VarInt {
                    self.hint = EncodingHint::Default;
                    visitor.$visit(self.parse_varint::<$int>()?)
                } else {
                    let bytes = self.read_bytes($size)?;
                    let arr: [u8; $size] = bytes.try_into().unwrap();
                    visitor.$visit(<$int>::from_be_bytes(arr))
                }
            }
        )*
    };
}

impl<'de> de::Deserializer<'de> for &mut CordDeserializer<'de> {
    type Error = CordError;

    fn deserialize_any<V>(self, visitor: V) -> CordResult<V::Value>
    where
        V: de::Visitor<'de>,
    {
        visitor.visit_bytes(self.parse_bytes()?)
    }

    fn deserialize_bool<V>(self, visitor: V) -> CordResult<V::Value>
    where
        V: de::Visitor<'de>,
    {
        visitor.visit_bool(self.parse_bool()?)
    }

    deserialize_fixed!(
        (i8, deserialize_i8, visit_i8, 1),
        (i16, deserialize_i16, visit_i16, 2),
        (i32, deserialize_i32, visit_i32, 4),
        (i64, deserialize_i64, visit_i64, 8),
        (i128, deserialize_i128, visit_i128, 16),
        (u8, deserialize_u8, visit_u8, 1),
        (u16, deserialize_u16, visit_u16, 2),
        (u32, deserialize_u32, visit_u32, 4),
        (u64, deserialize_u64, visit_u64, 8),
        (u128, deserialize_u128, visit_u128, 16)
    );

    fn deserialize_f32<V>(self, visitor: V) -> CordResult<V::Value>
    where
        V: de::Visitor<'de>,
    {
        let bytes = self.read_bytes(4)?;
        let arr: [u8; 4] = bytes.try_into().unwrap();
        let v = f32::from_be_bytes(arr);
        if v.is_nan() {
            return Err(CordError::NanNotAllowed);
        }
        if v.to_bits() == (-0.0_f32).to_bits() {
            return Err(CordError::NegativeZeroNotAllowed);
        }
        visitor.visit_f32(v)
    }

    fn deserialize_f64<V>(self, visitor: V) -> CordResult<V::Value>
    where
        V: de::Visitor<'de>,
    {
        let bytes = self.read_bytes(8)?;
        let arr: [u8; 8] = bytes.try_into().unwrap();
        let v = f64::from_be_bytes(arr);
        if v.is_nan() {
            return Err(CordError::NanNotAllowed);
        }
        if v.to_bits() == (-0.0_f64).to_bits() {
            return Err(CordError::NegativeZeroNotAllowed);
        }
        visitor.visit_f64(v)
    }

    fn deserialize_char<V>(self, visitor: V) -> CordResult<V::Value>
    where
        V: de::Visitor<'de>,
    {
        let s = self.parse_string()?;
        let mut chars = s.chars();
        match (chars.next(), chars.next()) {
            (Some(c), None) => visitor.visit_char(c),
            _ => Err(CordError::ValidationError("Expected exactly one character")),
        }
    }

    fn deserialize_str<V>(self, visitor: V) -> CordResult<V::Value>
    where
        V: de::Visitor<'de>,
    {
        visitor.visit_borrowed_str(self.parse_string()?)
    }

    fn deserialize_string<V>(self, visitor: V) -> CordResult<V::Value>
    where
        V: de::Visitor<'de>,
    {
        self.deserialize_str(visitor)
    }

    fn deserialize_bytes<V>(self, visitor: V) -> CordResult<V::Value>
    where
        V: de::Visitor<'de>,
    {
        visitor.visit_borrowed_bytes(self.parse_bytes()?)
    }

    fn deserialize_byte_buf<V>(self, visitor: V) -> CordResult<V::Value>
    where
        V: de::Visitor<'de>,
    {
        self.deserialize_bytes(visitor)
    }

    fn deserialize_option<V>(self, visitor: V) -> CordResult<V::Value>
    where
        V: de::Visitor<'de>,
    {
        let byte = self.next()?;

        match byte {
            0 => visitor.visit_none(),
            1 => {
                self.enter_nested()?;
                let result = visitor.visit_some(&mut *self);
                self.leave_nested();
                result
            }
            _ => Err(CordError::ValidationError("Invalid option variant")),
        }
    }

    fn deserialize_unit<V>(self, visitor: V) -> CordResult<V::Value>
    where
        V: de::Visitor<'de>,
    {
        visitor.visit_unit()
    }

    fn deserialize_unit_struct<V>(self, _name: &'static str, visitor: V) -> CordResult<V::Value>
    where
        V: de::Visitor<'de>,
    {
        self.deserialize_unit(visitor)
    }

    fn deserialize_newtype_struct<V>(self, name: &'static str, visitor: V) -> CordResult<V::Value>
    where
        V: de::Visitor<'de>,
    {
        if let Some(width) = evolving_width(name) {
            if self.hint.is_active() {
                return Err(CordError::ValidationError(
                    "Conflicting sentinel nesting: cannot combine width/varint wrappers",
                ));
            }
            let len = wire::read_length(&mut self.input, width, self.max_length)?;
            let payload = self.read_bytes(len)?;
            return visitor.visit_borrowed_bytes(payload);
        }
        #[cfg(feature = "uuid")]
        if name == SENTINEL_UUID {
            // UUID: read exactly 16 raw bytes, no length prefix.
            let bytes = self.read_bytes(16)?;
            return visitor.visit_newtype_struct(crate::de::FixedBytesDeserializer { bytes });
        }
        if let Some(hint) = sentinel_to_hint(name) {
            if self.hint.is_active() {
                return Err(CordError::ValidationError(
                    "Conflicting sentinel nesting: cannot combine width/varint wrappers",
                ));
            }
            self.hint = hint;
        }
        visitor.visit_newtype_struct(&mut *self)
    }

    fn deserialize_seq<V>(self, visitor: V) -> CordResult<V::Value>
    where
        V: de::Visitor<'de>,
    {
        self.enter_nested()?;
        let len = self.parse_length()?;
        let result = visitor.visit_seq(SeqDeserializer::new(self, len));
        self.leave_nested();
        result
    }

    fn deserialize_tuple<V>(self, len: usize, visitor: V) -> CordResult<V::Value>
    where
        V: de::Visitor<'de>,
    {
        self.enter_nested()?;
        let result = visitor.visit_seq(SeqDeserializer::new(self, len));
        self.leave_nested();
        result
    }

    fn deserialize_tuple_struct<V>(
        self,
        _name: &'static str,
        len: usize,
        visitor: V,
    ) -> CordResult<V::Value>
    where
        V: de::Visitor<'de>,
    {
        self.enter_nested()?;
        let result = visitor.visit_seq(SeqDeserializer::new(self, len));
        self.leave_nested();
        result
    }

    fn deserialize_map<V>(self, visitor: V) -> CordResult<V::Value>
    where
        V: de::Visitor<'de>,
    {
        self.enter_nested()?;
        let len = self.parse_length()?;
        let result = visitor.visit_map(MapDeserializer::new(self, len));
        self.leave_nested();
        result
    }

    fn deserialize_struct<V>(
        self,
        _name: &'static str,
        fields: &'static [&'static str],
        visitor: V,
    ) -> CordResult<V::Value>
    where
        V: de::Visitor<'de>,
    {
        self.enter_nested()?;
        let result = visitor.visit_seq(SeqDeserializer::new(self, fields.len()));
        self.leave_nested();
        result
    }

    fn deserialize_enum<V>(
        self,
        _name: &'static str,
        _variants: &'static [&'static str],
        visitor: V,
    ) -> CordResult<V::Value>
    where
        V: de::Visitor<'de>,
    {
        self.enter_nested()?;
        let result = visitor.visit_enum(&mut *self);
        self.leave_nested();
        result
    }

    fn deserialize_identifier<V>(self, _visitor: V) -> CordResult<V::Value>
    where
        V: de::Visitor<'de>,
    {
        self.deserialize_bytes(_visitor)
    }

    fn deserialize_ignored_any<V>(self, _visitor: V) -> CordResult<V::Value>
    where
        V: de::Visitor<'de>,
    {
        Err(CordError::NotSupported("ignored any"))
    }
}

struct SeqDeserializer<'a, 'de: 'a> {
    de: &'a mut CordDeserializer<'de>,
    remaining: usize,
}

impl<'a, 'de> SeqDeserializer<'a, 'de> {
    fn new(de: &'a mut CordDeserializer<'de>, remaining: usize) -> Self {
        Self { de, remaining }
    }
}

impl<'de> de::SeqAccess<'de> for SeqDeserializer<'_, 'de> {
    type Error = CordError;

    fn next_element_seed<T>(&mut self, seed: T) -> CordResult<Option<T::Value>>
    where
        T: de::DeserializeSeed<'de>,
    {
        if self.remaining == 0 {
            Ok(None)
        } else {
            self.remaining -= 1;
            self.de.reset_sentinels();
            seed.deserialize(&mut *self.de).map(Some)
        }
    }

    fn size_hint(&self) -> Option<usize> {
        Some(self.remaining)
    }
}

struct MapDeserializer<'a, 'de: 'a> {
    de: &'a mut CordDeserializer<'de>,
    remaining: usize,
    previous_key: Option<&'de [u8]>,
}

impl<'a, 'de> MapDeserializer<'a, 'de> {
    fn new(de: &'a mut CordDeserializer<'de>, remaining: usize) -> Self {
        Self {
            de,
            remaining,
            previous_key: None,
        }
    }
}

impl<'de> de::MapAccess<'de> for MapDeserializer<'_, 'de> {
    type Error = CordError;

    fn next_key_seed<K>(&mut self, seed: K) -> CordResult<Option<K::Value>>
    where
        K: de::DeserializeSeed<'de>,
    {
        if self.remaining == 0 {
            Ok(None)
        } else {
            self.remaining -= 1;
            self.de.reset_sentinels();

            let start_input = self.de.input;
            let key = seed.deserialize(&mut *self.de)?;
            let end_input = self.de.input;
            let key_bytes = &start_input[..start_input.len() - end_input.len()];

            if let Some(prev) = self.previous_key {
                if prev >= key_bytes {
                    return Err(CordError::ValidationError(
                        "Unordered or duplicate map keys",
                    ));
                }
            }
            self.previous_key = Some(key_bytes);

            Ok(Some(key))
        }
    }

    fn next_value_seed<V>(&mut self, seed: V) -> CordResult<V::Value>
    where
        V: de::DeserializeSeed<'de>,
    {
        self.de.reset_sentinels();
        seed.deserialize(&mut *self.de)
    }

    fn size_hint(&self) -> Option<usize> {
        Some(self.remaining)
    }
}

impl<'de> de::EnumAccess<'de> for &mut CordDeserializer<'de> {
    type Error = CordError;
    type Variant = Self;

    fn variant_seed<V>(self, seed: V) -> CordResult<(V::Value, Self::Variant)>
    where
        V: de::DeserializeSeed<'de>,
    {
        let variant_index = self.parse_variant_index()?;
        let result: CordResult<V::Value> = seed.deserialize(variant_index.into_deserializer());
        match result {
            Ok(v) => Ok((v, self)),
            Err(_) => Err(CordError::UnknownVariant(variant_index)),
        }
    }
}

impl<'de> de::VariantAccess<'de> for &mut CordDeserializer<'de> {
    type Error = CordError;

    fn unit_variant(self) -> CordResult<()> {
        Ok(())
    }

    fn newtype_variant_seed<T>(self, seed: T) -> CordResult<T::Value>
    where
        T: de::DeserializeSeed<'de>,
    {
        seed.deserialize(self)
    }

    fn tuple_variant<V>(self, len: usize, visitor: V) -> CordResult<V::Value>
    where
        V: de::Visitor<'de>,
    {
        de::Deserializer::deserialize_tuple(self, len, visitor)
    }

    fn struct_variant<V>(self, fields: &'static [&'static str], visitor: V) -> CordResult<V::Value>
    where
        V: de::Visitor<'de>,
    {
        de::Deserializer::deserialize_tuple(self, fields.len(), visitor)
    }
}

struct BytesVisitor;

impl de::Visitor<'_> for BytesVisitor {
    type Value = Bytes;

    fn expecting(&self, formatter: &mut Formatter) -> std::fmt::Result {
        formatter.write_str("bytes")
    }

    fn visit_bytes<E>(self, v: &[u8]) -> CordResult<Self::Value, E>
    where
        E: de::Error,
    {
        Ok(Bytes::from(v.to_vec()))
    }

    fn visit_byte_buf<E>(self, v: Vec<u8>) -> std::result::Result<Self::Value, E>
    where
        E: de::Error,
    {
        Ok(Bytes::from(v))
    }
}

impl<'de> de::Deserialize<'de> for Bytes {
    fn deserialize<D>(deserializer: D) -> CordResult<Bytes, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_bytes(BytesVisitor)
    }
}

struct SetVisitor<T: PartialEq> {
    marker: PhantomData<fn() -> Set<T>>,
}

impl<T: PartialEq> SetVisitor<T> {
    fn new() -> Self {
        SetVisitor {
            marker: PhantomData,
        }
    }
}

impl<'de, T> de::Visitor<'de> for SetVisitor<T>
where
    T: std::hash::Hash + Eq + Serialize + Deserialize<'de>,
{
    type Value = Set<T>;

    fn expecting(&self, formatter: &mut Formatter) -> std::fmt::Result {
        formatter.write_str("set")
    }

    fn visit_seq<A>(self, mut seq: A) -> CordResult<Self::Value, A::Error>
    where
        A: de::SeqAccess<'de>,
    {
        let mut set = std::collections::HashSet::new();
        let mut cur_buf = Vec::new();
        let mut prev_buf = Vec::new();
        let mut has_prev = false;
        while let Some(element) = seq.next_element::<T>()? {
            cur_buf.clear();
            crate::ser::serialize_into(&mut cur_buf, &element).map_err(de::Error::custom)?;
            if has_prev && prev_buf.as_slice() >= cur_buf.as_slice() {
                return Err(de::Error::custom(CordError::DuplicateSetElement));
            }

            std::mem::swap(&mut cur_buf, &mut prev_buf);
            has_prev = true;
            set.insert(element);
        }
        Ok(Set::from(set))
    }
}

struct SetNewtypeVisitor<T: PartialEq> {
    marker: PhantomData<fn() -> Set<T>>,
}

impl<'de, T> de::Visitor<'de> for SetNewtypeVisitor<T>
where
    T: std::hash::Hash + Eq + Serialize + Deserialize<'de>,
{
    type Value = Set<T>;

    fn expecting(&self, formatter: &mut Formatter) -> std::fmt::Result {
        formatter.write_str("set")
    }

    fn visit_newtype_struct<D>(self, deserializer: D) -> CordResult<Self::Value, D::Error>
    where
        D: de::Deserializer<'de>,
    {
        deserializer.deserialize_seq(SetVisitor::<T>::new())
    }
}

impl<'de, T> de::Deserialize<'de> for Set<T>
where
    T: Deserialize<'de> + std::hash::Hash + Eq + Serialize,
{
    fn deserialize<D>(deserializer: D) -> CordResult<Set<T>, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_newtype_struct(
            SENTINEL_SET,
            SetNewtypeVisitor {
                marker: PhantomData,
            },
        )
    }
}

#[cfg(feature = "datetime")]
struct DateTimeTupleVisitor;

#[cfg(feature = "datetime")]
impl<'de> de::Visitor<'de> for DateTimeTupleVisitor {
    type Value = DateTime;

    fn expecting(&self, formatter: &mut Formatter) -> std::fmt::Result {
        formatter.write_str("datetime (i64 seconds, u32 nanoseconds)")
    }

    fn visit_seq<A>(self, mut seq: A) -> std::result::Result<Self::Value, A::Error>
    where
        A: de::SeqAccess<'de>,
    {
        let seconds: i64 = seq
            .next_element()?
            .ok_or_else(|| de::Error::invalid_length(0, &self))?;
        let nanos: u32 = seq
            .next_element()?
            .ok_or_else(|| de::Error::invalid_length(1, &self))?;

        if nanos >= 1_000_000_000 {
            return Err(de::Error::custom(format!(
                "DateTime nanoseconds {nanos} exceeds 999_999_999"
            )));
        }

        let utc_dt =
            chrono::DateTime::<chrono::Utc>::from_timestamp(seconds, nanos).ok_or_else(|| {
                de::Error::custom(format!("DateTime ({seconds}s, {nanos}ns) is out of range"))
            })?;

        Ok(utc_dt.into())
    }
}

#[cfg(feature = "datetime")]
struct DateTimeNewtypeVisitor;

#[cfg(feature = "datetime")]
impl<'de> de::Visitor<'de> for DateTimeNewtypeVisitor {
    type Value = DateTime;

    fn expecting(&self, formatter: &mut Formatter) -> std::fmt::Result {
        formatter.write_str("datetime")
    }

    fn visit_newtype_struct<D>(self, deserializer: D) -> CordResult<Self::Value, D::Error>
    where
        D: de::Deserializer<'de>,
    {
        deserializer.deserialize_tuple(2, DateTimeTupleVisitor)
    }
}

#[cfg(feature = "datetime")]
impl<'de> de::Deserialize<'de> for DateTime {
    fn deserialize<D>(deserializer: D) -> CordResult<DateTime, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_newtype_struct(SENTINEL_DATETIME, DateTimeNewtypeVisitor)
    }
}

#[cfg(feature = "decimal")]
struct DecimalVisitor;

#[cfg(feature = "decimal")]
impl<'de> de::Visitor<'de> for DecimalVisitor {
    type Value = crate::Decimal;

    fn expecting(&self, formatter: &mut Formatter) -> std::fmt::Result {
        formatter.write_str("decimal (u8 scale, bytes two's-complement unscaled)")
    }

    fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
    where
        A: de::SeqAccess<'de>,
    {
        let scale: u8 = seq
            .next_element()?
            .ok_or_else(|| de::Error::invalid_length(0, &self))?;
        let tc_bytes: Bytes = seq
            .next_element()?
            .ok_or_else(|| de::Error::invalid_length(1, &self))?;

        // Reject non-minimal two's-complement encodings.
        // Minimal encoding rules:
        //   - Empty bytes encodes zero
        //   - No redundant leading 0x00 (positive) or 0xFF (negative) bytes
        //     unless needed to preserve the sign bit
        if tc_bytes.is_empty() {
            return Err(de::Error::custom(
                "Non-minimal BigInt encoding: zero requires at least one byte",
            ));
        }
        if tc_bytes.len() >= 2 {
            let first = tc_bytes[0];
            let second_high_bit = tc_bytes[1] & 0x80;
            if first == 0x00 && second_high_bit == 0 {
                return Err(de::Error::custom(
                    "Non-minimal BigInt encoding: redundant leading 0x00",
                ));
            }
            if first == 0xFF && second_high_bit != 0 {
                return Err(de::Error::custom(
                    "Non-minimal BigInt encoding: redundant leading 0xFF",
                ));
            }
        }

        let unscaled = num_bigint::BigInt::from_signed_bytes_be(&tc_bytes);

        // Reject non-normalized (scale, unscaled) pairs. The canonical form
        // has no trailing zeros in unscaled (i.e., normalization is a no-op).
        let normalized = crate::Decimal::new(unscaled.clone(), scale);
        if normalized.scale() != scale || *normalized.unscaled() != unscaled {
            return Err(de::Error::custom(
                "Non-canonical Decimal: scale/unscaled not normalized",
            ));
        }

        Ok(normalized)
    }
}

#[cfg(feature = "decimal")]
struct DecimalNewtypeVisitor;

#[cfg(feature = "decimal")]
impl<'de> de::Visitor<'de> for DecimalNewtypeVisitor {
    type Value = crate::Decimal;

    fn expecting(&self, formatter: &mut Formatter) -> std::fmt::Result {
        formatter.write_str("decimal")
    }

    fn visit_newtype_struct<D>(self, deserializer: D) -> CordResult<Self::Value, D::Error>
    where
        D: de::Deserializer<'de>,
    {
        deserializer.deserialize_tuple(2, DecimalVisitor)
    }
}

#[cfg(feature = "decimal")]
impl<'de> de::Deserialize<'de> for crate::Decimal {
    fn deserialize<D>(deserializer: D) -> CordResult<crate::Decimal, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_newtype_struct(SENTINEL_DECIMAL, DecimalNewtypeVisitor)
    }
}

#[cfg(feature = "uuid")]
struct UuidVisitor;

#[cfg(feature = "uuid")]
impl<'de> de::Visitor<'de> for UuidVisitor {
    type Value = crate::Uuid;

    fn expecting(&self, formatter: &mut Formatter) -> std::fmt::Result {
        formatter.write_str("uuid (16 bytes)")
    }

    fn visit_bytes<E>(self, v: &[u8]) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        if v.len() != 16 {
            return Err(de::Error::invalid_length(v.len(), &"16 bytes for UUID"));
        }
        let inner = uuid::Uuid::from_slice(v)
            .map_err(|e| de::Error::custom(format!("Invalid UUID bytes: {e}")))?;
        Ok(crate::Uuid::new(inner))
    }

    fn visit_byte_buf<E>(self, v: Vec<u8>) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        self.visit_bytes(&v)
    }
}

#[cfg(feature = "uuid")]
struct UuidNewtypeVisitor;

#[cfg(feature = "uuid")]
impl<'de> de::Visitor<'de> for UuidNewtypeVisitor {
    type Value = crate::Uuid;

    fn expecting(&self, formatter: &mut Formatter) -> std::fmt::Result {
        formatter.write_str("uuid")
    }

    fn visit_newtype_struct<D>(self, deserializer: D) -> CordResult<Self::Value, D::Error>
    where
        D: de::Deserializer<'de>,
    {
        deserializer.deserialize_bytes(UuidVisitor)
    }
}

#[cfg(feature = "uuid")]
impl<'de> de::Deserialize<'de> for crate::Uuid {
    fn deserialize<D>(deserializer: D) -> CordResult<crate::Uuid, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_newtype_struct(SENTINEL_UUID, UuidNewtypeVisitor)
    }
}

struct MapVisitor<K, V> {
    marker: PhantomData<fn() -> Map<K, V>>,
}

impl<K, V> MapVisitor<K, V> {
    fn new() -> Self {
        Self {
            marker: PhantomData,
        }
    }
}

impl<'de, K, V> de::Visitor<'de> for MapVisitor<K, V>
where
    K: std::hash::Hash + Eq + Deserialize<'de>,
    V: Deserialize<'de>,
{
    type Value = Map<K, V>;

    fn expecting(&self, formatter: &mut Formatter) -> std::fmt::Result {
        formatter.write_str("map")
    }

    fn visit_map<A>(self, mut map: A) -> CordResult<Self::Value, A::Error>
    where
        A: de::MapAccess<'de>,
    {
        let mut inner = std::collections::HashMap::new();
        while let Some((key, value)) = map.next_entry()? {
            inner.insert(key, value);
        }
        Ok(Map::from(inner))
    }
}

impl<'de, K, V> de::Deserialize<'de> for Map<K, V>
where
    K: std::hash::Hash + Eq + Deserialize<'de>,
    V: Deserialize<'de>,
{
    fn deserialize<D>(deserializer: D) -> CordResult<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_map(MapVisitor::new())
    }
}

/// Default Deserialize impl for `Evolving<T>` — uses 32-bit length prefix.
///
/// For other widths (8-bit, 16-bit), use `#[cord(evolving = 8)]` or
/// `#[cord(evolving = 16)]` with `#[derive(Cord)]`.
struct EvolvingVisitor<T> {
    marker: PhantomData<T>,
}

impl<'de, T: Deserialize<'de>> de::Visitor<'de> for EvolvingVisitor<T> {
    type Value = crate::Evolving<T>;

    fn expecting(&self, formatter: &mut Formatter) -> std::fmt::Result {
        formatter.write_str("evolving enum")
    }

    fn visit_borrowed_bytes<E: de::Error>(self, payload: &'de [u8]) -> Result<Self::Value, E> {
        let mut sub_de = CordDeserializer::new(payload);
        match T::deserialize(&mut sub_de) {
            Ok(value) => {
                if sub_de.input.is_empty() {
                    Ok(crate::Evolving::Known(value))
                } else {
                    Err(E::custom("trailing bytes in evolving payload"))
                }
            }
            Err(CordError::UnknownVariant(_)) => Ok(crate::Evolving::Unknown(payload.to_vec())),
            Err(e) => Err(E::custom(e)),
        }
    }

    fn visit_byte_buf<E: de::Error>(self, payload: Vec<u8>) -> Result<Self::Value, E> {
        Ok(crate::Evolving::Unknown(payload))
    }

    fn visit_newtype_struct<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
    where
        D: de::Deserializer<'de>,
    {
        T::deserialize(deserializer).map(crate::Evolving::Known)
    }
}

impl<'de, T: Deserialize<'de>> Deserialize<'de> for crate::Evolving<T> {
    fn deserialize<D>(deserializer: D) -> CordResult<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_newtype_struct(
            SENTINEL_EVOLVING32,
            EvolvingVisitor {
                marker: PhantomData,
            },
        )
    }
}

/// A minimal deserializer that serves a fixed byte slice for `deserialize_bytes`.
///
/// Used by the UUID sentinel so the inner `UuidVisitor` can receive
/// exactly 16 bytes without a length prefix on the wire.
#[cfg(feature = "uuid")]
pub(crate) struct FixedBytesDeserializer<'de> {
    pub(crate) bytes: &'de [u8],
}

#[cfg(feature = "uuid")]
impl<'de> de::Deserializer<'de> for FixedBytesDeserializer<'de> {
    type Error = CordError;

    fn deserialize_any<V>(self, _visitor: V) -> CordResult<V::Value>
    where
        V: de::Visitor<'de>,
    {
        Err(CordError::ValidationError(
            "FixedBytesDeserializer only supports deserialize_bytes",
        ))
    }

    fn deserialize_bytes<V>(self, visitor: V) -> CordResult<V::Value>
    where
        V: de::Visitor<'de>,
    {
        visitor.visit_borrowed_bytes(self.bytes)
    }

    fn deserialize_byte_buf<V>(self, visitor: V) -> CordResult<V::Value>
    where
        V: de::Visitor<'de>,
    {
        self.deserialize_bytes(visitor)
    }

    serde::forward_to_deserialize_any! {
        bool i8 i16 i32 i64 i128 u8 u16 u32 u64 u128 f32 f64 char str string
        option unit unit_struct newtype_struct seq tuple tuple_struct map
        struct enum identifier ignored_any
    }
}

#[cfg(test)]
mod tests {
    use super::deserialize;
    #[cfg(feature = "datetime")]
    use crate::DateTime;
    use crate::{Bytes, Cord, Evolving, Map};
    #[cfg(feature = "datetime")]
    use chrono::Utc;
    use serde::{Deserialize, Serialize};

    #[derive(Cord, Debug, PartialEq)]
    enum Enum {
        Unit,
        Container(u16),
        TupleContainer(u16, u16),
        Struct { field: u32 },
    }

    #[derive(Debug, Deserialize, PartialEq)]
    struct Struct {
        int: u16,
        option: Option<u8>,
        seq: Vec<String>,
        boolean: bool,
    }

    #[test]
    fn deserialize_unit() {
        let unit_input: Vec<u8> = vec![];
        assert_eq!(deserialize::<()>(&unit_input).unwrap(), ());
    }

    #[test]
    fn deserialize_booleans() {
        let false_input: Vec<u8> = vec![0];
        assert!(!deserialize::<bool>(&false_input).unwrap());

        let true_input: Vec<u8> = vec![1];
        assert!(deserialize::<bool>(&true_input).unwrap());
    }

    #[test]
    fn deserialize_numbers() {
        assert_eq!(deserialize::<u8>(&62_u8.to_be_bytes()).unwrap(), 62_u8);

        assert_eq!(deserialize::<i8>(&(-30_i8).to_be_bytes()).unwrap(), -30_i8);

        assert_eq!(
            deserialize::<u32>(&1293012_u32.to_be_bytes()).unwrap(),
            1293012_u32
        );

        assert_eq!(
            deserialize::<i32>(&(-1238470_i32).to_be_bytes()).unwrap(),
            -1238470_i32
        );

        assert_eq!(deserialize::<u32>(&12_u32.to_be_bytes()).unwrap(), 12_u32);

        assert_eq!(
            deserialize::<u128>(&u128::MAX.to_be_bytes()).unwrap(),
            u128::MAX
        );

        assert_eq!(
            deserialize::<i128>(&i128::MIN.to_be_bytes()).unwrap(),
            i128::MIN
        );

        assert_eq!(
            deserialize::<i128>(&42_i128.to_be_bytes()).unwrap(),
            42_i128
        );
    }

    #[test]
    fn deserialize_strings() {
        let mut input = 4_u32.to_be_bytes().to_vec();
        input.extend_from_slice(b"test");
        assert_eq!(deserialize::<String>(&input).unwrap(), "test");
    }

    #[test]
    fn deserialize_empty_strings() {
        let input = 0_u32.to_be_bytes().to_vec();
        assert_eq!(deserialize::<String>(&input).unwrap(), "");
    }

    #[test]
    fn deserialize_large_bytearrays() {
        let length: usize = 300;
        let mut input = (length as u32).to_be_bytes().to_vec();
        input.extend(vec![b'0'; length]);
        assert_eq!(deserialize::<Vec<u8>>(&input).unwrap(), vec![b'0'; length]);
    }

    #[test]
    fn deserialize_empty_bytearrays() {
        let input = 0_u32.to_be_bytes().to_vec();
        assert_eq!(deserialize::<Vec<u8>>(&input).unwrap(), Vec::<u8>::new());
    }

    #[test]
    fn deserialize_bytes() {
        let mut input = 3_u32.to_be_bytes().to_vec();
        input.extend_from_slice(&[0, 1, 2]);
        assert_eq!(
            deserialize::<Bytes>(&input).unwrap(),
            Bytes::from(vec![0, 1, 2])
        );
    }

    #[cfg(feature = "datetime")]
    #[test]
    fn deserialize_datetime() {
        let datetime: DateTime = chrono::DateTime::parse_from_rfc3339("2023-10-05T14:30:00.000Z")
            .unwrap()
            .with_timezone(&Utc)
            .into();
        let mut input = Vec::new();
        input.extend_from_slice(&datetime.chrono.timestamp().to_be_bytes());
        input.extend_from_slice(&datetime.chrono.timestamp_subsec_nanos().to_be_bytes());

        assert_eq!(deserialize::<DateTime>(&input).unwrap(), datetime);
    }

    #[test]
    fn deserialize_set() {
        let expected: crate::Set<String> = vec!["a", "b", "c", "d", "e", "f", "test"]
            .into_iter()
            .map(|s| s.to_string())
            .collect();

        // Serialize then deserialize for roundtrip
        let bytes = crate::serialize(&expected).unwrap();
        assert_eq!(deserialize::<crate::Set<String>>(&bytes).unwrap(), expected);
    }

    #[test]
    fn deserialize_enum() {
        assert_eq!(
            deserialize::<Enum>(&0_u32.to_be_bytes()).unwrap(),
            Enum::Unit
        );

        let mut input = 1_u32.to_be_bytes().to_vec();
        input.extend_from_slice(&1_u16.to_be_bytes());
        assert_eq!(deserialize::<Enum>(&input).unwrap(), Enum::Container(1));

        let mut input = 2_u32.to_be_bytes().to_vec();
        input.extend_from_slice(&1_u16.to_be_bytes());
        input.extend_from_slice(&2_u16.to_be_bytes());
        assert_eq!(
            deserialize::<Enum>(&input).unwrap(),
            Enum::TupleContainer(1, 2)
        );

        let mut input = 3_u32.to_be_bytes().to_vec();
        input.extend_from_slice(&1_u32.to_be_bytes());
        assert_eq!(
            deserialize::<Enum>(&input).unwrap(),
            Enum::Struct { field: 1 }
        );
    }

    #[test]
    fn deserialize_struct() {
        let mut input = Vec::new();
        input.extend_from_slice(&99_u16.to_be_bytes());
        input.push(1); // option discriminant
        input.push(7); // option value
        input.extend_from_slice(&2_u32.to_be_bytes()); // seq length
        input.extend_from_slice(&5_u32.to_be_bytes()); // "first" length
        input.extend_from_slice(b"first");
        input.extend_from_slice(&6_u32.to_be_bytes()); // "second" length
        input.extend_from_slice(b"second");
        input.push(1); // boolean

        assert_eq!(
            deserialize::<Struct>(&input).unwrap(),
            Struct {
                int: 99,
                option: Some(7_u8),
                seq: vec![String::from("first"), String::from("second")],
                boolean: true
            }
        );
    }

    #[test]
    fn deserialize_map() {
        let mut input = Vec::new();
        input.extend_from_slice(&2_u32.to_be_bytes()); // map length
                                                       // "a" -> 1
        input.extend_from_slice(&1_u32.to_be_bytes());
        input.extend_from_slice(b"a");
        input.extend_from_slice(&1_u32.to_be_bytes());
        // "b" -> 2
        input.extend_from_slice(&1_u32.to_be_bytes());
        input.extend_from_slice(b"b");
        input.extend_from_slice(&2_u32.to_be_bytes());

        let expected = Map::from([("a".to_string(), 1_u32), ("b".to_string(), 2_u32)]);
        assert_eq!(deserialize::<Map<String, u32>>(&input).unwrap(), expected);
    }

    #[test]
    fn deserialize_map_disordered_keys_fails() {
        let mut input = Vec::new();
        input.extend_from_slice(&2_u32.to_be_bytes());
        // "b" first (out of order)
        input.extend_from_slice(&1_u32.to_be_bytes());
        input.extend_from_slice(b"b");
        input.extend_from_slice(&2_u32.to_be_bytes());
        // "a" second
        input.extend_from_slice(&1_u32.to_be_bytes());
        input.extend_from_slice(b"a");
        input.extend_from_slice(&1_u32.to_be_bytes());

        assert!(deserialize::<Map<String, u32>>(&input).is_err());
    }

    #[test]
    fn deserialize_map_duplicate_keys_fails() {
        let mut input = Vec::new();
        input.extend_from_slice(&2_u32.to_be_bytes());
        // "a" -> 1
        input.extend_from_slice(&1_u32.to_be_bytes());
        input.extend_from_slice(b"a");
        input.extend_from_slice(&1_u32.to_be_bytes());
        // "a" -> 2 (duplicate)
        input.extend_from_slice(&1_u32.to_be_bytes());
        input.extend_from_slice(b"a");
        input.extend_from_slice(&2_u32.to_be_bytes());

        assert!(deserialize::<Map<String, u32>>(&input).is_err());
    }

    #[test]
    fn deserialize_varint() {
        use crate::varint::VarIntEncoding;

        #[derive(Cord, Debug, PartialEq)]
        struct W {
            #[cord(varint)]
            v: u32,
        }

        let input = 1293012_u32.encode_var_vec();
        let decoded = deserialize::<W>(&input).unwrap();
        assert_eq!(decoded.v, 1293012_u32);

        let input = vec![12_u8];
        let decoded = deserialize::<W>(&input).unwrap();
        assert_eq!(decoded.v, 12_u32);
    }

    #[test]
    fn deserialize_varint_non_minimal_fails() {
        #[derive(Cord, Debug, PartialEq)]
        struct W {
            #[cord(varint)]
            v: u8,
        }
        // Value 1 with non-minimal encoding [129, 0]
        let non_minimal: Vec<u8> = vec![129, 0];
        assert!(deserialize::<W>(&non_minimal).is_err());
    }

    #[test]
    fn varint_roundtrip() {
        #[derive(Cord, Debug, PartialEq)]
        struct W {
            #[cord(varint)]
            v: u32,
        }
        let original = W { v: 999999_u32 };
        let bytes = crate::serialize(&original).unwrap();
        let decoded: W = deserialize(&bytes).unwrap();
        assert_eq!(decoded, original);
    }

    #[test]
    fn i128_roundtrip() {
        let original: i128 = -170141183460469231731687303715884105728;
        let bytes = crate::serialize(&original).unwrap();
        assert_eq!(bytes.len(), 16);
        let decoded = deserialize::<i128>(&bytes).unwrap();
        assert_eq!(decoded, original);
    }

    #[test]
    fn u128_roundtrip() {
        let original: u128 = 340282366920938463463374607431768211455;
        let bytes = crate::serialize(&original).unwrap();
        assert_eq!(bytes.len(), 16);
        let decoded = deserialize::<u128>(&bytes).unwrap();
        assert_eq!(decoded, original);
    }

    #[test]
    fn varint_u128_roundtrip() {
        #[derive(Cord, Debug, PartialEq)]
        struct W {
            #[cord(varint)]
            v: u128,
        }
        let original = W { v: u128::MAX };
        let bytes = crate::serialize(&original).unwrap();
        let decoded: W = deserialize(&bytes).unwrap();
        assert_eq!(decoded, original);

        let original = W { v: 0_u128 };
        let bytes = crate::serialize(&original).unwrap();
        assert_eq!(bytes.len(), 1);
        let decoded: W = deserialize(&bytes).unwrap();
        assert_eq!(decoded, original);
    }

    #[test]
    fn varint_i128_roundtrip() {
        #[derive(Cord, Debug, PartialEq)]
        struct W {
            #[cord(varint)]
            v: i128,
        }
        let original = W { v: i128::MIN };
        let bytes = crate::serialize(&original).unwrap();
        let decoded: W = deserialize(&bytes).unwrap();
        assert_eq!(decoded, original);

        let original = W { v: 0_i128 };
        let bytes = crate::serialize(&original).unwrap();
        assert_eq!(bytes.len(), 1);
        let decoded: W = deserialize(&bytes).unwrap();
        assert_eq!(decoded, original);
    }

    #[test]
    fn varint_u128_non_minimal_fails() {
        #[derive(Cord, Debug, PartialEq)]
        struct W {
            #[cord(varint)]
            v: u128,
        }
        // Value 1 encoded non-minimally as [0x81, 0x00]
        let non_minimal: Vec<u8> = vec![0x81, 0x00];
        assert!(deserialize::<W>(&non_minimal).is_err());
    }

    #[test]
    fn i128_in_struct_roundtrip() {
        #[derive(Debug, Serialize, Deserialize, PartialEq)]
        struct Big {
            a: u128,
            b: i128,
            c: u32,
        }

        let original = Big {
            a: u128::MAX,
            b: i128::MIN,
            c: 42,
        };
        let bytes = crate::serialize(&original).unwrap();
        assert_eq!(bytes.len(), 16 + 16 + 4);
        let decoded = deserialize::<Big>(&bytes).unwrap();
        assert_eq!(decoded, original);
    }

    #[test]
    fn deserialize_set_duplicate_elements_fails() {
        // Craft bytes with duplicate "a" entries
        let mut input = Vec::new();
        input.extend_from_slice(&2_u32.to_be_bytes()); // length 2
        input.extend_from_slice(&1_u32.to_be_bytes()); // "a" length
        input.extend_from_slice(b"a");
        input.extend_from_slice(&1_u32.to_be_bytes()); // "a" length again
        input.extend_from_slice(b"a");

        assert!(deserialize::<crate::Set<String>>(&input).is_err());
    }

    #[cfg(feature = "datetime")]
    #[test]
    fn deserialize_datetime_pre_epoch_ok() {
        let pre_epoch: DateTime = chrono::DateTime::parse_from_rfc3339("1969-12-31T23:59:59.000Z")
            .unwrap()
            .with_timezone(&Utc)
            .into();
        let bytes = crate::serialize(&pre_epoch).unwrap();
        let decoded = deserialize::<DateTime>(&bytes).unwrap();
        assert_eq!(decoded, pre_epoch);
    }

    #[cfg(feature = "datetime")]
    #[test]
    fn deserialize_datetime_invalid_nanos_fails() {
        // Craft bytes with nanos >= 1_000_000_000
        let mut input = Vec::new();
        input.extend_from_slice(&0_i64.to_be_bytes());
        input.extend_from_slice(&1_000_000_000_u32.to_be_bytes());
        assert!(deserialize::<DateTime>(&input).is_err());
    }

    #[test]
    fn deserialize_set_unordered_elements_fails() {
        // Craft bytes with "b" before "a" (wrong order)
        let mut input = Vec::new();
        input.extend_from_slice(&2_u32.to_be_bytes()); // length 2
        input.extend_from_slice(&1_u32.to_be_bytes()); // "b" length
        input.extend_from_slice(b"b");
        input.extend_from_slice(&1_u32.to_be_bytes()); // "a" length
        input.extend_from_slice(b"a");

        assert!(deserialize::<crate::Set<String>>(&input).is_err());
    }

    #[test]
    fn roundtrip_len8_string() {
        #[derive(Cord, Debug, PartialEq)]
        struct W {
            #[cord(width = 8)]
            v: String,
        }
        let original = W {
            v: "hello".to_string(),
        };
        let bytes = crate::serialize(&original).unwrap();
        assert_eq!(bytes[0], 5_u8); // u8 length prefix
        let decoded: W = deserialize(&bytes).unwrap();
        assert_eq!(decoded, original);
    }

    #[test]
    fn roundtrip_len16_vec() {
        #[derive(Cord, Debug, PartialEq)]
        struct W {
            #[cord(width = 16)]
            v: Vec<u8>,
        }
        let original = W {
            v: vec![10_u8, 20, 30],
        };
        let bytes = crate::serialize(&original).unwrap();
        let decoded: W = deserialize(&bytes).unwrap();
        assert_eq!(decoded, original);
    }

    #[test]
    fn roundtrip_len64_bytes() {
        #[derive(Cord, Debug, PartialEq)]
        struct W {
            #[cord(width = 64)]
            v: Bytes,
        }
        let original = W {
            v: Bytes::from(vec![1, 2, 3, 4, 5]),
        };
        let bytes = crate::serialize(&original).unwrap();
        let decoded: W = deserialize(&bytes).unwrap();
        assert_eq!(decoded, original);
    }

    #[test]
    fn roundtrip_var8_enum() {
        #[derive(Cord, Debug, PartialEq)]
        struct W {
            #[cord(width = 8)]
            v: Enum,
        }
        let original = W {
            v: Enum::Container(42),
        };
        let bytes = crate::serialize(&original).unwrap();
        let decoded: W = deserialize(&bytes).unwrap();
        assert_eq!(decoded, original);
    }

    #[test]
    fn roundtrip_var16_enum() {
        #[derive(Cord, Debug, PartialEq)]
        struct W {
            #[cord(width = 16)]
            v: Enum,
        }
        let original = W {
            v: Enum::Struct { field: 99 },
        };
        let bytes = crate::serialize(&original).unwrap();
        let decoded: W = deserialize(&bytes).unwrap();
        assert_eq!(decoded, original);
    }

    #[test]
    fn roundtrip_var64_enum() {
        #[derive(Cord, Debug, PartialEq)]
        struct W {
            #[cord(width = 64)]
            v: Enum,
        }
        let original = W {
            v: Enum::TupleContainer(5, 6),
        };
        let bytes = crate::serialize(&original).unwrap();
        let decoded: W = deserialize(&bytes).unwrap();
        assert_eq!(decoded, original);
    }

    #[test]
    fn roundtrip_width_wrappers_in_struct() {
        #[derive(Cord, Debug, PartialEq)]
        struct Packet {
            #[cord(width = 8)]
            kind: Enum,
            #[cord(width = 8)]
            name: String,
            fixed: u32,
        }

        let original = Packet {
            kind: Enum::Container(7),
            name: "hi".to_string(),
            fixed: 99,
        };
        let bytes = crate::serialize(&original).unwrap();
        let decoded: Packet = deserialize(&bytes).unwrap();
        assert_eq!(decoded, original);
    }

    #[cfg(feature = "unicode")]
    #[test]
    fn deserialize_non_nfc_string_fails() {
        // Manually craft bytes for NFD "é" (e + combining acute = 0x65 0xCC 0x81)
        let nfd_bytes = b"caf\x65\xcc\x81"; // "café" in NFD
        let mut input = (nfd_bytes.len() as u32).to_be_bytes().to_vec();
        input.extend_from_slice(nfd_bytes);
        assert!(deserialize::<String>(&input).is_err());
    }

    #[test]
    fn deserialize_nfc_string_ok() {
        // NFC "é" = 0xC3 0xA9
        let nfc_bytes = b"caf\xc3\xa9"; // "café" in NFC
        let mut input = (nfc_bytes.len() as u32).to_be_bytes().to_vec();
        input.extend_from_slice(nfc_bytes);
        assert_eq!(deserialize::<String>(&input).unwrap(), "caf\u{00e9}");
    }

    // Evolving tests

    #[derive(Cord, Debug, PartialEq)]
    enum Status {
        Active,
        Inactive,
    }

    #[test]
    fn roundtrip_evolving32_known() {
        let original: Evolving<Enum> = Evolving::new(Enum::Container(42));
        let bytes = crate::serialize(&original).unwrap();
        let decoded = deserialize::<Evolving<Enum>>(&bytes).unwrap();
        assert_eq!(decoded, original);
        assert!(decoded.is_known());
    }

    #[test]
    fn roundtrip_evolving8_known() {
        #[derive(Cord, Debug, PartialEq)]
        struct W {
            #[cord(evolving = 8)]
            v: crate::Evolving<Enum>,
        }
        let original = W {
            v: crate::Evolving::new(Enum::Unit),
        };
        let bytes = crate::serialize(&original).unwrap();
        let decoded: W = deserialize(&bytes).unwrap();
        assert_eq!(decoded, original);
    }

    #[test]
    fn roundtrip_evolving16_known() {
        #[derive(Cord, Debug, PartialEq)]
        struct W {
            #[cord(evolving = 16)]
            v: crate::Evolving<Enum>,
        }
        let original = W {
            v: crate::Evolving::new(Enum::Struct { field: 99 }),
        };
        let bytes = crate::serialize(&original).unwrap();
        let decoded: W = deserialize(&bytes).unwrap();
        assert_eq!(decoded, original);
    }

    #[test]
    fn evolving8_with_var8_composition() {
        #[derive(Cord, Debug, PartialEq)]
        struct Var8Enum {
            #[cord(width = 8)]
            inner: Enum,
        }
        #[derive(Cord, Debug, PartialEq)]
        struct W {
            #[cord(evolving = 8)]
            v: crate::Evolving<Var8Enum>,
        }
        let original = W {
            v: crate::Evolving::new(Var8Enum {
                inner: Enum::Container(7),
            }),
        };
        let bytes = crate::serialize(&original).unwrap();

        // Wire format: [u8 payload len][u8 variant index][u16 payload]
        assert_eq!(bytes[0], 3); // payload = 1 byte variant + 2 bytes u16
        assert_eq!(bytes[1], 1); // variant index 1 (Container)
        assert_eq!(&bytes[2..4], &7_u16.to_be_bytes());

        let decoded: W = deserialize(&bytes).unwrap();
        assert_eq!(decoded, original);
    }

    #[test]
    fn evolving_unknown_variant() {
        use crate::Evolving;

        // Serialize a 3-variant enum, then deserialize as a 2-variant enum
        // Simulate by serializing Enum::TupleContainer(5, 6) (variant index 2)
        // and deserializing as Evolving<Status> which only has indices 0, 1
        let source = Evolving::new(Enum::TupleContainer(5, 6));
        let bytes = crate::serialize(&source).unwrap();

        let decoded = deserialize::<Evolving<Status>>(&bytes).unwrap();
        assert!(decoded.is_unknown());
        assert!(decoded.known().is_none());
        assert!(decoded.unknown_bytes().is_some());
    }

    #[test]
    fn evolving_unknown_roundtrip() {
        use crate::Evolving;

        // Serialize with more variants, deserialize with fewer, re-serialize
        let source = Evolving::new(Enum::TupleContainer(5, 6));
        let original_bytes = crate::serialize(&source).unwrap();

        let decoded = deserialize::<Evolving<Status>>(&original_bytes).unwrap();
        assert!(decoded.is_unknown());

        // Re-serialize the unknown value — should produce identical bytes
        let reserialized = crate::serialize(&decoded).unwrap();
        assert_eq!(original_bytes, reserialized);
    }

    #[test]
    fn evolving8_unknown_with_var8() {
        #[derive(Cord, Debug, PartialEq)]
        struct Var8Enum {
            #[cord(width = 8)]
            inner: Enum,
        }
        #[derive(Cord, Debug, PartialEq)]
        struct Var8Status {
            #[cord(width = 8)]
            inner: Status,
        }
        #[derive(Cord, Debug, PartialEq)]
        struct Ev8E {
            #[cord(evolving = 8)]
            v: crate::Evolving<Var8Enum>,
        }
        #[derive(Cord, Debug, PartialEq)]
        struct Ev8S {
            #[cord(evolving = 8)]
            v: crate::Evolving<Var8Status>,
        }

        // Serialize Var8<Enum::Struct { field: 99 }> (variant 3) inside Evolving8
        let source = Ev8E {
            v: crate::Evolving::new(Var8Enum {
                inner: Enum::Struct { field: 99 },
            }),
        };
        let original_bytes = crate::serialize(&source).unwrap();

        // Deserialize as Evolving8<Var8<Status>> — variant 3 is unknown
        let decoded: Ev8S = deserialize(&original_bytes).unwrap();
        assert!(decoded.v.is_unknown());

        // Re-serialize — should be identical
        let reserialized = crate::serialize(&decoded).unwrap();
        assert_eq!(original_bytes, reserialized);
    }

    #[test]
    fn evolving_in_struct() {
        #[derive(Cord, Debug, PartialEq)]
        struct Msg {
            id: u32,
            status: Evolving<Enum>,
            name: String,
        }

        let original = Msg {
            id: 1,
            status: Evolving::new(Enum::Container(42)),
            name: "test".to_string(),
        };
        let bytes = crate::serialize(&original).unwrap();
        let decoded: Msg = deserialize(&bytes).unwrap();
        assert_eq!(decoded, original);
    }

    #[test]
    fn evolving_trailing_bytes_in_payload_rejected() {
        // Craft bytes where the payload length is larger than the actual variant data
        let mut bytes = Vec::new();
        // Evolving32: u32 payload length
        bytes.extend_from_slice(&10_u32.to_be_bytes()); // claim 10 bytes
                                                        // But only put a unit variant (4 bytes for variant index 0)
        bytes.extend_from_slice(&0_u32.to_be_bytes());
        // Pad with 6 garbage bytes to fill the claimed length
        bytes.extend_from_slice(&[0xDE; 6]);

        let result = deserialize::<crate::Evolving<Status>>(&bytes);
        // This should fail: variant 0 (Active) is valid, but there are trailing bytes
        assert!(result.is_err());
    }

    #[test]
    fn evolving8_payload_size_guard() {
        // Create a value whose serialization exceeds 255 bytes
        let big_string = "x".repeat(300);

        #[derive(Cord, Debug, PartialEq)]
        enum BigEnum {
            Data(String),
        }

        #[derive(Cord, Debug)]
        struct W {
            #[cord(evolving = 8)]
            v: crate::Evolving<BigEnum>,
        }

        let val = W {
            v: crate::Evolving::new(BigEnum::Data(big_string)),
        };
        let result = crate::serialize(&val);
        assert!(result.is_err());
    }

    #[test]
    fn evolving_unit_variant_known() {
        use crate::Evolving;
        let original = Evolving::new(Enum::Unit);
        let bytes = crate::serialize(&original).unwrap();

        // Wire: [u32 payload len = 4][u32 variant index = 0]
        assert_eq!(&bytes[..4], &4_u32.to_be_bytes());
        assert_eq!(&bytes[4..8], &0_u32.to_be_bytes());
        assert_eq!(bytes.len(), 8);

        let decoded = deserialize::<Evolving<Enum>>(&bytes).unwrap();
        assert_eq!(decoded, original);
    }

    #[cfg(feature = "unicode")]
    #[test]
    fn evolving_non_canonical_payload_is_hard_error() {
        // Craft an Evolving32 payload containing a known variant (0 = Active)
        // but with a non-NFC string in the payload. This should be a hard error,
        // not silently treated as Unknown.
        #[derive(Debug, Serialize, Deserialize, PartialEq)]
        enum StringEnum {
            Named(String),
        }

        // Build payload: variant index 0 + NFD string "café" (non-canonical)
        let nfd_bytes = b"caf\x65\xcc\x81"; // NFD form
        let mut payload = Vec::new();
        payload.extend_from_slice(&0_u32.to_be_bytes()); // variant index 0
        payload.extend_from_slice(&(nfd_bytes.len() as u32).to_be_bytes());
        payload.extend_from_slice(nfd_bytes);

        // Wrap in Evolving32: u32 payload length + payload
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&(payload.len() as u32).to_be_bytes());
        bytes.extend_from_slice(&payload);

        let result = deserialize::<crate::Evolving<StringEnum>>(&bytes);
        assert!(
            result.is_err(),
            "non-canonical payload should be a hard error, not Unknown"
        );
    }

    #[test]
    fn evolving_unknown_variant_still_becomes_unknown() {
        use crate::Evolving;

        // Variant index 5 doesn't exist in Status (only 0 and 1)
        let mut payload = Vec::new();
        payload.extend_from_slice(&5_u32.to_be_bytes()); // unknown variant index

        let mut bytes = Vec::new();
        bytes.extend_from_slice(&(payload.len() as u32).to_be_bytes());
        bytes.extend_from_slice(&payload);

        let decoded = deserialize::<Evolving<Status>>(&bytes).unwrap();
        assert!(decoded.is_unknown());
    }

    #[test]
    fn evolving_truncated_payload_is_hard_error() {
        // Craft an Evolving32 payload with variant 0 (Active, a unit variant)
        // but claim a shorter payload that truncates the variant index.
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&2_u32.to_be_bytes()); // payload length: only 2 bytes
        bytes.extend_from_slice(&[0x00, 0x00]); // truncated variant index

        let result = deserialize::<crate::Evolving<Status>>(&bytes);
        assert!(
            result.is_err(),
            "truncated payload should be a hard error, not Unknown"
        );
    }

    #[cfg(feature = "datetime")]
    #[test]
    fn deserialize_datetime_nanosecond_precision() {
        use chrono::NaiveDateTime;
        let naive =
            NaiveDateTime::parse_from_str("2023-10-05 14:30:00.123456789", "%Y-%m-%d %H:%M:%S%.f")
                .unwrap();
        let dt: DateTime =
            chrono::DateTime::<chrono::Utc>::from_naive_utc_and_offset(naive, chrono::Utc).into();
        let bytes = crate::serialize(&dt).unwrap();
        let decoded = deserialize::<DateTime>(&bytes).unwrap();
        assert_eq!(decoded, dt);
    }

    #[test]
    fn deserialize_depth_limit_nested_options() {
        // Build deeply nested Option<Option<...Option<u8>...>> bytes manually.
        // Each Some is a 0x01 byte prefix. Nesting 200 Options exceeds the default limit of 128.
        let depth = 200_usize;
        let mut bytes = Vec::new();
        for _ in 0..depth {
            bytes.push(1u8); // Some discriminant
        }
        bytes.push(42u8); // the inner u8

        // This type nests 200 Options deep — too much for the default limit
        // We can't express Option^200 as a Rust type, but we can test with a
        // modest nesting that still exceeds a small custom limit.

        // Instead, test with a small struct that we serialize, then deserialize
        // with a very low limit by using the internal deserializer.
        let val = Some(Some(Some(42u8)));
        let encoded = crate::serialize(&val).unwrap();

        // With max_depth=2, three levels of Option nesting should fail
        let mut de = super::CordDeserializer::new(&encoded);
        de.max_depth = 2;
        let result = Option::<Option<Option<u8>>>::deserialize(&mut de);
        assert_eq!(result.unwrap_err(), crate::CordError::DepthLimitExceeded);
    }

    #[test]
    fn deserialize_depth_limit_nested_vec() {
        // Vec<Vec<u8>> with max_depth=1 should fail
        let val: Vec<Vec<u8>> = vec![vec![1, 2]];
        let encoded = crate::serialize(&val).unwrap();

        let mut de = super::CordDeserializer::new(&encoded);
        de.max_depth = 1;
        let result = Vec::<Vec<u8>>::deserialize(&mut de);
        assert_eq!(result.unwrap_err(), crate::CordError::DepthLimitExceeded);
    }

    #[test]
    fn deserialize_depth_limit_nested_struct() {
        #[derive(Debug, Serialize, Deserialize, PartialEq)]
        struct Inner {
            x: u32,
        }
        #[derive(Debug, Serialize, Deserialize, PartialEq)]
        struct Outer {
            inner: Inner,
        }

        let val = Outer {
            inner: Inner { x: 1 },
        };
        let encoded = crate::serialize(&val).unwrap();

        // max_depth=1: Outer(depth=1) -> Inner(depth=2) should fail
        let mut de = super::CordDeserializer::new(&encoded);
        de.max_depth = 1;
        let result = Outer::deserialize(&mut de);
        assert_eq!(result.unwrap_err(), crate::CordError::DepthLimitExceeded);
    }

    #[test]
    fn deserialize_default_depth_allows_normal_structs() {
        // Normal structs with moderate nesting should work fine with default limit
        #[derive(Debug, Serialize, Deserialize, PartialEq)]
        struct Inner {
            value: u32,
        }
        #[derive(Debug, Serialize, Deserialize, PartialEq)]
        struct Middle {
            inner: Vec<Inner>,
        }
        #[derive(Debug, Serialize, Deserialize, PartialEq)]
        struct Outer {
            middle: Option<Middle>,
        }

        let val = Outer {
            middle: Some(Middle {
                inner: vec![Inner { value: 42 }],
            }),
        };
        let encoded = crate::serialize(&val).unwrap();
        let decoded = deserialize::<Outer>(&encoded).unwrap();
        assert_eq!(decoded, val);
    }

    #[cfg(feature = "decimal")]
    #[test]
    fn deserialize_decimal_non_minimal_bigint_leading_zero_fails() {
        use crate::Decimal;
        // Craft bytes for Decimal with non-minimal unscaled encoding:
        // scale=0, unscaled=[0x00, 0x01] (value 1, but should be just [0x01])
        let mut input = Vec::new();
        input.push(0_u8); // scale
                          // Bytes field: u32 length prefix + payload
        input.extend_from_slice(&2_u32.to_be_bytes()); // length 2
        input.extend_from_slice(&[0x00, 0x01]); // non-minimal: leading 0x00
        assert!(deserialize::<Decimal>(&input).is_err());
    }

    #[cfg(feature = "decimal")]
    #[test]
    fn deserialize_decimal_non_minimal_bigint_leading_ff_fails() {
        use crate::Decimal;
        // Craft bytes for Decimal with non-minimal negative encoding:
        // scale=0, unscaled=[0xFF, 0xFF] (value -1, but should be just [0xFF])
        let mut input = Vec::new();
        input.push(0_u8); // scale
        input.extend_from_slice(&2_u32.to_be_bytes());
        input.extend_from_slice(&[0xFF, 0xFF]); // non-minimal: leading 0xFF
        assert!(deserialize::<Decimal>(&input).is_err());
    }

    #[cfg(feature = "decimal")]
    #[test]
    fn deserialize_decimal_minimal_bigint_accepted() {
        use crate::Decimal;
        use num_bigint::BigInt;
        // Minimal encoding of 128: [0x00, 0x80] — the 0x00 IS needed (sign bit)
        let d = Decimal::new(BigInt::from(128), 0);
        let bytes = crate::serialize(&d).unwrap();
        let decoded = deserialize::<Decimal>(&bytes).unwrap();
        assert_eq!(decoded, d);
    }

    #[cfg(feature = "decimal")]
    #[test]
    fn deserialize_decimal_minimal_negative_accepted() {
        use crate::Decimal;
        use num_bigint::BigInt;
        // Minimal encoding of -128: [0x80] — no leading 0xFF needed
        let d = Decimal::new(BigInt::from(-128), 0);
        let bytes = crate::serialize(&d).unwrap();
        let decoded = deserialize::<Decimal>(&bytes).unwrap();
        assert_eq!(decoded, d);
    }

    #[cfg(feature = "decimal")]
    #[test]
    fn deserialize_decimal_minimal_negative_129_accepted() {
        use crate::Decimal;
        use num_bigint::BigInt;
        // -129 requires [0xFF, 0x7F] — the 0xFF IS needed (sign bit)
        let d = Decimal::new(BigInt::from(-129), 0);
        let bytes = crate::serialize(&d).unwrap();
        let decoded = deserialize::<Decimal>(&bytes).unwrap();
        assert_eq!(decoded, d);
    }

    #[cfg(feature = "decimal")]
    #[test]
    fn deserialize_decimal_non_normalized_scale_fails() {
        use crate::Decimal;
        // Craft bytes for scale=2, unscaled=100 (= 1.00, normalizes to scale=0, unscaled=1)
        let mut input = Vec::new();
        input.push(2_u8); // scale = 2
                          // unscaled = 100 = 0x64, minimal encoding is [0x64]
        input.extend_from_slice(&1_u32.to_be_bytes()); // length 1
        input.push(0x64); // 100
        assert!(deserialize::<Decimal>(&input).is_err());
    }

    #[cfg(feature = "decimal")]
    #[test]
    fn deserialize_decimal_non_normalized_zero_scale_fails() {
        use crate::Decimal;
        // Craft bytes for scale=3, unscaled=0 (zero should always have scale=0)
        let mut input = Vec::new();
        input.push(3_u8); // scale = 3
        input.extend_from_slice(&0_u32.to_be_bytes()); // length 0 (empty = zero)
        assert!(deserialize::<Decimal>(&input).is_err());
    }

    #[cfg(feature = "decimal")]
    #[test]
    fn deserialize_decimal_canonical_roundtrip() {
        use crate::Decimal;
        use num_bigint::BigInt;
        // Verify that canonical values round-trip correctly
        let values = vec![
            Decimal::new(BigInt::from(0), 0),
            Decimal::new(BigInt::from(1), 0),
            Decimal::new(BigInt::from(-1), 0),
            Decimal::new(BigInt::from(12345), 3), // 12.345
            Decimal::new(BigInt::from(-99), 1),   // -9.9
        ];
        for d in values {
            let bytes = crate::serialize(&d).unwrap();
            let decoded = deserialize::<Decimal>(&bytes).unwrap();
            assert_eq!(decoded, d);
        }
    }

    #[test]
    fn deserialize_length_limit_string() {
        let val = "hello".to_string();
        let encoded = crate::serialize(&val).unwrap();

        // With max_length=3, a 5-byte string should fail
        let mut de = super::CordDeserializer::new(&encoded);
        de.max_length = 3;
        let result = String::deserialize(&mut de);
        assert!(matches!(
            result.unwrap_err(),
            crate::CordError::LengthLimitExceeded(5, 3)
        ));
    }

    #[test]
    fn deserialize_length_limit_vec() {
        let val: Vec<u8> = vec![1, 2, 3, 4, 5];
        let encoded = crate::serialize(&val).unwrap();

        // With max_length=2, a 5-element vec should fail
        let mut de = super::CordDeserializer::new(&encoded);
        de.max_length = 2;
        let result = Vec::<u8>::deserialize(&mut de);
        assert!(matches!(
            result.unwrap_err(),
            crate::CordError::LengthLimitExceeded(5, 2)
        ));
    }

    #[test]
    fn deserialize_length_limit_allows_within_limit() {
        let val = "hi".to_string();
        let encoded = crate::serialize(&val).unwrap();

        let mut de = super::CordDeserializer::new(&encoded);
        de.max_length = 2;
        let result = String::deserialize(&mut de).unwrap();
        assert_eq!(result, "hi");
    }

    #[test]
    fn deserialize_options_default_works() {
        let encoded = crate::serialize(&42u32).unwrap();
        let result: u32 = super::DeserializeOptions::new()
            .deserialize(&encoded)
            .unwrap();
        assert_eq!(result, 42);
    }

    #[test]
    fn deserialize_options_custom_max_length() {
        let val = "hello".to_string();
        let encoded = crate::serialize(&val).unwrap();

        // Length 5 exceeds max_length of 3
        let opts = super::DeserializeOptions::new().max_length(3);
        let result = opts.deserialize::<String>(&encoded);
        assert!(result.is_err());

        // Length 5 is within max_length of 5
        let opts = super::DeserializeOptions::new().max_length(5);
        let result: String = opts.deserialize(&encoded).unwrap();
        assert_eq!(result, "hello");
    }

    #[test]
    fn deserialize_options_custom_max_depth() {
        // A nested structure: Vec<Vec<u8>>
        let val: Vec<Vec<u8>> = vec![vec![1, 2], vec![3]];
        let encoded = crate::serialize(&val).unwrap();

        // Depth limit of 1 should fail on the nested vec
        let opts = super::DeserializeOptions::new().max_depth(1);
        let result = opts.deserialize::<Vec<Vec<u8>>>(&encoded);
        assert!(result.is_err());

        // Default depth should succeed
        let opts = super::DeserializeOptions::new();
        let result: Vec<Vec<u8>> = opts.deserialize(&encoded).unwrap();
        assert_eq!(result, val);
    }

    #[test]
    fn deserialize_options_builder_chaining() {
        let opts = super::DeserializeOptions::new()
            .max_depth(10)
            .max_length(256);
        let encoded = crate::serialize(&"hi".to_string()).unwrap();
        let result: String = opts.deserialize(&encoded).unwrap();
        assert_eq!(result, "hi");
    }
}
