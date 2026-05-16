use crate::result::{CordError, CordResult};
use crate::schema::{Schema, Value, VariantSchema, VariantValue};
use crate::wire;

/// Encode a dynamic Value according to a Schema, producing canonical bytes.
pub fn encode(value: &Value, schema: &Schema) -> CordResult<Vec<u8>> {
    let mut buf = Vec::new();
    encode_value(&mut buf, value, schema)?;
    Ok(buf)
}

/// Decode canonical bytes into a dynamic Value according to a Schema.
pub fn decode(schema: &Schema, bytes: &[u8]) -> CordResult<Value> {
    let mut input = bytes;
    let value = decode_value(
        &mut input,
        schema,
        0,
        crate::de::DEFAULT_MAX_DEPTH,
        crate::de::DEFAULT_MAX_LENGTH,
    )?;
    if !input.is_empty() {
        return Err(CordError::TrailingBytes);
    }
    Ok(value)
}

// --- Helpers (delegating to wire module) ---

macro_rules! write_varint_value {
    ($buf:expr, $val:expr, $ty:ty) => {{
        let v = $val as $ty;
        wire::write_varint($buf, v);
        Ok(())
    }};
}

macro_rules! read_varint_value {
    ($input:expr, $ty:ty, $variant:ident) => {{
        let value: $ty = wire::read_varint($input)?;
        Ok(Value::$variant(value))
    }};
}

// --- Encode ---

fn encode_value(buf: &mut Vec<u8>, value: &Value, schema: &Schema) -> CordResult<()> {
    match (value, schema) {
        (Value::Unit, Schema::Unit) => Ok(()),

        (Value::Bool(v), Schema::Bool) => {
            wire::write_bool(buf, *v);
            Ok(())
        }

        // Fixed-width integers
        (Value::U8(v), Schema::U8) => {
            buf.push(*v);
            Ok(())
        }
        (Value::U16(v), Schema::U16) => {
            buf.extend_from_slice(&v.to_be_bytes());
            Ok(())
        }
        (Value::U32(v), Schema::U32) => {
            buf.extend_from_slice(&v.to_be_bytes());
            Ok(())
        }
        (Value::U64(v), Schema::U64) => {
            buf.extend_from_slice(&v.to_be_bytes());
            Ok(())
        }
        (Value::U128(v), Schema::U128) => {
            buf.extend_from_slice(&v.to_be_bytes());
            Ok(())
        }
        (Value::I8(v), Schema::I8) => {
            buf.extend_from_slice(&v.to_be_bytes());
            Ok(())
        }
        (Value::I16(v), Schema::I16) => {
            buf.extend_from_slice(&v.to_be_bytes());
            Ok(())
        }
        (Value::I32(v), Schema::I32) => {
            buf.extend_from_slice(&v.to_be_bytes());
            Ok(())
        }
        (Value::I64(v), Schema::I64) => {
            buf.extend_from_slice(&v.to_be_bytes());
            Ok(())
        }
        (Value::I128(v), Schema::I128) => {
            buf.extend_from_slice(&v.to_be_bytes());
            Ok(())
        }

        // Floats (canonicalized: NaN rejected, -0.0 → +0.0)
        (Value::F32(v), Schema::F32) => {
            if v.is_nan() {
                return Err(CordError::NanNotAllowed);
            }
            let v = if *v == 0.0 { 0.0_f32 } else { *v };
            buf.extend_from_slice(&v.to_be_bytes());
            Ok(())
        }
        (Value::F64(v), Schema::F64) => {
            if v.is_nan() {
                return Err(CordError::NanNotAllowed);
            }
            let v = if *v == 0.0 { 0.0_f64 } else { *v };
            buf.extend_from_slice(&v.to_be_bytes());
            Ok(())
        }

        // VarInt
        (Value::U8(v), Schema::VarInt(inner)) if **inner == Schema::U8 => {
            write_varint_value!(buf, *v, u8)
        }
        (Value::U16(v), Schema::VarInt(inner)) if **inner == Schema::U16 => {
            write_varint_value!(buf, *v, u16)
        }
        (Value::U32(v), Schema::VarInt(inner)) if **inner == Schema::U32 => {
            write_varint_value!(buf, *v, u32)
        }
        (Value::U64(v), Schema::VarInt(inner)) if **inner == Schema::U64 => {
            write_varint_value!(buf, *v, u64)
        }
        (Value::U128(v), Schema::VarInt(inner)) if **inner == Schema::U128 => {
            write_varint_value!(buf, *v, u128)
        }
        (Value::I8(v), Schema::VarInt(inner)) if **inner == Schema::I8 => {
            write_varint_value!(buf, *v, i8)
        }
        (Value::I16(v), Schema::VarInt(inner)) if **inner == Schema::I16 => {
            write_varint_value!(buf, *v, i16)
        }
        (Value::I32(v), Schema::VarInt(inner)) if **inner == Schema::I32 => {
            write_varint_value!(buf, *v, i32)
        }
        (Value::I64(v), Schema::VarInt(inner)) if **inner == Schema::I64 => {
            write_varint_value!(buf, *v, i64)
        }
        (Value::I128(v), Schema::VarInt(inner)) if **inner == Schema::I128 => {
            write_varint_value!(buf, *v, i128)
        }

        // String
        (Value::String(s), Schema::String(width)) => wire::write_str(buf, s, *width),

        // Bytes
        (Value::Bytes(b), Schema::Bytes(width)) => wire::write_bytes(buf, b, *width),

        // DateTime
        #[cfg(feature = "datetime")]
        (Value::DateTime(dt), Schema::DateTime) => {
            let nanos = dt.timestamp_subsec_nanos();
            if nanos >= 1_000_000_000 {
                return Err(CordError::SchemaError(
                    "DateTime nanos must be < 1_000_000_000".into(),
                ));
            }
            buf.extend_from_slice(&dt.timestamp().to_be_bytes());
            buf.extend_from_slice(&nanos.to_be_bytes());
            Ok(())
        }
        #[cfg(not(feature = "datetime"))]
        (Value::DateTime(_), Schema::DateTime) => {
            Err(CordError::NotSupported("datetime (feature not enabled)"))
        }

        // Decimal: [u8 scale][length-prefixed tc_bytes]
        #[cfg(feature = "decimal")]
        (Value::Decimal(d), Schema::Decimal(width)) => {
            buf.push(d.scale());
            let tc_bytes = d.unscaled().to_signed_bytes_be();
            wire::write_bytes(buf, &tc_bytes, *width)
        }
        #[cfg(not(feature = "decimal"))]
        (Value::Decimal(_), Schema::Decimal(_)) => {
            Err(CordError::NotSupported("decimal (feature not enabled)"))
        }

        // Uuid: exactly 16 raw bytes (no length prefix)
        #[cfg(feature = "uuid")]
        (Value::Uuid(u), Schema::Uuid) => {
            buf.extend_from_slice(u.as_bytes());
            Ok(())
        }
        #[cfg(not(feature = "uuid"))]
        (Value::Uuid(_), Schema::Uuid) => {
            Err(CordError::NotSupported("uuid (feature not enabled)"))
        }

        // Option
        (Value::None, Schema::Option(_)) => {
            buf.push(0);
            Ok(())
        }
        (Value::Some(inner_val), Schema::Option(inner_schema)) => {
            buf.push(1);
            encode_value(buf, inner_val, inner_schema)
        }

        // Seq
        (Value::Seq(items), Schema::Seq(elem_schema, width)) => {
            wire::write_length(buf, items.len(), *width)?;
            for item in items {
                encode_value(buf, item, elem_schema)?;
            }
            Ok(())
        }

        // Tuple
        (Value::Tuple(items), Schema::Tuple(schemas)) => {
            if items.len() != schemas.len() {
                return Err(CordError::SchemaError(format!(
                    "Tuple length mismatch: value has {} elements, schema has {}",
                    items.len(),
                    schemas.len()
                )));
            }
            for (item, schema) in items.iter().zip(schemas.iter()) {
                encode_value(buf, item, schema)?;
            }
            Ok(())
        }

        // Struct
        (Value::Struct(fields), Schema::Struct(field_schemas)) => {
            if fields.len() != field_schemas.len() {
                return Err(CordError::SchemaError(format!(
                    "Struct field count mismatch: value has {} fields, schema has {}",
                    fields.len(),
                    field_schemas.len()
                )));
            }
            for ((name, val), (schema_name, schema)) in fields.iter().zip(field_schemas.iter()) {
                if name != schema_name {
                    return Err(CordError::SchemaError(format!(
                        "Struct field name mismatch: expected '{}', got '{}'",
                        schema_name, name
                    )));
                }
                encode_value(buf, val, schema)?;
            }
            Ok(())
        }

        // Enum
        (
            Value::Enum {
                variant_index,
                variant_name,
                payload,
            },
            Schema::Enum(variants, width),
        ) => {
            let idx = *variant_index as usize;
            if idx >= variants.len() {
                return Err(CordError::SchemaError(format!(
                    "Enum variant index {} out of range (max {})",
                    idx,
                    variants.len() - 1
                )));
            }
            let (expected_name, variant_schema) = &variants[idx];
            if variant_name != expected_name {
                return Err(CordError::SchemaError(format!(
                    "Enum variant name mismatch: expected '{}', got '{}'",
                    expected_name, variant_name
                )));
            }
            wire::write_variant_index(buf, *variant_index, *width)?;
            encode_variant(buf, payload, variant_schema)
        }

        // Map — sort by key bytes, reject duplicates
        (Value::Map(entries), Schema::Map(key_schema, val_schema, width)) => {
            let mut shared_buf = Vec::new();
            let mut ranges: Vec<(usize, usize, usize)> = Vec::with_capacity(entries.len());
            for (k, v) in entries {
                let key_start = shared_buf.len();
                encode_value(&mut shared_buf, k, key_schema)?;
                let key_end = shared_buf.len();
                encode_value(&mut shared_buf, v, val_schema)?;
                ranges.push((key_start, key_end, shared_buf.len()));
            }

            wire::sort_and_dedup_map(&shared_buf, &mut ranges)?;

            wire::write_length(buf, ranges.len(), *width)?;
            for (key_start, _key_end, val_end) in &ranges {
                buf.extend_from_slice(&shared_buf[*key_start..*val_end]);
            }
            Ok(())
        }

        // Set — sort by element bytes, reject duplicates
        (Value::Set(items), Schema::Set(elem_schema, width)) => {
            let mut shared_buf = Vec::new();
            let mut ranges: Vec<(usize, usize)> = Vec::with_capacity(items.len());
            for item in items {
                let start = shared_buf.len();
                encode_value(&mut shared_buf, item, elem_schema)?;
                ranges.push((start, shared_buf.len()));
            }

            wire::sort_and_dedup_set(&shared_buf, &mut ranges)?;

            wire::write_length(buf, ranges.len(), *width)?;
            for (start, end) in &ranges {
                buf.extend_from_slice(&shared_buf[*start..*end]);
            }
            Ok(())
        }

        // Evolving wrapper
        (Value::UnknownEvolving(raw), Schema::Evolving(_, width)) => {
            wire::write_length(buf, raw.len(), *width)?;
            buf.extend_from_slice(raw);
            Ok(())
        }
        (_, Schema::Evolving(inner, width)) => {
            let mut tmp = Vec::new();
            encode_value(&mut tmp, value, inner)?;
            wire::write_length(buf, tmp.len(), *width)?;
            buf.extend_from_slice(&tmp);
            Ok(())
        }

        _ => Err(CordError::SchemaError(format!(
            "Value/Schema mismatch: {:?} vs {:?}",
            std::mem::discriminant(value),
            std::mem::discriminant(schema)
        ))),
    }
}

