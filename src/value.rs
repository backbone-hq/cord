//! Serde bridge: convert between Rust types and dynamic [`Value`] without going through bytes.
//!
//! - [`to_value`] serializes any `Serialize` type into a `Value`
//! - [`from_value`] deserializes a `Value` into any `Deserialize` type

#[cfg(feature = "datetime")]
use crate::private::SENTINEL_DATETIME;
#[cfg(feature = "decimal")]
use crate::private::SENTINEL_DECIMAL;
#[cfg(feature = "uuid")]
use crate::private::SENTINEL_UUID;
use crate::private::{is_evolving_known, is_evolving_raw, is_sentinel, SENTINEL_SET};
use crate::result::{CordError, CordResult};
use crate::schema::{Value, VariantValue};
use serde::{de, ser, Deserialize, Serialize};

// ---------------------------------------------------------------------------
// to_value
// ---------------------------------------------------------------------------

/// Convert any `Serialize` type into a [`Value`].
///
/// Cord field attributes (`#[cord(varint)]`, `#[cord(width = 8)]`, etc.) and
/// `Evolving` wrappers are transparent — they are stripped and the inner value
/// is used directly, since they only affect wire encoding, not the semantic value.
pub fn to_value<T: Serialize + ?Sized>(value: &T) -> CordResult<Value> {
    value.serialize(ValueSerializer)
}

struct ValueSerializer;

// `is_sentinel` is now imported from crate::private

/// Encode a Value to its canonical bytes for ordering comparison and fallback.
fn value_to_bytes(value: &Value) -> CordResult<Vec<u8>> {
    crate::dynamic::encode(value, &crate::schema::infer_schema(value))
}

impl ser::Serializer for ValueSerializer {
    type Ok = Value;
    type Error = CordError;
    type SerializeSeq = SerializeVec;
    type SerializeTuple = SerializeTuple;
    type SerializeTupleStruct = SerializeTuple;
    type SerializeTupleVariant = SerializeTupleVariant;
    type SerializeMap = SerializeMap;
    type SerializeStruct = SerializeStruct;
    type SerializeStructVariant = SerializeStructVariant;

    fn serialize_bool(self, v: bool) -> CordResult<Value> {
        Ok(Value::Bool(v))
    }

    fn serialize_i8(self, v: i8) -> CordResult<Value> {
        Ok(Value::I8(v))
    }
    fn serialize_i16(self, v: i16) -> CordResult<Value> {
        Ok(Value::I16(v))
    }
    fn serialize_i32(self, v: i32) -> CordResult<Value> {
        Ok(Value::I32(v))
    }
    fn serialize_i64(self, v: i64) -> CordResult<Value> {
        Ok(Value::I64(v))
    }
    fn serialize_u8(self, v: u8) -> CordResult<Value> {
        Ok(Value::U8(v))
    }
    fn serialize_u16(self, v: u16) -> CordResult<Value> {
        Ok(Value::U16(v))
    }
    fn serialize_u32(self, v: u32) -> CordResult<Value> {
        Ok(Value::U32(v))
    }
    fn serialize_u64(self, v: u64) -> CordResult<Value> {
        Ok(Value::U64(v))
    }
    fn serialize_i128(self, v: i128) -> CordResult<Value> {
        Ok(Value::I128(v))
    }
    fn serialize_u128(self, v: u128) -> CordResult<Value> {
        Ok(Value::U128(v))
    }

    fn serialize_f32(self, v: f32) -> CordResult<Value> {
        Ok(Value::F32(v))
    }
    fn serialize_f64(self, v: f64) -> CordResult<Value> {
        Ok(Value::F64(v))
    }

    fn serialize_char(self, v: char) -> CordResult<Value> {
        let mut buf = [0u8; 4];
        let s = v.encode_utf8(&mut buf);
        self.serialize_str(s)
    }

    fn serialize_str(self, v: &str) -> CordResult<Value> {
        let normalized = crate::wire::normalize_nfc(v);
        Ok(Value::String(normalized.into_owned()))
    }

    fn serialize_bytes(self, v: &[u8]) -> CordResult<Value> {
        Ok(Value::Bytes(v.to_owned()))
    }

    fn serialize_none(self) -> CordResult<Value> {
        Ok(Value::None)
    }

    fn serialize_some<T: ?Sized + Serialize>(self, value: &T) -> CordResult<Value> {
        Ok(Value::Some(Box::new(to_value(value)?)))
    }

    fn serialize_unit(self) -> CordResult<Value> {
        Ok(Value::Unit)
    }

    fn serialize_unit_struct(self, _name: &'static str) -> CordResult<Value> {
        Ok(Value::Unit)
    }

    fn serialize_unit_variant(
        self,
        _name: &'static str,
        variant_index: u32,
        variant: &'static str,
    ) -> CordResult<Value> {
        Ok(Value::Enum {
            variant_index,
            variant_name: variant.to_owned(),
            payload: Box::new(VariantValue::Unit),
        })
    }

    fn serialize_newtype_struct<T: ?Sized + Serialize>(
        self,
        name: &'static str,
        value: &T,
    ) -> CordResult<Value> {
        if name == SENTINEL_SET {
            // Set sentinel: inner serializes as a seq, wrap as Value::Set
            let inner = value.serialize(self)?;
            return match inner {
                Value::Seq(items) => Ok(Value::Set(items)),
                other => Ok(Value::Set(vec![other])),
            };
        }
        #[cfg(feature = "datetime")]
        if name == SENTINEL_DATETIME {
            let inner = value.serialize(self)?;
            return match inner {
                Value::Tuple(ref items) if items.len() == 2 => match (&items[0], &items[1]) {
                    (Value::I64(secs), Value::U32(nanos)) => {
                        chrono::DateTime::from_timestamp(*secs, *nanos)
                            .map(Value::DateTime)
                            .ok_or_else(|| CordError::SerializationError("Invalid DateTime".into()))
                    }
                    _ => Err(CordError::SerializationError(
                        "Invalid DateTime payload".into(),
                    )),
                },
                _ => Err(CordError::SerializationError(
                    "Invalid DateTime payload".into(),
                )),
            };
        }
        #[cfg(feature = "decimal")]
        if name == SENTINEL_DECIMAL {
            let inner = value.serialize(self)?;
            return match inner {
                Value::Tuple(ref items) if items.len() == 2 => match (&items[0], &items[1]) {
                    (Value::U8(scale), Value::Bytes(tc_bytes)) => {
                        let unscaled = num_bigint::BigInt::from_signed_bytes_be(tc_bytes);
                        Ok(Value::Decimal(crate::Decimal::new(unscaled, *scale)))
                    }
                    _ => Err(CordError::SerializationError(
                        "Invalid Decimal payload".into(),
                    )),
                },
                _ => Err(CordError::SerializationError(
                    "Invalid Decimal payload".into(),
                )),
            };
        }
        #[cfg(feature = "uuid")]
        if name == SENTINEL_UUID {
            let inner = value.serialize(self)?;
            return match inner {
                Value::Tuple(ref items) if items.len() == 16 => {
                    let bytes: Vec<u8> = items
                        .iter()
                        .map(|v| match v {
                            Value::U8(b) => Ok(*b),
                            _ => Err(CordError::SerializationError("Invalid Uuid payload".into())),
                        })
                        .collect::<Result<_, _>>()?;
                    let uuid = uuid::Uuid::from_slice(&bytes).map_err(|e| {
                        CordError::SerializationError(format!("Invalid UUID bytes: {e}"))
                    })?;
                    Ok(Value::Uuid(uuid))
                }
                _ => Err(CordError::SerializationError("Invalid Uuid payload".into())),
            };
        }
        if is_evolving_known(name) {
            // Known evolving value — just produce the inner T's value.
            return value.serialize(self);
        }
        if is_evolving_raw(name) {
            // Unknown evolving value — PreSerialized produces a Tuple of U8 values.
            let inner = value.serialize(self)?;
            if let Value::Tuple(items) = inner {
                let bytes: Vec<u8> = items
                    .iter()
                    .map(|v| match v {
                        Value::U8(b) => *b,
                        _ => 0, // PreSerialized always produces U8 elements
                    })
                    .collect();
                return Ok(Value::UnknownEvolving(bytes));
            }
            // Empty PreSerialized → empty unknown
            return Ok(Value::UnknownEvolving(vec![]));
        }
        if is_sentinel(name) {
            // Width/encoding wrappers are transparent in the value domain
            return value.serialize(self);
        }
        // For non-sentinel newtype structs, just serialize the inner value.
        // The serde data model treats newtype structs as transparent wrappers.
        value.serialize(self)
    }