fn encode_variant(
    buf: &mut Vec<u8>,
    payload: &VariantValue,
    schema: &VariantSchema,
) -> CordResult<()> {
    match (payload, schema) {
        (VariantValue::Unit, VariantSchema::Unit) => Ok(()),
        (VariantValue::Newtype(val), VariantSchema::Newtype(s)) => encode_value(buf, val, s),
        (VariantValue::Tuple(items), VariantSchema::Tuple(schemas)) => {
            if items.len() != schemas.len() {
                return Err(CordError::SchemaError(
                    "Tuple variant length mismatch".into(),
                ));
            }
            for (item, schema) in items.iter().zip(schemas.iter()) {
                encode_value(buf, item, schema)?;
            }
            Ok(())
        }
        (VariantValue::Struct(fields), VariantSchema::Struct(field_schemas)) => {
            if fields.len() != field_schemas.len() {
                return Err(CordError::SchemaError(
                    "Struct variant field count mismatch".into(),
                ));
            }
            for ((name, val), (schema_name, schema)) in fields.iter().zip(field_schemas.iter()) {
                if name != schema_name {
                    return Err(CordError::SchemaError(format!(
                        "Struct variant field name mismatch: expected '{}', got '{}'",
                        schema_name, name
                    )));
                }
                encode_value(buf, val, schema)?;
            }
            Ok(())
        }
        _ => Err(CordError::SchemaError(
            "Variant value/schema mismatch".into(),
        )),
    }
}

// --- Decode ---