    fn serialize_newtype_variant<T: ?Sized + Serialize>(
        self,
        _name: &'static str,
        variant_index: u32,
        variant: &'static str,
        value: &T,
    ) -> CordResult<Value> {
        Ok(Value::Enum {
            variant_index,
            variant_name: variant.to_owned(),
            payload: Box::new(VariantValue::Newtype(to_value(value)?)),
        })
    }

    fn serialize_seq(self, len: Option<usize>) -> CordResult<Self::SerializeSeq> {
        Ok(SerializeVec {
            items: Vec::with_capacity(len.unwrap_or(0)),
        })
    }

    fn serialize_tuple(self, len: usize) -> CordResult<Self::SerializeTuple> {
        Ok(SerializeTuple {
            items: Vec::with_capacity(len),
        })
    }

    fn serialize_tuple_struct(
        self,
        _name: &'static str,
        len: usize,
    ) -> CordResult<Self::SerializeTupleStruct> {
        if len > 0 {
            return Err(CordError::NotSupported("tuple struct"));
        }
        self.serialize_tuple(len)
    }

    fn serialize_tuple_variant(
        self,
        _name: &'static str,
        variant_index: u32,
        variant: &'static str,
        len: usize,
    ) -> CordResult<Self::SerializeTupleVariant> {
        Ok(SerializeTupleVariant {
            variant_index,
            variant_name: variant.to_owned(),
            items: Vec::with_capacity(len),
        })
    }

    fn serialize_map(self, len: Option<usize>) -> CordResult<Self::SerializeMap> {
        Ok(SerializeMap {
            entries: Vec::with_capacity(len.unwrap_or(0)),
            next_key: None,
        })
    }

    fn serialize_struct(
        self,
        _name: &'static str,
        len: usize,
    ) -> CordResult<Self::SerializeStruct> {
        Ok(SerializeStruct {
            fields: Vec::with_capacity(len),
        })
    }

    fn serialize_struct_variant(
        self,
        _name: &'static str,
        variant_index: u32,
        variant: &'static str,
        len: usize,
    ) -> CordResult<Self::SerializeStructVariant> {
        Ok(SerializeStructVariant {
            variant_index,
            variant_name: variant.to_owned(),
            fields: Vec::with_capacity(len),
        })
    }
}

// --- Compound serializers ---

struct SerializeVec {
    items: Vec<Value>,
}

impl ser::SerializeSeq for SerializeVec {
    type Ok = Value;
    type Error = CordError;

    fn serialize_element<T: ?Sized + Serialize>(&mut self, value: &T) -> CordResult<()> {
        self.items.push(to_value(value)?);
        Ok(())
    }

    fn end(self) -> CordResult<Value> {
        Ok(Value::Seq(self.items))
    }
}

struct SerializeTuple {
    items: Vec<Value>,
}

impl ser::SerializeTuple for SerializeTuple {
    type Ok = Value;
    type Error = CordError;

    fn serialize_element<T: ?Sized + Serialize>(&mut self, value: &T) -> CordResult<()> {
        self.items.push(to_value(value)?);
        Ok(())
    }

    fn end(self) -> CordResult<Value> {
        Ok(Value::Tuple(self.items))
    }
}

impl ser::SerializeTupleStruct for SerializeTuple {
    type Ok = Value;
    type Error = CordError;

    fn serialize_field<T: ?Sized + Serialize>(&mut self, value: &T) -> CordResult<()> {
        self.items.push(to_value(value)?);
        Ok(())
    }

    fn end(self) -> CordResult<Value> {
        Ok(Value::Tuple(self.items))
    }
}

struct SerializeTupleVariant {
    variant_index: u32,
    variant_name: String,
    items: Vec<Value>,
}

impl ser::SerializeTupleVariant for SerializeTupleVariant {
    type Ok = Value;
    type Error = CordError;

    fn serialize_field<T: ?Sized + Serialize>(&mut self, value: &T) -> CordResult<()> {
        self.items.push(to_value(value)?);
        Ok(())
    }

    fn end(self) -> CordResult<Value> {
        Ok(Value::Enum {
            variant_index: self.variant_index,
            variant_name: self.variant_name,
            payload: Box::new(VariantValue::Tuple(self.items)),
        })
    }
}

struct SerializeMap {
    /// (key_value, val_value, serialized_key_bytes)
    entries: Vec<(Value, Value, Vec<u8>)>,
    next_key: Option<(Value, Vec<u8>)>,
}

impl ser::SerializeMap for SerializeMap {
    type Ok = Value;
    type Error = CordError;

    fn serialize_key<T: ?Sized + Serialize>(&mut self, key: &T) -> CordResult<()> {
        // Serialize the key to bytes now, while we still have the original type
        let mut key_bytes = Vec::new();
        crate::ser::serialize_into(&mut key_bytes, key)
            .map_err(|e| CordError::SerializationError(e.to_string()))?;
        self.next_key = Some((to_value(key)?, key_bytes));
        Ok(())
    }

    fn serialize_value<T: ?Sized + Serialize>(&mut self, value: &T) -> CordResult<()> {
        let (key, key_bytes) = self.next_key.take().ok_or_else(|| {
            CordError::SerializationError("serialize_value called before serialize_key".into())
        })?;
        self.entries.push((key, to_value(value)?, key_bytes));
        Ok(())
    }

    fn end(self) -> CordResult<Value> {
        let mut entries = self.entries;
        // Sort by canonical key bytes for consistency with the wire format
        entries.sort_by(|a, b| a.2.cmp(&b.2));
        // Reject duplicates
        for i in 1..entries.len() {
            if entries[i - 1].2 == entries[i].2 {
                return Err(CordError::SerializationError(
                    "Duplicate keys in map".into(),
                ));
            }
        }
        Ok(Value::Map(
            entries.into_iter().map(|(k, v, _)| (k, v)).collect(),
        ))
    }
}