fn decode_value(
    input: &mut &[u8],
    schema: &Schema,
    depth: usize,
    max_depth: usize,
    max_length: usize,
) -> CordResult<Value> {
    if depth > max_depth {
        return Err(CordError::DepthLimitExceeded);
    }
    match schema {
        Schema::Unit => Ok(Value::Unit),

        Schema::Bool => Ok(Value::Bool(wire::read_bool(input)?)),

        Schema::U8 => {
            let b = wire::read_bytes(input, 1)?;
            Ok(Value::U8(b[0]))
        }
        Schema::U16 => {
            let b = wire::read_bytes(input, 2)?;
            Ok(Value::U16(u16::from_be_bytes(b.try_into().unwrap())))
        }
        Schema::U32 => {
            let b = wire::read_bytes(input, 4)?;
            Ok(Value::U32(u32::from_be_bytes(b.try_into().unwrap())))
        }
        Schema::U64 => {
            let b = wire::read_bytes(input, 8)?;
            Ok(Value::U64(u64::from_be_bytes(b.try_into().unwrap())))
        }
        Schema::U128 => {
            let b = wire::read_bytes(input, 16)?;
            Ok(Value::U128(u128::from_be_bytes(b.try_into().unwrap())))
        }
        Schema::I8 => {
            let b = wire::read_bytes(input, 1)?;
            Ok(Value::I8(i8::from_be_bytes(b.try_into().unwrap())))
        }
        Schema::I16 => {
            let b = wire::read_bytes(input, 2)?;
            Ok(Value::I16(i16::from_be_bytes(b.try_into().unwrap())))
        }
        Schema::I32 => {
            let b = wire::read_bytes(input, 4)?;
            Ok(Value::I32(i32::from_be_bytes(b.try_into().unwrap())))
        }
        Schema::I64 => {
            let b = wire::read_bytes(input, 8)?;
            Ok(Value::I64(i64::from_be_bytes(b.try_into().unwrap())))
        }
        Schema::I128 => {
            let b = wire::read_bytes(input, 16)?;
            Ok(Value::I128(i128::from_be_bytes(b.try_into().unwrap())))
        }

        Schema::F32 => {
            let b = wire::read_bytes(input, 4)?;
            let v = f32::from_be_bytes(b.try_into().unwrap());
            if v.is_nan() {
                return Err(CordError::NanNotAllowed);
            }
            if v.to_bits() == (-0.0_f32).to_bits() {
                return Err(CordError::NegativeZeroNotAllowed);
            }
            Ok(Value::F32(v))
        }
        Schema::F64 => {
            let b = wire::read_bytes(input, 8)?;
            let v = f64::from_be_bytes(b.try_into().unwrap());
            if v.is_nan() {
                return Err(CordError::NanNotAllowed);
            }
            if v.to_bits() == (-0.0_f64).to_bits() {
                return Err(CordError::NegativeZeroNotAllowed);
            }
            Ok(Value::F64(v))
        }

        Schema::VarInt(inner) => match inner.as_ref() {
            Schema::U8 => read_varint_value!(input, u8, U8),
            Schema::U16 => read_varint_value!(input, u16, U16),
            Schema::U32 => read_varint_value!(input, u32, U32),
            Schema::U64 => read_varint_value!(input, u64, U64),
            Schema::U128 => read_varint_value!(input, u128, U128),
            Schema::I8 => read_varint_value!(input, i8, I8),
            Schema::I16 => read_varint_value!(input, i16, I16),
            Schema::I32 => read_varint_value!(input, i32, I32),
            Schema::I64 => read_varint_value!(input, i64, I64),
            Schema::I128 => read_varint_value!(input, i128, I128),
            _ => Err(CordError::SchemaError(
                "VarInt must wrap an integer schema".into(),
            )),
        },

        Schema::String(width) => {
            let s = wire::read_str(input, *width, max_length)?;
            Ok(Value::String(s.to_string()))
        }

        Schema::Bytes(width) => {
            let b = wire::read_bytes_prefixed(input, *width, max_length)?;
            Ok(Value::Bytes(b.to_vec()))
        }

        #[cfg(feature = "datetime")]
        Schema::DateTime => {
            let secs_bytes = wire::read_bytes(input, 8)?;
            let nanos_bytes = wire::read_bytes(input, 4)?;
            let secs = i64::from_be_bytes(secs_bytes.try_into().unwrap());
            let nanos = u32::from_be_bytes(nanos_bytes.try_into().unwrap());
            if nanos >= 1_000_000_000 {
                return Err(CordError::SchemaError(
                    "DateTime nanos must be < 1_000_000_000".into(),
                ));
            }
            let dt = chrono::DateTime::from_timestamp(secs, nanos)
                .ok_or_else(|| CordError::SchemaError("Invalid DateTime timestamp".into()))?;
            Ok(Value::DateTime(dt))
        }
        #[cfg(not(feature = "datetime"))]
        Schema::DateTime => Err(CordError::NotSupported("datetime (feature not enabled)")),

        #[cfg(feature = "decimal")]
        Schema::Decimal(width) => {
            let scale = wire::read_bytes(input, 1)?[0];
            let tc_bytes = wire::read_bytes_prefixed(input, *width, max_length)?;

            // Reject non-minimal two's-complement encoding
            if tc_bytes.is_empty() {
                return Err(CordError::ValidationError(
                    "Non-minimal BigInt encoding: zero requires at least one byte",
                ));
            }
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

            let unscaled = num_bigint::BigInt::from_signed_bytes_be(tc_bytes);

            // Reject non-normalized scale/unscaled pairs
            let normalized = crate::Decimal::new(unscaled.clone(), scale);
            if normalized.scale() != scale || *normalized.unscaled() != unscaled {
                return Err(CordError::ValidationError(
                    "Non-canonical Decimal: scale/unscaled not normalized",
                ));
            }

            Ok(Value::Decimal(normalized))
        }
        #[cfg(not(feature = "decimal"))]
        Schema::Decimal(_) => Err(CordError::NotSupported("decimal (feature not enabled)")),

        #[cfg(feature = "uuid")]
        Schema::Uuid => {
            let b = wire::read_bytes(input, 16)?;
            let uuid = uuid::Uuid::from_slice(b)
                .map_err(|_| CordError::ValidationError("Invalid UUID bytes"))?;
            Ok(Value::Uuid(uuid))
        }
        #[cfg(not(feature = "uuid"))]
        Schema::Uuid => Err(CordError::NotSupported("uuid (feature not enabled)")),

        Schema::Option(inner) => {
            let disc = wire::read_bytes(input, 1)?[0];
            match disc {
                0 => Ok(Value::None),
                1 => {
                    let val = decode_value(input, inner, depth + 1, max_depth, max_length)?;
                    Ok(Value::Some(Box::new(val)))
                }
                _ => Err(CordError::ValidationError("Invalid option discriminant")),
            }
        }

        Schema::Seq(elem, width) => {
            let count = wire::read_length(input, *width, max_length)?;
            let mut items = Vec::with_capacity(count);
            for _ in 0..count {
                items.push(decode_value(input, elem, depth + 1, max_depth, max_length)?);
            }
            Ok(Value::Seq(items))
        }

        Schema::Tuple(schemas) => {
            let mut items = Vec::with_capacity(schemas.len());
            for s in schemas {
                items.push(decode_value(input, s, depth + 1, max_depth, max_length)?);
            }
            Ok(Value::Tuple(items))
        }

        Schema::Struct(field_schemas) => {
            let mut fields = Vec::with_capacity(field_schemas.len());
            for (name, s) in field_schemas {
                let val = decode_value(input, s, depth + 1, max_depth, max_length)?;
                fields.push((name.clone(), val));
            }
            Ok(Value::Struct(fields))
        }

        Schema::Enum(variants, width) => {
            let idx = wire::read_variant_index(input, *width)?;
            let idx_usize = idx as usize;
            if idx_usize >= variants.len() {
                return Err(CordError::SchemaError(format!(
                    "Enum variant index {} out of range (max {})",
                    idx,
                    variants.len() - 1
                )));
            }
            let (name, variant_schema) = &variants[idx_usize];
            let payload = decode_variant(input, variant_schema, depth + 1, max_depth, max_length)?;
            Ok(Value::Enum {
                variant_index: idx,
                variant_name: name.clone(),
                payload: Box::new(payload),
            })
        }

        Schema::Map(key_schema, val_schema, width) => {
            let count = wire::read_length(input, *width, max_length)?;
            let mut entries = Vec::with_capacity(count);
            let mut prev_key_bytes: Option<Vec<u8>> = None;
            for _ in 0..count {
                let before = *input;
                let key = decode_value(input, key_schema, depth + 1, max_depth, max_length)?;
                let key_bytes_len = before.len() - input.len();
                let key_bytes = &before[..key_bytes_len];

                if let Some(ref prev) = prev_key_bytes {
                    if key_bytes <= prev.as_slice() {
                        return Err(CordError::ValidationError(
                            "Unordered or duplicate map keys",
                        ));
                    }
                }
                prev_key_bytes = Some(key_bytes.to_vec());

                let val = decode_value(input, val_schema, depth + 1, max_depth, max_length)?;
                entries.push((key, val));
            }
            Ok(Value::Map(entries))
        }

        Schema::Set(elem_schema, width) => {
            let count = wire::read_length(input, *width, max_length)?;
            let mut items = Vec::with_capacity(count);
            let mut prev_elem_bytes: Option<Vec<u8>> = None;
            for _ in 0..count {
                let before = *input;
                let item = decode_value(input, elem_schema, depth + 1, max_depth, max_length)?;
                let elem_bytes_len = before.len() - input.len();
                let elem_bytes = &before[..elem_bytes_len];

                if let Some(ref prev) = prev_elem_bytes {
                    if elem_bytes <= prev.as_slice() {
                        return Err(CordError::DuplicateSetElement);
                    }
                }
                prev_elem_bytes = Some(elem_bytes.to_vec());

                items.push(item);
            }
            Ok(Value::Set(items))
        }

        // Evolving wrapper
        Schema::Evolving(inner, width) => {
            let payload_len = wire::read_length(input, *width, max_length)?;
            let payload_bytes = wire::read_bytes(input, payload_len)?;
            let mut cursor = payload_bytes;
            match decode_value(&mut cursor, inner, depth + 1, max_depth, max_length) {
                Ok(val) if cursor.is_empty() => Ok(val),
                Ok(_) => Err(CordError::ValidationError(
                    "trailing bytes in evolving payload",
                )),
                Err(_) => Ok(Value::UnknownEvolving(payload_bytes.to_vec())),
            }
        }
    }
}