struct SerializeStruct {
    fields: Vec<(String, Value)>,
}

impl ser::SerializeStruct for SerializeStruct {
    type Ok = Value;
    type Error = CordError;

    fn serialize_field<T: ?Sized + Serialize>(
        &mut self,
        key: &'static str,
        value: &T,
    ) -> CordResult<()> {
        self.fields.push((key.to_owned(), to_value(value)?));
        Ok(())
    }

    fn end(self) -> CordResult<Value> {
        Ok(Value::Struct(self.fields))
    }
}

struct SerializeStructVariant {
    variant_index: u32,
    variant_name: String,
    fields: Vec<(String, Value)>,
}

impl ser::SerializeStructVariant for SerializeStructVariant {
    type Ok = Value;
    type Error = CordError;

    fn serialize_field<T: ?Sized + Serialize>(
        &mut self,
        key: &'static str,
        value: &T,
    ) -> CordResult<()> {
        self.fields.push((key.to_owned(), to_value(value)?));
        Ok(())
    }

    fn end(self) -> CordResult<Value> {
        Ok(Value::Enum {
            variant_index: self.variant_index,
            variant_name: self.variant_name,
            payload: Box::new(VariantValue::Struct(self.fields)),
        })
    }
}

// ---------------------------------------------------------------------------
// from_value
// ---------------------------------------------------------------------------

/// Convert a [`Value`] into any `Deserialize` type.
///
/// Cord field attributes (`#[cord(varint)]`, `#[cord(width = 8)]`, etc.) and
/// `Evolving` wrappers are transparent — the deserializer will pass through
/// sentinel newtype struct names without modification, since the `Value` domain
/// has no encoding concerns.
pub fn from_value<T: for<'de> Deserialize<'de>>(value: Value) -> CordResult<T> {
    T::deserialize(ValueDeserializer::new(value))
}

/// A deserializer that deserializes from a [`Value`].
pub struct ValueDeserializer {
    value: Value,
    depth: usize,
    max_depth: usize,
}

impl ValueDeserializer {
    fn new(value: Value) -> Self {
        Self {
            value,
            depth: 0,
            max_depth: crate::de::DEFAULT_MAX_DEPTH,
        }
    }

    /// Return the child depth, checking the depth limit.
    fn child_depth(&self) -> CordResult<usize> {
        let child = self.depth + 1;
        if child > self.max_depth {
            return Err(CordError::DepthLimitExceeded);
        }
        Ok(child)
    }
}

impl<'de> de::Deserializer<'de> for ValueDeserializer {
    type Error = CordError;

    fn deserialize_any<V: de::Visitor<'de>>(self, visitor: V) -> CordResult<V::Value> {
        // Match the static path: deserialize_any treats data as bytes.
        // The static CordDeserializer reads length-prefixed bytes here;
        // we encode the Value to canonical bytes for the same semantics.
        match self.value {
            Value::Bytes(v) => visitor.visit_byte_buf(v),
            other => {
                let bytes = value_to_bytes(&other)?;
                visitor.visit_bytes(&bytes)
            }
        }
    }

    fn deserialize_bool<V: de::Visitor<'de>>(self, visitor: V) -> CordResult<V::Value> {
        match self.value {
            Value::Bool(v) => visitor.visit_bool(v),
            other => Err(CordError::DeserializationError(format!(
                "Expected Bool, got {:?}",
                std::mem::discriminant(&other)
            ))),
        }
    }

    fn deserialize_i8<V: de::Visitor<'de>>(self, visitor: V) -> CordResult<V::Value> {
        match self.value {
            Value::I8(v) => visitor.visit_i8(v),
            other => Err(CordError::DeserializationError(format!(
                "Expected I8, got {:?}",
                std::mem::discriminant(&other)
            ))),
        }
    }

    fn deserialize_i16<V: de::Visitor<'de>>(self, visitor: V) -> CordResult<V::Value> {
        match self.value {
            Value::I16(v) => visitor.visit_i16(v),
            other => Err(CordError::DeserializationError(format!(
                "Expected I16, got {:?}",
                std::mem::discriminant(&other)
            ))),
        }
    }

    fn deserialize_i32<V: de::Visitor<'de>>(self, visitor: V) -> CordResult<V::Value> {
        match self.value {
            Value::I32(v) => visitor.visit_i32(v),
            other => Err(CordError::DeserializationError(format!(
                "Expected I32, got {:?}",
                std::mem::discriminant(&other)
            ))),
        }
    }

    fn deserialize_i64<V: de::Visitor<'de>>(self, visitor: V) -> CordResult<V::Value> {
        match self.value {
            Value::I64(v) => visitor.visit_i64(v),
            other => Err(CordError::DeserializationError(format!(
                "Expected I64, got {:?}",
                std::mem::discriminant(&other)
            ))),
        }
    }

    fn deserialize_i128<V: de::Visitor<'de>>(self, visitor: V) -> CordResult<V::Value> {
        match self.value {
            Value::I128(v) => visitor.visit_i128(v),
            other => Err(CordError::DeserializationError(format!(
                "Expected I128, got {:?}",
                std::mem::discriminant(&other)
            ))),
        }
    }

    fn deserialize_u8<V: de::Visitor<'de>>(self, visitor: V) -> CordResult<V::Value> {
        match self.value {
            Value::U8(v) => visitor.visit_u8(v),
            other => Err(CordError::DeserializationError(format!(
                "Expected U8, got {:?}",
                std::mem::discriminant(&other)
            ))),
        }
    }

    fn deserialize_u16<V: de::Visitor<'de>>(self, visitor: V) -> CordResult<V::Value> {
        match self.value {
            Value::U16(v) => visitor.visit_u16(v),
            other => Err(CordError::DeserializationError(format!(
                "Expected U16, got {:?}",
                std::mem::discriminant(&other)
            ))),
        }
    }

    fn deserialize_u32<V: de::Visitor<'de>>(self, visitor: V) -> CordResult<V::Value> {
        match self.value {
            Value::U32(v) => visitor.visit_u32(v),
            other => Err(CordError::DeserializationError(format!(
                "Expected U32, got {:?}",
                std::mem::discriminant(&other)
            ))),
        }
    }

    fn deserialize_u64<V: de::Visitor<'de>>(self, visitor: V) -> CordResult<V::Value> {
        match self.value {
            Value::U64(v) => visitor.visit_u64(v),
            other => Err(CordError::DeserializationError(format!(
                "Expected U64, got {:?}",
                std::mem::discriminant(&other)
            ))),
        }
    }

    fn deserialize_u128<V: de::Visitor<'de>>(self, visitor: V) -> CordResult<V::Value> {
        match self.value {
            Value::U128(v) => visitor.visit_u128(v),
            other => Err(CordError::DeserializationError(format!(
                "Expected U128, got {:?}",
                std::mem::discriminant(&other)
            ))),
        }
    }

    fn deserialize_f32<V: de::Visitor<'de>>(self, visitor: V) -> CordResult<V::Value> {
        match self.value {
            Value::F32(v) => visitor.visit_f32(v),
            _ => Err(CordError::DeserializationError("expected F32 value".into())),
        }
    }

    fn deserialize_f64<V: de::Visitor<'de>>(self, visitor: V) -> CordResult<V::Value> {
        match self.value {
            Value::F64(v) => visitor.visit_f64(v),
            _ => Err(CordError::DeserializationError("expected F64 value".into())),
        }
    }

    fn deserialize_char<V: de::Visitor<'de>>(self, visitor: V) -> CordResult<V::Value> {
        match self.value {
            Value::String(s) => {
                #[cfg(feature = "unicode")]
                if !s.is_ascii() && !unicode_normalization::is_nfc(&s) {
                    return Err(CordError::ValidationError("String is not NFC normalized"));
                }
                let mut chars = s.chars();
                match (chars.next(), chars.next()) {
                    (Some(c), None) => visitor.visit_char(c),
                    _ => Err(CordError::DeserializationError(
                        "Expected exactly one character".into(),
                    )),
                }
            }
            other => Err(CordError::DeserializationError(format!(
                "Expected char (String), got {:?}",
                std::mem::discriminant(&other)
            ))),
        }
    }

    fn deserialize_str<V: de::Visitor<'de>>(self, visitor: V) -> CordResult<V::Value> {
        match self.value {
            Value::String(v) => {
                #[cfg(feature = "unicode")]
                if !v.is_ascii() && !unicode_normalization::is_nfc(&v) {
                    return Err(CordError::ValidationError("String is not NFC normalized"));
                }
                visitor.visit_string(v)
            }
            other => Err(CordError::DeserializationError(format!(
                "Expected String, got {:?}",
                std::mem::discriminant(&other)
            ))),
        }
    }

    fn deserialize_string<V: de::Visitor<'de>>(self, visitor: V) -> CordResult<V::Value> {
        self.deserialize_str(visitor)
    }

    fn deserialize_bytes<V: de::Visitor<'de>>(self, visitor: V) -> CordResult<V::Value> {
        match self.value {
            Value::Bytes(v) => visitor.visit_byte_buf(v),
            other => Err(CordError::DeserializationError(format!(
                "Expected Bytes, got {:?}",
                std::mem::discriminant(&other)
            ))),
        }
    }

    fn deserialize_byte_buf<V: de::Visitor<'de>>(self, visitor: V) -> CordResult<V::Value> {
        self.deserialize_bytes(visitor)
    }

    fn deserialize_option<V: de::Visitor<'de>>(self, visitor: V) -> CordResult<V::Value> {
        let child_depth = self.child_depth()?;
        let max_depth = self.max_depth;
        match self.value {
            Value::None => visitor.visit_none(),
            Value::Some(inner) => visitor.visit_some(ValueDeserializer {
                value: *inner,
                depth: child_depth,
                max_depth,
            }),
            other => Err(CordError::DeserializationError(format!(
                "Expected None or Some, got {:?}",
                std::mem::discriminant(&other)
            ))),
        }
    }

    fn deserialize_unit<V: de::Visitor<'de>>(self, visitor: V) -> CordResult<V::Value> {
        match self.value {
            Value::Unit => visitor.visit_unit(),
            other => Err(CordError::DeserializationError(format!(
                "Expected Unit, got {:?}",
                std::mem::discriminant(&other)
            ))),
        }
    }

    fn deserialize_unit_struct<V: de::Visitor<'de>>(
        self,
        _name: &'static str,
        visitor: V,
    ) -> CordResult<V::Value> {
        self.deserialize_unit(visitor)
    }

    fn deserialize_newtype_struct<V: de::Visitor<'de>>(
        self,
        name: &'static str,
        visitor: V,
    ) -> CordResult<V::Value> {
        let depth = self.depth;
        let max_depth = self.max_depth;
        let make = |value| ValueDeserializer {
            value,
            depth,
            max_depth,
        };
        #[cfg(feature = "datetime")]
        if name == SENTINEL_DATETIME {
            if let Value::DateTime(dt) = self.value {
                let tuple_value = Value::Tuple(vec![
                    Value::I64(dt.timestamp()),
                    Value::U32(dt.timestamp_subsec_nanos()),
                ]);
                return visitor.visit_newtype_struct(make(tuple_value));
            }
        }
        #[cfg(feature = "decimal")]
        if name == SENTINEL_DECIMAL {
            if let Value::Decimal(ref d) = self.value {
                // Reject non-canonical Decimal values (matching dynamic::decode validation)
                let normalized = crate::Decimal::new(d.unscaled().clone(), d.scale());
                if normalized.scale() != d.scale() || *normalized.unscaled() != *d.unscaled() {
                    return Err(CordError::ValidationError(
                        "Non-canonical Decimal: scale/unscaled not normalized",
                    ));
                }
                let tc_bytes = d.unscaled().to_signed_bytes_be();
                // Reject non-minimal two's-complement encoding
                if tc_bytes.len() >= 2 {
                    let first = tc_bytes[0];
                    let second_high_bit = tc_bytes[1] & 0x80;
                    if first == 0x00 && second_high_bit == 0 {
                        return Err(CordError::ValidationError(
                            "Non-minimal BigInt encoding: redundant leading 0x00",
                        ));
                    }
                    if first == 0xFF && second_high_bit != 0 {
                        return Err(CordError::ValidationError(
                            "Non-minimal BigInt encoding: redundant leading 0xFF",
                        ));
                    }
                }
                let tuple_value = Value::Tuple(vec![Value::U8(d.scale()), Value::Bytes(tc_bytes)]);
                return visitor.visit_newtype_struct(make(tuple_value));
            }
        }
        #[cfg(feature = "uuid")]
        if name == SENTINEL_UUID {
            if let Value::Uuid(u) = self.value {
                return visitor.visit_newtype_struct(make(Value::Bytes(u.as_bytes().to_vec())));
            }
        }
        if name == SENTINEL_SET {
            if let Value::Set(ref items) = self.value {
                return visitor.visit_newtype_struct(make(Value::Seq(items.clone())));
            }
        }
        if is_evolving_known(name) {
            if let Value::UnknownEvolving(bytes) = self.value {
                return visitor.visit_byte_buf(bytes);
            }
            return visitor.visit_newtype_struct(make(self.value));
        }
        if is_evolving_raw(name) {
            if let Value::UnknownEvolving(bytes) = self.value {
                return visitor.visit_byte_buf(bytes);
            }
            return visitor.visit_newtype_struct(make(self.value));
        }
        // Other sentinel names are transparent — just pass through to the inner value
        visitor.visit_newtype_struct(make(self.value))
    }

    fn deserialize_seq<V: de::Visitor<'de>>(self, visitor: V) -> CordResult<V::Value> {
        let child_depth = self.child_depth()?;
        let max_depth = self.max_depth;
        match self.value {
            Value::Seq(items) | Value::Set(items) => visitor.visit_seq(SeqAccess {
                iter: items.into_iter(),
                depth: child_depth,
                max_depth,
            }),
            Value::Tuple(items) => visitor.visit_seq(TupleSeqAccess {
                items: items.into_iter(),
                depth: child_depth,
                max_depth,
            }),
            other => Err(CordError::DeserializationError(format!(
                "Expected Seq, got {:?}",
                std::mem::discriminant(&other)
            ))),
        }
    }

    fn deserialize_tuple<V: de::Visitor<'de>>(
        self,
        _len: usize,
        visitor: V,
    ) -> CordResult<V::Value> {
        let child_depth = self.child_depth()?;
        let max_depth = self.max_depth;
        match self.value {
            Value::Tuple(items) => visitor.visit_seq(TupleSeqAccess {
                items: items.into_iter(),
                depth: child_depth,
                max_depth,
            }),
            Value::Seq(items) => visitor.visit_seq(SeqAccess {
                iter: items.into_iter(),
                depth: child_depth,
                max_depth,
            }),
            other => Err(CordError::DeserializationError(format!(
                "Expected Tuple, got {:?}",
                std::mem::discriminant(&other)
            ))),
        }
    }

    fn deserialize_tuple_struct<V: de::Visitor<'de>>(
        self,
        _name: &'static str,
        len: usize,
        visitor: V,
    ) -> CordResult<V::Value> {
        self.deserialize_tuple(len, visitor)
    }

    fn deserialize_map<V: de::Visitor<'de>>(self, visitor: V) -> CordResult<V::Value> {
        let child_depth = self.child_depth()?;
        let max_depth = self.max_depth;
        match self.value {
            Value::Map(entries) => visitor.visit_map(MapAccess {
                iter: entries.into_iter(),
                pending_value: None,
                previous_key_bytes: None,
                depth: child_depth,
                max_depth,
            }),
            other => Err(CordError::DeserializationError(format!(
                "Expected Map, got {:?}",
                std::mem::discriminant(&other)
            ))),
        }
    }

    fn deserialize_struct<V: de::Visitor<'de>>(
        self,
        _name: &'static str,
        _fields: &'static [&'static str],
        visitor: V,
    ) -> CordResult<V::Value> {
        let child_depth = self.child_depth()?;
        let max_depth = self.max_depth;
        match self.value {
            Value::Struct(fields) => {
                let values: Vec<Value> = fields.into_iter().map(|(_, v)| v).collect();
                visitor.visit_seq(TupleSeqAccess {
                    items: values.into_iter(),
                    depth: child_depth,
                    max_depth,
                })
            }
            Value::Tuple(items) => visitor.visit_seq(TupleSeqAccess {
                items: items.into_iter(),
                depth: child_depth,
                max_depth,
            }),
            other => Err(CordError::DeserializationError(format!(
                "Expected Struct, got {:?}",
                std::mem::discriminant(&other)
            ))),
        }
    }

    fn deserialize_enum<V: de::Visitor<'de>>(
        self,
        _name: &'static str,
        _variants: &'static [&'static str],
        visitor: V,
    ) -> CordResult<V::Value> {
        let child_depth = self.child_depth()?;
        let max_depth = self.max_depth;
        match self.value {
            Value::Enum {
                variant_index,
                variant_name,
                payload,
            } => visitor.visit_enum(EnumAccess {
                variant_index,
                variant_name,
                payload: *payload,
                depth: child_depth,
                max_depth,
            }),
            _ => Err(CordError::DeserializationError(
                "Expected enum Value".into(),
            )),
        }
    }

    fn deserialize_identifier<V: de::Visitor<'de>>(self, visitor: V) -> CordResult<V::Value> {
        // Match the static path: deserialize_identifier delegates to deserialize_bytes.
        match self.value {
            Value::Bytes(v) => visitor.visit_byte_buf(v),
            other => {
                let bytes = value_to_bytes(&other)?;
                visitor.visit_bytes(&bytes)
            }
        }
    }

    fn deserialize_ignored_any<V: de::Visitor<'de>>(self, _visitor: V) -> CordResult<V::Value> {
        Err(CordError::NotSupported("ignored any"))
    }
}