fn decode_variant(
    input: &mut &[u8],
    schema: &VariantSchema,
    depth: usize,
    max_depth: usize,
    max_length: usize,
) -> CordResult<VariantValue> {
    match schema {
        VariantSchema::Unit => Ok(VariantValue::Unit),
        VariantSchema::Newtype(s) => {
            let val = decode_value(input, s, depth, max_depth, max_length)?;
            Ok(VariantValue::Newtype(val))
        }
        VariantSchema::Tuple(schemas) => {
            let mut items = Vec::with_capacity(schemas.len());
            for s in schemas {
                items.push(decode_value(input, s, depth, max_depth, max_length)?);
            }
            Ok(VariantValue::Tuple(items))
        }
        VariantSchema::Struct(field_schemas) => {
            let mut fields = Vec::with_capacity(field_schemas.len());
            for (name, s) in field_schemas {
                let val = decode_value(input, s, depth, max_depth, max_length)?;
                fields.push((name.clone(), val));
            }
            Ok(VariantValue::Struct(fields))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::{Schema, Value, VariantSchema, VariantValue, Width};

    // --- 1. Primitive roundtrips ---

    #[test]
    fn roundtrip_unit() {
        let v = Value::Unit;
        let bytes = encode(&v, &Schema::Unit).unwrap();
        assert_eq!(bytes, Vec::<u8>::new());
        assert_eq!(decode(&Schema::Unit, &bytes).unwrap(), v);
    }

    #[test]
    fn roundtrip_bool() {
        for b in [true, false] {
            let v = Value::Bool(b);
            let bytes = encode(&v, &Schema::Bool).unwrap();
            assert_eq!(decode(&Schema::Bool, &bytes).unwrap(), v);
        }
    }

    #[test]
    fn roundtrip_u8() {
        let v = Value::U8(42);
        let bytes = encode(&v, &Schema::U8).unwrap();
        assert_eq!(bytes, vec![42]);
        assert_eq!(decode(&Schema::U8, &bytes).unwrap(), v);
    }

    #[test]
    fn roundtrip_u16() {
        let v = Value::U16(1000);
        let bytes = encode(&v, &Schema::U16).unwrap();
        assert_eq!(bytes, 1000_u16.to_be_bytes().to_vec());
        assert_eq!(decode(&Schema::U16, &bytes).unwrap(), v);
    }

    #[test]
    fn roundtrip_u32() {
        let v = Value::U32(70000);
        let bytes = encode(&v, &Schema::U32).unwrap();
        assert_eq!(decode(&Schema::U32, &bytes).unwrap(), v);
    }

    #[test]
    fn roundtrip_u64() {
        let v = Value::U64(123456789);
        let bytes = encode(&v, &Schema::U64).unwrap();
        assert_eq!(decode(&Schema::U64, &bytes).unwrap(), v);
    }

    #[test]
    fn roundtrip_i8() {
        let v = Value::I8(-30);
        let bytes = encode(&v, &Schema::I8).unwrap();
        assert_eq!(decode(&Schema::I8, &bytes).unwrap(), v);
    }

    #[test]
    fn roundtrip_i16() {
        let v = Value::I16(-1000);
        let bytes = encode(&v, &Schema::I16).unwrap();
        assert_eq!(decode(&Schema::I16, &bytes).unwrap(), v);
    }

    #[test]
    fn roundtrip_i32() {
        let v = Value::I32(-70000);
        let bytes = encode(&v, &Schema::I32).unwrap();
        assert_eq!(decode(&Schema::I32, &bytes).unwrap(), v);
    }

    #[test]
    fn roundtrip_i64() {
        let v = Value::I64(-123456789);
        let bytes = encode(&v, &Schema::I64).unwrap();
        assert_eq!(decode(&Schema::I64, &bytes).unwrap(), v);
    }

    #[test]
    fn roundtrip_string() {
        let v = Value::String("hello".into());
        let bytes = encode(&v, &Schema::string()).unwrap();
        assert_eq!(decode(&Schema::string(), &bytes).unwrap(), v);
    }

    #[test]
    fn roundtrip_bytes() {
        let v = Value::Bytes(vec![1, 2, 3, 4]);
        let bytes = encode(&v, &Schema::bytes()).unwrap();
        assert_eq!(decode(&Schema::bytes(), &bytes).unwrap(), v);
    }

    // --- 2. Cross-path compatibility ---

    #[test]
    fn cross_path_struct() {
        #[derive(serde::Serialize, serde::Deserialize, Debug, PartialEq)]
        struct Foo {
            name: String,
            age: u32,
        }

        let schema = Schema::Struct(vec![
            ("name".into(), Schema::string()),
            ("age".into(), Schema::U32),
        ]);

        // serde -> dynamic decode
        let foo = Foo {
            name: "Alice".into(),
            age: 30,
        };
        let serde_bytes = crate::serialize(&foo).unwrap();
        let dynamic_val = decode(&schema, &serde_bytes).unwrap();
        assert_eq!(
            dynamic_val,
            Value::Struct(vec![
                ("name".into(), Value::String("Alice".into())),
                ("age".into(), Value::U32(30)),
            ])
        );

        // dynamic encode -> serde decode
        let dynamic_bytes = encode(&dynamic_val, &schema).unwrap();
        assert_eq!(serde_bytes, dynamic_bytes);
        let decoded_foo: Foo = crate::deserialize(&dynamic_bytes).unwrap();
        assert_eq!(decoded_foo, foo);
    }

    // --- 3. Map canonical ordering ---

    #[test]
    fn map_canonical_ordering() {
        let schema = Schema::map(Schema::string(), Schema::U32);
        let val = Value::Map(vec![
            (Value::String("b".into()), Value::U32(2)),
            (Value::String("a".into()), Value::U32(1)),
        ]);
        let bytes = encode(&val, &schema).unwrap();

        // Compare with serde path
        let mut inner = std::collections::HashMap::new();
        inner.insert("a".to_string(), 1_u32);
        inner.insert("b".to_string(), 2_u32);
        let serde_bytes = crate::serialize(&crate::Map::from(inner)).unwrap();
        assert_eq!(bytes, serde_bytes);

        let decoded = decode(&schema, &bytes).unwrap();
        // Decoded map should be in sorted order
        if let Value::Map(entries) = &decoded {
            assert_eq!(entries[0].0, Value::String("a".into()));
            assert_eq!(entries[1].0, Value::String("b".into()));
        } else {
            panic!("Expected Map");
        }
    }

    // --- 4. Set canonical ordering ---

    #[test]
    fn set_canonical_ordering() {
        let schema = Schema::set(Schema::string());
        let val = Value::Set(vec![
            Value::String("c".into()),
            Value::String("a".into()),
            Value::String("b".into()),
        ]);
        let bytes = encode(&val, &schema).unwrap();

        // Compare with serde path
        let set: crate::Set<String> = vec!["a", "b", "c"]
            .into_iter()
            .map(|s| s.to_string())
            .collect();
        let serde_bytes = crate::serialize(&set).unwrap();
        assert_eq!(bytes, serde_bytes);
    }

    // --- 5. Width wrappers ---

    #[test]
    fn len8_string() {
        let schema = Schema::String(Width::W8);
        let val = Value::String("test".into());
        let bytes = encode(&val, &schema).unwrap();
        // u8 length prefix
        assert_eq!(bytes[0], 4_u8);
        assert_eq!(&bytes[1..], b"test");

        // Compare with serde path
        use cord::Cord;
        #[derive(Cord)]
        struct W(#[cord(width = 8)] String);
        let serde_bytes = crate::serialize(&W("test".to_string())).unwrap();
        assert_eq!(bytes, serde_bytes);

        let decoded = decode(&schema, &bytes).unwrap();
        assert_eq!(decoded, val);
    }

    #[test]
    fn len16_string() {
        let schema = Schema::String(Width::W16);
        let val = Value::String("test".into());
        let bytes = encode(&val, &schema).unwrap();
        assert_eq!(&bytes[..2], &4_u16.to_be_bytes());

        use cord::Cord;
        #[derive(Cord)]
        struct W(#[cord(width = 16)] String);
        let serde_bytes = crate::serialize(&W("test".to_string())).unwrap();
        assert_eq!(bytes, serde_bytes);
    }

    #[test]
    fn var16_enum() {
        let schema = Schema::Enum(
            vec![
                ("Unit".into(), VariantSchema::Unit),
                ("Container".into(), VariantSchema::Newtype(Schema::U16)),
            ],
            Width::W16,
        );
        let val = Value::Enum {
            variant_index: 1,
            variant_name: "Container".into(),
            payload: Box::new(VariantValue::Newtype(Value::U16(7))),
        };
        let bytes = encode(&val, &schema).unwrap();
        // 2-byte variant index
        assert_eq!(&bytes[..2], &1_u16.to_be_bytes());

        let decoded = decode(&schema, &bytes).unwrap();
        assert_eq!(decoded, val);
    }

    // --- 6. VarInt ---

    #[test]
    fn varint_u32() {
        let schema = Schema::varint(Schema::U32);
        let val = Value::U32(42);
        let bytes = encode(&val, &schema).unwrap();

        use cord::Cord;
        #[derive(Cord)]
        struct W(#[cord(varint)] u32);
        let serde_bytes = crate::serialize(&W(42_u32)).unwrap();
        assert_eq!(bytes, serde_bytes);

        let decoded = decode(&schema, &bytes).unwrap();
        assert_eq!(decoded, val);
    }

    #[test]
    fn varint_large() {
        let schema = Schema::varint(Schema::U32);
        let val = Value::U32(1293012);
        let bytes = encode(&val, &schema).unwrap();

        use cord::Cord;
        #[derive(Cord)]
        struct W(#[cord(varint)] u32);
        let serde_bytes = crate::serialize(&W(1293012_u32)).unwrap();
        assert_eq!(bytes, serde_bytes);
    }

    // --- 7. Schema self-description ---

    #[test]
    fn schema_self_description() {
        let schema = Schema::Struct(vec![
            ("name".into(), Schema::string()),
            ("age".into(), Schema::U32),
        ]);
        let bytes = crate::serialize(&schema).unwrap();
        let roundtripped: Schema = crate::deserialize(&bytes).unwrap();
        assert_eq!(schema, roundtripped);
    }

    #[test]
    fn schema_complex_self_description() {
        let schema = Schema::r#enum(vec![
            ("A".into(), VariantSchema::Unit),
            (
                "B".into(),
                VariantSchema::Struct(vec![
                    ("x".into(), Schema::varint(Schema::U32)),
                    ("y".into(), Schema::option(Schema::string())),
                ]),
            ),
        ]);
        let bytes = crate::serialize(&schema).unwrap();
        let roundtripped: Schema = crate::deserialize(&bytes).unwrap();
        assert_eq!(schema, roundtripped);
    }

    // --- 8. Option handling ---

    #[test]
    fn option_none() {
        let schema = Schema::option(Schema::U32);
        let val = Value::None;
        let bytes = encode(&val, &schema).unwrap();
        assert_eq!(bytes, vec![0]);
        assert_eq!(decode(&schema, &bytes).unwrap(), val);
    }

    #[test]
    fn option_some() {
        let schema = Schema::option(Schema::U32);
        let val = Value::Some(Box::new(Value::U32(42)));
        let bytes = encode(&val, &schema).unwrap();
        let mut expected = vec![1];
        expected.extend_from_slice(&42_u32.to_be_bytes());
        assert_eq!(bytes, expected);
        assert_eq!(decode(&schema, &bytes).unwrap(), val);
    }

    // --- 9. Trailing bytes rejection ---

    #[test]
    fn trailing_bytes_rejected() {
        let mut bytes = encode(&Value::U32(1), &Schema::U32).unwrap();
        bytes.push(0xFF);
        assert!(decode(&Schema::U32, &bytes).is_err());
    }

    // --- 10. NFC string normalization ---

    #[cfg(feature = "unicode")]
    #[test]
    fn nfc_normalization() {
        let nfd = "caf\u{0065}\u{0301}"; // NFD
        let nfc = "caf\u{00e9}"; // NFC

        let schema = Schema::string();
        let bytes_nfd = encode(&Value::String(nfd.into()), &schema).unwrap();
        let bytes_nfc = encode(&Value::String(nfc.into()), &schema).unwrap();
        assert_eq!(bytes_nfd, bytes_nfc);

        // Also matches serde path
        let serde_bytes = crate::serialize(nfc).unwrap();
        assert_eq!(bytes_nfc, serde_bytes);
    }

    // --- 11. Enum variants ---

    #[test]
    fn enum_unit_variant() {
        let schema = Schema::r#enum(vec![
            ("A".into(), VariantSchema::Unit),
            ("B".into(), VariantSchema::Unit),
        ]);
        let val = Value::Enum {
            variant_index: 0,
            variant_name: "A".into(),
            payload: Box::new(VariantValue::Unit),
        };
        let bytes = encode(&val, &schema).unwrap();
        assert_eq!(bytes, 0_u32.to_be_bytes());
        assert_eq!(decode(&schema, &bytes).unwrap(), val);
    }

    #[test]
    fn enum_newtype_variant() {
        let schema = Schema::r#enum(vec![
            ("Unit".into(), VariantSchema::Unit),
            ("Container".into(), VariantSchema::Newtype(Schema::U16)),
        ]);
        let val = Value::Enum {
            variant_index: 1,
            variant_name: "Container".into(),
            payload: Box::new(VariantValue::Newtype(Value::U16(1))),
        };
        let bytes = encode(&val, &schema).unwrap();

        // Compare with serde path
        #[derive(serde::Serialize)]
        #[allow(dead_code)]
        enum Enum {
            Unit,
            Container(u16),
        }
        let serde_bytes = crate::serialize(&Enum::Container(1)).unwrap();
        assert_eq!(bytes, serde_bytes);

        assert_eq!(decode(&schema, &bytes).unwrap(), val);
    }

    #[test]
    fn enum_tuple_variant() {
        let schema = Schema::r#enum(vec![
            ("Unit".into(), VariantSchema::Unit),
            ("Container".into(), VariantSchema::Newtype(Schema::U16)),
            (
                "TupleContainer".into(),
                VariantSchema::Tuple(vec![Schema::U16, Schema::U16]),
            ),
        ]);
        let val = Value::Enum {
            variant_index: 2,
            variant_name: "TupleContainer".into(),
            payload: Box::new(VariantValue::Tuple(vec![Value::U16(5), Value::U16(6)])),
        };
        let bytes = encode(&val, &schema).unwrap();
        assert_eq!(decode(&schema, &bytes).unwrap(), val);
    }

    #[test]
    fn enum_struct_variant() {
        let schema = Schema::r#enum(vec![
            ("Unit".into(), VariantSchema::Unit),
            ("Container".into(), VariantSchema::Newtype(Schema::U16)),
            (
                "TupleContainer".into(),
                VariantSchema::Tuple(vec![Schema::U16, Schema::U16]),
            ),
            (
                "Struct".into(),
                VariantSchema::Struct(vec![("field".into(), Schema::U32)]),
            ),
        ]);
        let val = Value::Enum {
            variant_index: 3,
            variant_name: "Struct".into(),
            payload: Box::new(VariantValue::Struct(vec![("field".into(), Value::U32(42))])),
        };
        let bytes = encode(&val, &schema).unwrap();
        assert_eq!(decode(&schema, &bytes).unwrap(), val);
    }

    // --- 12. DateTime ---

    #[cfg(feature = "datetime")]
    #[test]
    fn datetime_roundtrip() {
        let dt = chrono::DateTime::parse_from_rfc3339("2023-10-05T14:30:00.123456789Z")
            .unwrap()
            .with_timezone(&chrono::Utc);
        let schema = Schema::DateTime;
        let val = Value::DateTime(dt);
        let bytes = encode(&val, &schema).unwrap();
        assert_eq!(bytes.len(), 12);
        assert_eq!(decode(&schema, &bytes).unwrap(), val);
    }

    #[cfg(feature = "datetime")]
    #[test]
    fn datetime_pre_epoch() {
        let dt = chrono::DateTime::parse_from_rfc3339("1969-12-31T23:59:59.500Z")
            .unwrap()
            .with_timezone(&chrono::Utc);
        let schema = Schema::DateTime;
        let val = Value::DateTime(dt);
        let bytes = encode(&val, &schema).unwrap();
        assert_eq!(decode(&schema, &bytes).unwrap(), val);

        // Cross-path
        let cord_dt: crate::DateTime = dt.into();
        let serde_bytes = crate::serialize(&cord_dt).unwrap();
        assert_eq!(bytes, serde_bytes);
    }

    #[cfg(feature = "datetime")]
    #[test]
    fn datetime_invalid_nanos() {
        // Manually construct bytes with invalid nanos
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&0_i64.to_be_bytes());
        bytes.extend_from_slice(&1_000_000_000_u32.to_be_bytes());
        assert!(decode(&Schema::DateTime, &bytes).is_err());
    }

    // --- Float tests ---

    #[test]
    fn roundtrip_f32() {
        let v = Value::F32(42.5);
        let bytes = encode(&v, &Schema::F32).unwrap();
        assert_eq!(bytes.len(), 4);
        assert_eq!(decode(&Schema::F32, &bytes).unwrap(), v);
    }

    #[test]
    fn roundtrip_f64() {
        let v = Value::F64(-123.456);
        let bytes = encode(&v, &Schema::F64).unwrap();
        assert_eq!(bytes.len(), 8);
        assert_eq!(decode(&Schema::F64, &bytes).unwrap(), v);
    }

    #[test]
    fn f32_nan_encode_rejected() {
        let v = Value::F32(f32::NAN);
        assert_eq!(
            encode(&v, &Schema::F32).unwrap_err(),
            CordError::NanNotAllowed
        );
    }

    #[test]
    fn f64_nan_encode_rejected() {
        let v = Value::F64(f64::NAN);
        assert_eq!(
            encode(&v, &Schema::F64).unwrap_err(),
            CordError::NanNotAllowed
        );
    }

    #[test]
    fn f32_nan_decode_rejected() {
        let bytes = f32::NAN.to_be_bytes();
        assert_eq!(
            decode(&Schema::F32, &bytes).unwrap_err(),
            CordError::NanNotAllowed
        );
    }

    #[test]
    fn f64_nan_decode_rejected() {
        let bytes = f64::NAN.to_be_bytes();
        assert_eq!(
            decode(&Schema::F64, &bytes).unwrap_err(),
            CordError::NanNotAllowed
        );
    }

    #[test]
    fn f32_neg_zero_encode_canonicalized() {
        let v = Value::F32(-0.0);
        let bytes = encode(&v, &Schema::F32).unwrap();
        assert_eq!(bytes, 0.0_f32.to_be_bytes());
    }

    #[test]
    fn f64_neg_zero_encode_canonicalized() {
        let v = Value::F64(-0.0);
        let bytes = encode(&v, &Schema::F64).unwrap();
        assert_eq!(bytes, 0.0_f64.to_be_bytes());
    }

    #[test]
    fn f32_neg_zero_decode_rejected() {
        let bytes = (-0.0_f32).to_be_bytes();
        assert_eq!(
            decode(&Schema::F32, &bytes).unwrap_err(),
            CordError::NegativeZeroNotAllowed
        );
    }

    #[test]
    fn f64_neg_zero_decode_rejected() {
        let bytes = (-0.0_f64).to_be_bytes();
        assert_eq!(
            decode(&Schema::F64, &bytes).unwrap_err(),
            CordError::NegativeZeroNotAllowed
        );
    }

    #[test]
    fn f64_infinity_roundtrip() {
        for &v in &[f64::INFINITY, f64::NEG_INFINITY] {
            let val = Value::F64(v);
            let bytes = encode(&val, &Schema::F64).unwrap();
            assert_eq!(decode(&Schema::F64, &bytes).unwrap(), val);
        }
    }

    #[test]
    fn f32_infinity_roundtrip() {
        for &v in &[f32::INFINITY, f32::NEG_INFINITY] {
            let val = Value::F32(v);
            let bytes = encode(&val, &Schema::F32).unwrap();
            assert_eq!(decode(&Schema::F32, &bytes).unwrap(), val);
        }
    }

    #[test]
    fn f64_schema_mismatch() {
        let v = Value::F64(1.0);
        assert!(encode(&v, &Schema::F32).is_err());
        assert!(encode(&v, &Schema::U64).is_err());
    }

    #[test]
    fn f32_cross_path_compatibility() {
        // Serde serialize, dynamic decode
        let v: f32 = 3.14;
        let serde_bytes = crate::serialize(&v).unwrap();
        let dynamic_val = decode(&Schema::F32, &serde_bytes).unwrap();
        assert_eq!(dynamic_val, Value::F32(v));

        // Dynamic encode, serde deserialize
        let dynamic_bytes = encode(&Value::F32(v), &Schema::F32).unwrap();
        assert_eq!(serde_bytes, dynamic_bytes);
        let decoded: f32 = crate::deserialize(&dynamic_bytes).unwrap();
        assert_eq!(decoded, v);
    }

    #[test]
    fn f64_cross_path_compatibility() {
        let v: f64 = 2.71828;
        let serde_bytes = crate::serialize(&v).unwrap();
        let dynamic_val = decode(&Schema::F64, &serde_bytes).unwrap();
        assert_eq!(dynamic_val, Value::F64(v));

        let dynamic_bytes = encode(&Value::F64(v), &Schema::F64).unwrap();
        assert_eq!(serde_bytes, dynamic_bytes);
        let decoded: f64 = crate::deserialize(&dynamic_bytes).unwrap();
        assert_eq!(decoded, v);
    }

    // --- 13. Evolving roundtrip ---

    #[test]
    fn evolving32_known_roundtrip() {
        let inner_schema = Schema::U32;
        let schema = Schema::Evolving(Box::new(inner_schema), Width::W32);
        let val = Value::U32(42);
        let bytes = encode(&val, &schema).unwrap();

        // Should have u32 length prefix + 4 bytes payload
        assert_eq!(bytes.len(), 8);
        assert_eq!(decode(&schema, &bytes).unwrap(), val);

        // Cross-path with serde
        use cord::Cord;
        #[derive(Cord)]
        struct W(#[cord(evolving = 32)] crate::Evolving<u32>);
        let serde_bytes = crate::serialize(&W(crate::Evolving::Known(42_u32))).unwrap();
        assert_eq!(bytes, serde_bytes);
    }

    #[test]
    fn evolving8_known_roundtrip() {
        let schema = Schema::Evolving(Box::new(Schema::U8), Width::W8);
        let val = Value::U8(7);
        let bytes = encode(&val, &schema).unwrap();
        assert_eq!(bytes.len(), 2); // u8 length prefix + 1 byte
        assert_eq!(decode(&schema, &bytes).unwrap(), val);
    }

    // --- 14. Evolving unknown ---

    #[test]
    fn evolving_unknown_preserved() {
        // Create bytes that look like an evolving payload but inner decode would fail
        // (e.g., inner schema is U32 but payload is 3 bytes)
        let raw = vec![0xDE, 0xAD, 0xBE];
        let schema = Schema::Evolving(Box::new(Schema::U32), Width::W32);

        // Encode as UnknownEvolving
        let val = Value::UnknownEvolving(raw.clone());
        let bytes = encode(&val, &schema).unwrap();

        // Decode -- should come back as UnknownEvolving since inner decode fails
        let decoded = decode(&schema, &bytes).unwrap();
        assert_eq!(decoded, Value::UnknownEvolving(raw));
    }

    #[test]
    fn evolving_unknown_roundtrip_preserves_bytes() {
        // Use a payload that genuinely fails to decode as U32 (3 bytes, not 4)
        let raw = vec![1, 2, 3];
        let schema = Schema::Evolving(Box::new(Schema::U32), Width::W32);

        let val = Value::UnknownEvolving(raw.clone());
        let bytes1 = encode(&val, &schema).unwrap();
        let decoded = decode(&schema, &bytes1).unwrap();
        let bytes2 = encode(&decoded, &schema).unwrap();
        assert_eq!(bytes1, bytes2);
    }

    #[test]
    fn evolving_trailing_bytes_in_payload_rejected() {
        // 5-byte payload decoded against U32 (4 bytes) succeeds but has trailing byte
        let raw = vec![1, 2, 3, 4, 5];
        let schema = Schema::Evolving(Box::new(Schema::U32), Width::W32);

        let val = Value::UnknownEvolving(raw);
        let bytes = encode(&val, &schema).unwrap();
        assert!(decode(&schema, &bytes).is_err());
    }

    // --- Additional edge cases ---

    #[test]
    fn empty_seq() {
        let schema = Schema::seq(Schema::U32);
        let val = Value::Seq(vec![]);
        let bytes = encode(&val, &schema).unwrap();
        assert_eq!(decode(&schema, &bytes).unwrap(), val);
    }

    #[test]
    fn empty_map() {
        let schema = Schema::map(Schema::string(), Schema::U32);
        let val = Value::Map(vec![]);
        let bytes = encode(&val, &schema).unwrap();
        assert_eq!(decode(&schema, &bytes).unwrap(), val);
    }

    #[test]
    fn empty_set() {
        let schema = Schema::set(Schema::U32);
        let val = Value::Set(vec![]);
        let bytes = encode(&val, &schema).unwrap();
        assert_eq!(decode(&schema, &bytes).unwrap(), val);
    }

    #[test]
    fn nested_struct_with_option_and_seq() {
        let schema = Schema::Struct(vec![
            ("id".into(), Schema::U32),
            ("name".into(), Schema::option(Schema::string())),
            ("tags".into(), Schema::seq(Schema::string())),
        ]);

        let val = Value::Struct(vec![
            ("id".into(), Value::U32(1)),
            (
                "name".into(),
                Value::Some(Box::new(Value::String("test".into()))),
            ),
            (
                "tags".into(),
                Value::Seq(vec![Value::String("a".into()), Value::String("b".into())]),
            ),
        ]);

        let bytes = encode(&val, &schema).unwrap();
        assert_eq!(decode(&schema, &bytes).unwrap(), val);
    }

    #[test]
    fn schema_mismatch_errors() {
        assert!(encode(&Value::Bool(true), &Schema::U32).is_err());
        assert!(encode(&Value::U32(1), &Schema::string()).is_err());
    }

    #[test]
    fn map_duplicate_keys_rejected() {
        let schema = Schema::map(Schema::U32, Schema::U32);
        let val = Value::Map(vec![
            (Value::U32(1), Value::U32(10)),
            (Value::U32(1), Value::U32(20)),
        ]);
        assert!(encode(&val, &schema).is_err());
    }

    #[test]
    fn set_duplicate_elements_rejected() {
        let schema = Schema::set(Schema::U32);
        let val = Value::Set(vec![Value::U32(1), Value::U32(1)]);
        assert!(encode(&val, &schema).is_err());
    }

    #[test]
    fn var8_enum_roundtrip() {
        let schema = Schema::Enum(
            vec![
                ("Unit".into(), VariantSchema::Unit),
                ("Container".into(), VariantSchema::Newtype(Schema::U16)),
            ],
            Width::W8,
        );
        let val = Value::Enum {
            variant_index: 0,
            variant_name: "Unit".into(),
            payload: Box::new(VariantValue::Unit),
        };
        let bytes = encode(&val, &schema).unwrap();
        // u8 variant index
        assert_eq!(bytes, vec![0]);

        // Compare with serde path
        use cord::Cord;
        #[derive(Cord, Debug, PartialEq)]
        enum E {
            Unit,
            Container(u16),
        }
        #[derive(Cord)]
        struct W(#[cord(width = 8)] E);
        let serde_bytes = crate::serialize(&W(E::Unit)).unwrap();
        assert_eq!(bytes, serde_bytes);

        assert_eq!(decode(&schema, &bytes).unwrap(), val);
    }

    #[test]
    fn len64_bytes_roundtrip() {
        let schema = Schema::Bytes(Width::W64);
        let val = Value::Bytes(vec![1, 2, 3]);
        let bytes = encode(&val, &schema).unwrap();

        use cord::Cord;
        #[derive(Cord)]
        struct W(#[cord(width = 64)] crate::Bytes);
        let serde_bytes = crate::serialize(&W(crate::Bytes::from(vec![1, 2, 3]))).unwrap();
        assert_eq!(bytes, serde_bytes);

        assert_eq!(decode(&schema, &bytes).unwrap(), val);
    }

    #[test]
    fn tuple_roundtrip() {
        let schema = Schema::Tuple(vec![Schema::U8, Schema::U16, Schema::U32]);
        let val = Value::Tuple(vec![Value::U8(1), Value::U16(2), Value::U32(3)]);
        let bytes = encode(&val, &schema).unwrap();
        // No length prefix, just concatenated
        let mut expected = vec![1u8];
        expected.extend_from_slice(&2_u16.to_be_bytes());
        expected.extend_from_slice(&3_u32.to_be_bytes());
        assert_eq!(bytes, expected);
        assert_eq!(decode(&schema, &bytes).unwrap(), val);
    }

    #[test]
    fn decode_depth_limit_nested_seq() {
        // Schema: Seq(Seq(U8)) -- two levels of nesting
        let schema = Schema::Seq(
            Box::new(Schema::Seq(Box::new(Schema::U8), Width::W32)),
            Width::W32,
        );
        let val = Value::Seq(vec![Value::Seq(vec![Value::U8(1)])]);
        let bytes = encode(&val, &schema).unwrap();

        // With max_depth=1, inner Seq at depth 2 should fail
        let mut input = bytes.as_slice();
        let result = super::decode_value(&mut input, &schema, 0, 1, crate::de::DEFAULT_MAX_LENGTH);
        assert_eq!(result.unwrap_err(), CordError::DepthLimitExceeded);
    }

    #[test]
    fn decode_depth_limit_nested_option() {
        // Schema: Option(Option(U8))
        let schema = Schema::Option(Box::new(Schema::Option(Box::new(Schema::U8))));
        let val = Value::Some(Box::new(Value::Some(Box::new(Value::U8(42)))));
        let bytes = encode(&val, &schema).unwrap();

        // With max_depth=1, inner Option at depth 2 should fail
        let mut input = bytes.as_slice();
        let result = super::decode_value(&mut input, &schema, 0, 1, crate::de::DEFAULT_MAX_LENGTH);
        assert_eq!(result.unwrap_err(), CordError::DepthLimitExceeded);
    }

    #[test]
    fn decode_default_depth_allows_normal_values() {
        // Moderate nesting should work fine with default limit
        let schema = Schema::Struct(vec![(
            "items".into(),
            Schema::Seq(Box::new(Schema::Option(Box::new(Schema::U32))), Width::W32),
        )]);
        let val = Value::Struct(vec![(
            "items".into(),
            Value::Seq(vec![Value::Some(Box::new(Value::U32(42)))]),
        )]);
        let bytes = encode(&val, &schema).unwrap();
        assert_eq!(decode(&schema, &bytes).unwrap(), val);
    }

    #[test]
    fn string_width_w8() {
        // Previously tested as Schema::Len8(Box::new(Schema::String))
        let schema = Schema::String(Width::W8);
        let val = Value::Some(Box::new(Value::String("hi".into())));
        // This should fail because the value is Option but schema is String
        let err = encode(&val, &schema).unwrap_err();
        assert!(matches!(err, CordError::SchemaError(_)));
    }
}