// --- SeqAccess (for Seq/Set) ---

struct SeqAccess {
    iter: std::vec::IntoIter<Value>,
    depth: usize,
    max_depth: usize,
}

impl<'de> de::SeqAccess<'de> for SeqAccess {
    type Error = CordError;

    fn next_element_seed<T: de::DeserializeSeed<'de>>(
        &mut self,
        seed: T,
    ) -> CordResult<Option<T::Value>> {
        match self.iter.next() {
            Some(value) => seed
                .deserialize(ValueDeserializer {
                    value,
                    depth: self.depth,
                    max_depth: self.max_depth,
                })
                .map(Some),
            None => Ok(None),
        }
    }

    fn size_hint(&self) -> Option<usize> {
        Some(self.iter.len())
    }
}

// --- TupleSeqAccess (for Tuple) ---

struct TupleSeqAccess {
    items: std::vec::IntoIter<Value>,
    depth: usize,
    max_depth: usize,
}

impl<'de> de::SeqAccess<'de> for TupleSeqAccess {
    type Error = CordError;

    fn next_element_seed<T: de::DeserializeSeed<'de>>(
        &mut self,
        seed: T,
    ) -> CordResult<Option<T::Value>> {
        match self.items.next() {
            Some(value) => seed
                .deserialize(ValueDeserializer {
                    value,
                    depth: self.depth,
                    max_depth: self.max_depth,
                })
                .map(Some),
            None => Ok(None),
        }
    }

    fn size_hint(&self) -> Option<usize> {
        Some(self.items.len())
    }
}

// --- MapAccess (for Map) ---

struct MapAccess {
    iter: std::vec::IntoIter<(Value, Value)>,
    pending_value: Option<Value>,
    previous_key_bytes: Option<Vec<u8>>,
    depth: usize,
    max_depth: usize,
}

impl<'de> de::MapAccess<'de> for MapAccess {
    type Error = CordError;

    fn next_key_seed<K: de::DeserializeSeed<'de>>(
        &mut self,
        seed: K,
    ) -> CordResult<Option<K::Value>> {
        match self.iter.next() {
            Some((key, value)) => {
                // Validate key ordering — match the static path's MapDeserializer
                let key_bytes = value_to_bytes(&key)?;
                if let Some(ref prev) = self.previous_key_bytes {
                    if key_bytes.as_slice() <= prev.as_slice() {
                        return Err(CordError::ValidationError(
                            "Unordered or duplicate map keys",
                        ));
                    }
                }
                self.previous_key_bytes = Some(key_bytes);
                self.pending_value = Some(value);
                seed.deserialize(ValueDeserializer {
                    value: key,
                    depth: self.depth,
                    max_depth: self.max_depth,
                })
                .map(Some)
            }
            None => Ok(None),
        }
    }

    fn next_value_seed<V: de::DeserializeSeed<'de>>(&mut self, seed: V) -> CordResult<V::Value> {
        let value = self.pending_value.take().ok_or_else(|| {
            CordError::DeserializationError("next_value_seed called before next_key_seed".into())
        })?;
        seed.deserialize(ValueDeserializer {
            value,
            depth: self.depth,
            max_depth: self.max_depth,
        })
    }

    fn size_hint(&self) -> Option<usize> {
        Some(self.iter.len())
    }
}

// --- EnumAccess ---

#[allow(dead_code)]
struct EnumAccess {
    variant_index: u32,
    variant_name: String,
    payload: VariantValue,
    depth: usize,
    max_depth: usize,
}

impl<'de> de::EnumAccess<'de> for EnumAccess {
    type Error = CordError;
    type Variant = VariantAccess;

    fn variant_seed<V: de::DeserializeSeed<'de>>(
        self,
        seed: V,
    ) -> CordResult<(V::Value, Self::Variant)> {
        let val = seed.deserialize(de::value::U32Deserializer::<CordError>::new(
            self.variant_index,
        ))?;
        Ok((
            val,
            VariantAccess {
                payload: self.payload,
                depth: self.depth,
                max_depth: self.max_depth,
            },
        ))
    }
}

struct VariantAccess {
    payload: VariantValue,
    depth: usize,
    max_depth: usize,
}

impl<'de> de::VariantAccess<'de> for VariantAccess {
    type Error = CordError;

    fn unit_variant(self) -> CordResult<()> {
        match self.payload {
            VariantValue::Unit => Ok(()),
            _ => Err(CordError::DeserializationError(
                "Expected unit variant".into(),
            )),
        }
    }

    fn newtype_variant_seed<T: de::DeserializeSeed<'de>>(self, seed: T) -> CordResult<T::Value> {
        match self.payload {
            VariantValue::Newtype(value) => seed.deserialize(ValueDeserializer {
                value,
                depth: self.depth,
                max_depth: self.max_depth,
            }),
            _ => Err(CordError::DeserializationError(
                "Expected newtype variant".into(),
            )),
        }
    }

    fn tuple_variant<V: de::Visitor<'de>>(self, _len: usize, visitor: V) -> CordResult<V::Value> {
        match self.payload {
            VariantValue::Tuple(items) => visitor.visit_seq(TupleSeqAccess {
                items: items.into_iter(),
                depth: self.depth,
                max_depth: self.max_depth,
            }),
            _ => Err(CordError::DeserializationError(
                "Expected tuple variant".into(),
            )),
        }
    }

    fn struct_variant<V: de::Visitor<'de>>(
        self,
        _fields: &'static [&'static str],
        visitor: V,
    ) -> CordResult<V::Value> {
        match self.payload {
            VariantValue::Struct(fields) => {
                let values: Vec<Value> = fields.into_iter().map(|(_, v)| v).collect();
                visitor.visit_seq(TupleSeqAccess {
                    items: values.into_iter(),
                    depth: self.depth,
                    max_depth: self.max_depth,
                })
            }
            _ => Err(CordError::DeserializationError(
                "Expected struct variant".into(),
            )),
        }
    }
}

// --- IntoDeserializer impl for Value (useful for seed-based APIs) ---

impl<'de> de::IntoDeserializer<'de, CordError> for Value {
    type Deserializer = ValueDeserializer;

    fn into_deserializer(self) -> ValueDeserializer {
        ValueDeserializer::new(self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::{Deserialize, Serialize};

    #[test]
    fn roundtrip_primitives() {
        assert_eq!(from_value::<bool>(to_value(&true).unwrap()).unwrap(), true);
        assert_eq!(from_value::<u32>(to_value(&42u32).unwrap()).unwrap(), 42u32);
        assert_eq!(
            from_value::<i64>(to_value(&-100i64).unwrap()).unwrap(),
            -100i64
        );
        assert_eq!(
            from_value::<String>(to_value(&"hello".to_string()).unwrap()).unwrap(),
            "hello"
        );
    }

    #[test]
    fn roundtrip_struct() {
        #[derive(Serialize, Deserialize, Debug, PartialEq)]
        struct User {
            name: String,
            age: u32,
            active: bool,
        }

        let user = User {
            name: "Alice".into(),
            age: 30,
            active: true,
        };

        let value = to_value(&user).unwrap();
        let back: User = from_value(value).unwrap();
        assert_eq!(back, user);
    }

    #[test]
    fn roundtrip_enum() {
        #[derive(Serialize, Deserialize, Debug, PartialEq)]
        enum Status {
            Active,
            Inactive,
            Custom(String),
            Complex { code: u32, msg: String },
            Tuple(u32, u32),
        }

        let cases = vec![
            Status::Active,
            Status::Inactive,
            Status::Custom("test".into()),
            Status::Complex {
                code: 42,
                msg: "err".into(),
            },
            Status::Tuple(1, 2),
        ];

        for case in cases {
            let value = to_value(&case).unwrap();
            let back: Status = from_value(value).unwrap();
            assert_eq!(back, case);
        }
    }

    #[test]
    fn roundtrip_option() {
        let some_val: Option<u32> = Some(42);
        let none_val: Option<u32> = None;

        assert_eq!(
            from_value::<Option<u32>>(to_value(&some_val).unwrap()).unwrap(),
            some_val
        );
        assert_eq!(
            from_value::<Option<u32>>(to_value(&none_val).unwrap()).unwrap(),
            none_val
        );
    }

    #[test]
    fn roundtrip_vec() {
        let v = vec![1u32, 2, 3, 4, 5];
        let value = to_value(&v).unwrap();
        let back: Vec<u32> = from_value(value).unwrap();
        assert_eq!(back, v);
    }

    #[test]
    fn roundtrip_nested() {
        #[derive(Serialize, Deserialize, Debug, PartialEq)]
        struct Inner {
            x: u32,
        }

        #[derive(Serialize, Deserialize, Debug, PartialEq)]
        struct Outer {
            inner: Inner,
            items: Vec<Inner>,
        }

        let val = Outer {
            inner: Inner { x: 1 },
            items: vec![Inner { x: 2 }, Inner { x: 3 }],
        };

        let value = to_value(&val).unwrap();
        let back: Outer = from_value(value).unwrap();
        assert_eq!(back, val);
    }

    #[test]
    fn to_value_produces_expected_shape() {
        #[derive(Serialize)]
        struct Pair {
            a: u32,
            b: String,
        }

        let value = to_value(&Pair {
            a: 1,
            b: "two".into(),
        })
        .unwrap();

        assert_eq!(
            value,
            Value::Struct(vec![
                ("a".into(), Value::U32(1)),
                ("b".into(), Value::String("two".into())),
            ])
        );
    }

    #[test]
    fn varint_wrapper_transparent() {
        use cord::Cord;

        #[derive(Cord, Debug, PartialEq)]
        struct W {
            #[cord(varint)]
            v: u32,
        }

        let val = W { v: 42u32 };
        let value = to_value(&val).unwrap();
        // VarInt is transparent — produces the inner type's value
        assert_eq!(value, Value::Struct(vec![("v".into(), Value::U32(42))]));

        // Round-trip through the wrapper
        let back: W = from_value(value).unwrap();
        assert_eq!(back, W { v: 42 });
    }

    #[test]
    fn len_wrapper_transparent() {
        use cord::Cord;

        #[derive(Cord, Debug, PartialEq)]
        struct W {
            #[cord(width = 8)]
            v: String,
        }

        let val = W {
            v: "hello".to_string(),
        };
        let value = to_value(&val).unwrap();
        assert_eq!(
            value,
            Value::Struct(vec![("v".into(), Value::String("hello".into()))])
        );

        let back: W = from_value(value).unwrap();
        assert_eq!(back.v, "hello");
    }

    #[test]
    fn roundtrip_bytes() {
        use crate::Bytes;

        let val = Bytes::from(vec![1, 2, 3]);
        let value = to_value(&val).unwrap();
        assert_eq!(value, Value::Bytes(vec![1, 2, 3]));

        let back: Bytes = from_value(value).unwrap();
        assert_eq!(back.to_vec(), vec![1, 2, 3]);
    }

    #[cfg(feature = "datetime")]
    #[test]
    fn roundtrip_datetime() {
        use crate::DateTime;
        use chrono::Utc;

        let dt: DateTime = chrono::DateTime::parse_from_rfc3339("2023-10-05T14:30:00.123456789Z")
            .unwrap()
            .with_timezone(&Utc)
            .into();

        let value = to_value(&dt).unwrap();
        // DateTime now produces Value::DateTime via sentinel
        assert!(matches!(value, Value::DateTime(_)));

        let back: DateTime = from_value(value).unwrap();
        assert_eq!(back, dt);
    }

    #[test]
    fn roundtrip_hashset() {
        use std::collections::HashSet;

        // Note: cord::Set uses CordSerializer internally for canonical ordering,
        // so it doesn't round-trip through to_value/from_value. Use HashSet instead.
        let set: HashSet<String> = vec!["a", "b", "c"]
            .into_iter()
            .map(|s| s.to_string())
            .collect();

        let value = to_value(&set).unwrap();
        assert!(matches!(value, Value::Seq(_)));

        let back: HashSet<String> = from_value(value).unwrap();
        assert_eq!(back, set);
    }

    #[test]
    fn roundtrip_map() {
        use crate::Map;

        let map: Map<String, u32> = vec![("a".to_string(), 1u32), ("b".to_string(), 2u32)]
            .into_iter()
            .collect();

        let value = to_value(&map).unwrap();
        assert!(matches!(value, Value::Map(_)));

        let back: Map<String, u32> = from_value(value).unwrap();
        assert_eq!(back, map);
    }

    #[test]
    fn float_to_value() {
        let v = to_value(&1.0f64).unwrap();
        assert_eq!(v, Value::F64(1.0));
        let v = to_value(&2.5f32).unwrap();
        assert_eq!(v, Value::F32(2.5));
    }

    #[test]
    fn float_from_value() {
        let v: f64 = from_value(Value::F64(3.14)).unwrap();
        assert_eq!(v, 3.14);
        let v: f32 = from_value(Value::F32(2.5)).unwrap();
        assert_eq!(v, 2.5);
    }

    #[cfg(feature = "decimal")]
    #[test]
    fn roundtrip_decimal() {
        let d = crate::Decimal::new(num_bigint::BigInt::from(12345), 2);
        let value = to_value(&d).unwrap();
        assert!(matches!(value, Value::Decimal(_)));
        let back: crate::Decimal = from_value(value).unwrap();
        assert_eq!(back, d);
    }

    #[cfg(feature = "uuid")]
    #[test]
    fn roundtrip_uuid() {
        let u = crate::Uuid::from(uuid::Uuid::from_bytes([
            0x55, 0x0e, 0x84, 0x00, 0xe2, 0x9b, 0x41, 0xd4, 0xa7, 0x16, 0x44, 0x66, 0x55, 0x44,
            0x00, 0x00,
        ]));
        let value = to_value(&u).unwrap();
        assert!(matches!(value, Value::Uuid(_)));
        let back: crate::Uuid = from_value(value).unwrap();
        assert_eq!(back, u);
    }

    #[test]
    fn roundtrip_set() {
        use crate::Set;
        let set: Set<String> = vec!["a", "b", "c"]
            .into_iter()
            .map(|s| s.to_string())
            .collect();
        let value = to_value(&set).unwrap();
        assert!(matches!(value, Value::Set(_)));
        let back: Set<String> = from_value(value).unwrap();
        assert_eq!(back, set);
    }

    #[test]
    fn roundtrip_evolving_known() {
        use crate::Evolving;
        let val = Evolving::Known(42u32);
        let value = to_value(&val).unwrap();
        assert_eq!(value, Value::U32(42));
        let back: Evolving<u32> = from_value(value).unwrap();
        assert_eq!(back, Evolving::Known(42));
    }

    #[test]
    fn roundtrip_evolving_unknown() {
        use crate::Evolving;
        let val: Evolving<u32> = Evolving::Unknown(vec![0xDE, 0xAD]);
        let value = to_value(&val).unwrap();
        assert!(matches!(value, Value::UnknownEvolving(_)));
        let back: Evolving<u32> = from_value(value).unwrap();
        assert_eq!(back, Evolving::Unknown(vec![0xDE, 0xAD]));
    }

    #[cfg(feature = "decimal")]
    #[test]
    fn cross_path_decimal() {
        let d = crate::Decimal::new(num_bigint::BigInt::from(-999), 3);
        let bytes_static = crate::serialize(&d).unwrap();
        let value = to_value(&d).unwrap();
        let bytes_from_value =
            crate::serialize(&from_value::<crate::Decimal>(value).unwrap()).unwrap();
        assert_eq!(bytes_static, bytes_from_value);
    }

    #[cfg(feature = "uuid")]
    #[test]
    fn cross_path_uuid() {
        let u = crate::Uuid::from(uuid::Uuid::nil());
        let bytes_static = crate::serialize(&u).unwrap();
        let value = to_value(&u).unwrap();
        let bytes_from_value =
            crate::serialize(&from_value::<crate::Uuid>(value).unwrap()).unwrap();
        assert_eq!(bytes_static, bytes_from_value);
    }

    // --- Tests for Value::Serialize (cord::serialize(&value)) ---

    #[test]
    fn value_serialize_unit() {
        let value = Value::Unit;
        assert_eq!(crate::serialize(&value).unwrap(), Vec::<u8>::new());
    }

    #[test]
    fn value_serialize_bool() {
        assert_eq!(crate::serialize(&Value::Bool(true)).unwrap(), [1]);
        assert_eq!(crate::serialize(&Value::Bool(false)).unwrap(), [0]);
    }

    #[test]
    fn value_serialize_integers() {
        assert_eq!(
            crate::serialize(&Value::U8(42)).unwrap(),
            crate::serialize(&42u8).unwrap()
        );
        assert_eq!(
            crate::serialize(&Value::U16(1000)).unwrap(),
            crate::serialize(&1000u16).unwrap()
        );
        assert_eq!(
            crate::serialize(&Value::U32(123456)).unwrap(),
            crate::serialize(&123456u32).unwrap()
        );
        assert_eq!(
            crate::serialize(&Value::U64(999999)).unwrap(),
            crate::serialize(&999999u64).unwrap()
        );
        assert_eq!(
            crate::serialize(&Value::U128(u128::MAX)).unwrap(),
            crate::serialize(&u128::MAX).unwrap()
        );
        assert_eq!(
            crate::serialize(&Value::I8(-30)).unwrap(),
            crate::serialize(&(-30i8)).unwrap()
        );
        assert_eq!(
            crate::serialize(&Value::I16(-1000)).unwrap(),
            crate::serialize(&(-1000i16)).unwrap()
        );
        assert_eq!(
            crate::serialize(&Value::I32(-123456)).unwrap(),
            crate::serialize(&(-123456i32)).unwrap()
        );
        assert_eq!(
            crate::serialize(&Value::I64(-999999)).unwrap(),
            crate::serialize(&(-999999i64)).unwrap()
        );
        assert_eq!(
            crate::serialize(&Value::I128(i128::MIN)).unwrap(),
            crate::serialize(&i128::MIN).unwrap()
        );
    }

    #[test]
    fn value_serialize_string() {
        let value = Value::String("hello".into());
        assert_eq!(
            crate::serialize(&value).unwrap(),
            crate::serialize("hello").unwrap()
        );
    }

    #[test]
    fn value_serialize_bytes() {
        let value = Value::Bytes(vec![1, 2, 3]);
        let typed = crate::Bytes::from(vec![1, 2, 3]);
        assert_eq!(
            crate::serialize(&value).unwrap(),
            crate::serialize(&typed).unwrap()
        );
    }

    #[test]
    fn value_serialize_option() {
        let some_val = Value::Some(Box::new(Value::U32(42)));
        assert_eq!(
            crate::serialize(&some_val).unwrap(),
            crate::serialize(&Some(42u32)).unwrap()
        );

        let none_val = Value::None;
        let typed_none: Option<u32> = None;
        assert_eq!(
            crate::serialize(&none_val).unwrap(),
            crate::serialize(&typed_none).unwrap()
        );
    }

    #[test]
    fn value_serialize_seq() {
        let value = Value::Seq(vec![Value::U32(1), Value::U32(2), Value::U32(3)]);
        let typed: Vec<u32> = vec![1, 2, 3];
        assert_eq!(
            crate::serialize(&value).unwrap(),
            crate::serialize(&typed).unwrap()
        );
    }

    #[test]
    fn value_serialize_tuple() {
        let value = Value::Tuple(vec![Value::U16(10), Value::U16(20)]);
        let typed: (u16, u16) = (10, 20);
        assert_eq!(
            crate::serialize(&value).unwrap(),
            crate::serialize(&typed).unwrap()
        );
    }

    #[test]
    fn value_serialize_struct() {
        #[derive(Serialize)]
        struct Pair {
            a: u32,
            b: String,
        }

        let value = Value::Struct(vec![
            ("a".into(), Value::U32(1)),
            ("b".into(), Value::String("two".into())),
        ]);
        let typed = Pair {
            a: 1,
            b: "two".into(),
        };
        assert_eq!(
            crate::serialize(&value).unwrap(),
            crate::serialize(&typed).unwrap()
        );
    }

    #[test]
    fn value_serialize_enum_unit() {
        let value = Value::Enum {
            variant_index: 0,
            variant_name: "Unit".into(),
            payload: Box::new(VariantValue::Unit),
        };
        // Unit variant = just the variant index as u32 BE
        assert_eq!(
            crate::serialize(&value).unwrap(),
            0u32.to_be_bytes().to_vec()
        );
    }

    #[test]
    fn value_serialize_enum_newtype() {
        let value = Value::Enum {
            variant_index: 1,
            variant_name: "Container".into(),
            payload: Box::new(VariantValue::Newtype(Value::U16(7))),
        };
        let mut expected = 1u32.to_be_bytes().to_vec();
        expected.extend_from_slice(&7u16.to_be_bytes());
        assert_eq!(crate::serialize(&value).unwrap(), expected);
    }

    #[test]
    fn value_serialize_enum_tuple() {
        let value = Value::Enum {
            variant_index: 2,
            variant_name: "Tup".into(),
            payload: Box::new(VariantValue::Tuple(vec![Value::U16(5), Value::U16(6)])),
        };
        let mut expected = 2u32.to_be_bytes().to_vec();
        expected.extend_from_slice(&5u16.to_be_bytes());
        expected.extend_from_slice(&6u16.to_be_bytes());
        assert_eq!(crate::serialize(&value).unwrap(), expected);
    }

    #[test]
    fn value_serialize_enum_struct() {
        let value = Value::Enum {
            variant_index: 3,
            variant_name: "S".into(),
            payload: Box::new(VariantValue::Struct(vec![("field".into(), Value::U32(42))])),
        };
        let mut expected = 3u32.to_be_bytes().to_vec();
        expected.extend_from_slice(&42u32.to_be_bytes());
        assert_eq!(crate::serialize(&value).unwrap(), expected);
    }

    #[test]
    fn value_serialize_map() {
        let value = Value::Map(vec![
            (Value::String("a".into()), Value::U32(1)),
            (Value::String("b".into()), Value::U32(2)),
        ]);

        let mut inner = std::collections::HashMap::new();
        inner.insert("a".to_string(), 1u32);
        inner.insert("b".to_string(), 2u32);
        let typed = crate::Map::from(inner);

        assert_eq!(
            crate::serialize(&value).unwrap(),
            crate::serialize(&typed).unwrap()
        );
    }

    #[cfg(feature = "datetime")]
    #[test]
    fn value_serialize_datetime() {
        use chrono::Utc;
        let dt = chrono::DateTime::parse_from_rfc3339("2023-10-05T14:30:00.123456789Z")
            .unwrap()
            .with_timezone(&Utc);
        let typed = crate::DateTime::from(dt);
        let value = Value::DateTime(dt);
        assert_eq!(
            crate::serialize(&value).unwrap(),
            crate::serialize(&typed).unwrap()
        );
    }

    #[cfg(feature = "decimal")]
    #[test]
    fn value_serialize_decimal() {
        let d = crate::Decimal::new(num_bigint::BigInt::from(12345), 2);
        let value = Value::Decimal(d.clone());
        assert_eq!(
            crate::serialize(&value).unwrap(),
            crate::serialize(&d).unwrap()
        );
    }

    #[cfg(feature = "uuid")]
    #[test]
    fn value_serialize_uuid() {
        let u = uuid::Uuid::parse_str("550e8400-e29b-41d4-a716-446655440000").unwrap();
        let typed = crate::Uuid::from(u);
        let value = Value::Uuid(u);
        assert_eq!(
            crate::serialize(&value).unwrap(),
            crate::serialize(&typed).unwrap()
        );
    }

    #[test]
    fn value_serialize_matches_typed_roundtrip() {
        // Verify that to_value -> serialize(&value) matches direct serialize
        #[derive(Serialize)]
        struct Record {
            name: String,
            age: u32,
            active: bool,
        }

        let typed = Record {
            name: "Alice".into(),
            age: 30,
            active: true,
        };
        let direct_bytes = crate::serialize(&typed).unwrap();
        let value = to_value(&typed).unwrap();
        let value_bytes = crate::serialize(&value).unwrap();
        assert_eq!(direct_bytes, value_bytes);
    }

    #[test]
    fn value_serialize_nested_struct() {
        #[derive(Serialize)]
        struct Inner {
            x: u32,
        }
        #[derive(Serialize)]
        struct Outer {
            inner: Inner,
            items: Vec<u32>,
        }

        let typed = Outer {
            inner: Inner { x: 99 },
            items: vec![1, 2, 3],
        };
        let direct_bytes = crate::serialize(&typed).unwrap();
        let value = to_value(&typed).unwrap();
        let value_bytes = crate::serialize(&value).unwrap();
        assert_eq!(direct_bytes, value_bytes);
    }
}
