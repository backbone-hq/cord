#[cfg(feature = "uuid")]
use crate::private::SENTINEL_UUID;
use crate::private::{
    evolving_width, sentinel_to_hint, EncodingHint, SENTINEL_EVOLVING32, SENTINEL_EVOLVING32_RAW,
};
use crate::result::{CordError, CordResult};
use crate::wire;
use serde::{ser, Serialize, Serializer};

pub fn serialize<T>(value: &T) -> CordResult<Vec<u8>>
where
    T: ?Sized + Serialize,
{
    let mut output = Vec::with_capacity(64);
    value.serialize(CordSerializer::new(&mut output))?;
    Ok(output)
}

pub(crate) fn serialize_into<T>(buf: &mut Vec<u8>, value: &T) -> CordResult<()>
where
    T: ?Sized + Serialize,
{
    value.serialize(CordSerializer::new(buf))
}

struct CordSerializer<'a, W: ?Sized> {
    output: &'a mut W,
    hint: EncodingHint,
}

impl<'a, W> CordSerializer<'a, W>
where
    W: ?Sized + std::io::Write,
{
    fn new(output: &'a mut W) -> Self {
        Self {
            output,
            hint: EncodingHint::Default,
        }
    }

    fn serialize_length(&mut self, v: usize) -> CordResult<()> {
        let width = self.hint.width();
        self.hint = EncodingHint::Default;
        wire::write_length(self.output, v, width)
    }

    fn serialize_variant_index(&mut self, v: u32) -> CordResult<()> {
        let width = self.hint.width();
        self.hint = EncodingHint::Default;
        wire::write_variant_index(self.output, v, width)
    }

    fn write_varint<T: crate::varint::VarIntEncoding>(&mut self, v: T) -> CordResult<()> {
        wire::write_varint_to(self.output, v)
    }
}

macro_rules! serialize_fixed {
    ($(($int:ty, $name:ident)),*) => {
        $(
            fn $name(mut self, v: $int) -> CordResult<()> {

                if self.hint == EncodingHint::VarInt {
                    self.hint = EncodingHint::Default;
                    self.write_varint(v)
                } else {
                    self.output.write_all(&v.to_be_bytes())?;
                    Ok(())
                }
            }
        )*
    };
}

impl<'a, W> ser::Serializer for CordSerializer<'a, W>
where
    W: ?Sized + std::io::Write,
{
    type Ok = ();
    type Error = CordError;
    type SerializeSeq = Self;
    type SerializeTuple = Self;
    type SerializeTupleStruct = Self;
    type SerializeTupleVariant = Self;
    type SerializeMap = MapSerializer<'a, W>;
    type SerializeStruct = Self;
    type SerializeStructVariant = Self;

    fn serialize_bool(self, v: bool) -> CordResult<()> {
        self.output.write_all(&[v as u8])?;
        Ok(())
    }

    serialize_fixed!(
        (i8, serialize_i8),
        (i16, serialize_i16),
        (i32, serialize_i32),
        (i64, serialize_i64),
        (i128, serialize_i128),
        (u8, serialize_u8),
        (u16, serialize_u16),
        (u32, serialize_u32),
        (u64, serialize_u64),
        (u128, serialize_u128)
    );

    fn serialize_f32(self, v: f32) -> CordResult<()> {
        if v.is_nan() {
            return Err(CordError::NanNotAllowed);
        }
        // Canonicalize -0.0 to +0.0
        let v = if v == 0.0 { 0.0_f32 } else { v };
        self.output.write_all(&v.to_be_bytes())?;
        Ok(())
    }

    fn serialize_f64(self, v: f64) -> CordResult<()> {
        if v.is_nan() {
            return Err(CordError::NanNotAllowed);
        }
        // Canonicalize -0.0 to +0.0
        let v = if v == 0.0 { 0.0_f64 } else { v };
        self.output.write_all(&v.to_be_bytes())?;
        Ok(())
    }

    fn serialize_char(self, v: char) -> CordResult<()> {
        let mut buf = [0u8; 4];
        let s = v.encode_utf8(&mut buf);
        self.serialize_str(s)
    }

    fn serialize_str(mut self, v: &str) -> CordResult<()> {
        let normalized = wire::normalize_nfc(v);
        self.serialize_length(normalized.len())?;
        self.output.write_all(normalized.as_bytes())?;
        Ok(())
    }

    fn serialize_bytes(mut self, v: &[u8]) -> CordResult<()> {
        self.serialize_length(v.len())?;
        self.output.write_all(v)?;
        Ok(())
    }

    fn serialize_none(self) -> CordResult<()> {
        self.output.write_all(&[0u8])?;
        Ok(())
    }

    fn serialize_some<T>(self, value: &T) -> CordResult<()>
    where
        T: ?Sized + Serialize,
    {
        self.output.write_all(&[1u8])?;
        value.serialize(self)
    }

    fn serialize_unit(self) -> CordResult<()> {
        Ok(())
    }

    fn serialize_unit_struct(self, _name: &'static str) -> CordResult<()> {
        self.serialize_unit()
    }

    fn serialize_unit_variant(
        mut self,
        _name: &'static str,
        variant_index: u32,
        _variant: &'static str,
    ) -> CordResult<()> {
        self.serialize_variant_index(variant_index)?;
        Ok(())
    }

    fn serialize_newtype_struct<T>(mut self, name: &'static str, value: &T) -> CordResult<()>
    where
        T: ?Sized + Serialize,
    {
        if let Some(width) = evolving_width(name) {
            if self.hint.is_active() {
                return Err(CordError::ConflictingEncodingHints);
            }
            let mut buf = Vec::new();
            value.serialize(CordSerializer::new(&mut buf))?;
            wire::write_length(self.output, buf.len(), width)?;
            self.output.write_all(&buf)?;
            return Ok(());
        }
        #[cfg(feature = "uuid")]
        if name == SENTINEL_UUID {
            // UUID: inner payload serializes as 16 raw bytes via SerializeTuple.
            // No temp buffer needed — bytes are written directly.
            return value.serialize(self);
        }
        if let Some(hint) = sentinel_to_hint(name) {
            if self.hint.is_active() {
                return Err(CordError::ConflictingEncodingHints);
            }
            self.hint = hint;
        }
        value.serialize(self)
    }

    fn serialize_newtype_variant<T>(
        mut self,
        _name: &'static str,
        variant_index: u32,
        _variant: &'static str,
        value: &T,
    ) -> CordResult<()>
    where
        T: ?Sized + Serialize,
    {
        self.serialize_variant_index(variant_index)?;
        value.serialize(self)
    }

    fn serialize_seq(mut self, len: Option<usize>) -> CordResult<Self::SerializeSeq> {
        if let Some(len) = len {
            self.serialize_length(len)?;
            Ok(self)
        } else {
            Err(CordError::NotSupported("unsized sequences"))
        }
    }

    fn serialize_tuple(self, _len: usize) -> CordResult<Self::SerializeTuple> {
        Ok(self)
    }

    fn serialize_tuple_struct(
        self,
        _name: &'static str,
        _len: usize,
    ) -> CordResult<Self::SerializeTupleStruct> {
        Ok(self)
    }

    fn serialize_tuple_variant(
        mut self,
        _name: &'static str,
        variant_index: u32,
        _variant: &'static str,
        _len: usize,
    ) -> CordResult<Self::SerializeTupleVariant> {
        self.serialize_variant_index(variant_index)?;
        Ok(self)
    }

    fn serialize_map(self, len: Option<usize>) -> CordResult<Self::SerializeMap> {
        Ok(MapSerializer::new(self, len.unwrap_or(0)))
    }

    fn serialize_struct(
        self,
        _name: &'static str,
        _len: usize,
    ) -> CordResult<Self::SerializeStruct> {
        Ok(self)
    }

    fn serialize_struct_variant(
        mut self,
        _name: &'static str,
        variant_index: u32,
        _variant: &'static str,
        _len: usize,
    ) -> CordResult<Self::SerializeStructVariant> {
        self.serialize_variant_index(variant_index)?;
        Ok(self)
    }
}

impl<W> ser::SerializeSeq for CordSerializer<'_, W>
where
    W: ?Sized + std::io::Write,
{
    type Ok = ();
    type Error = CordError;

    fn serialize_element<T>(&mut self, value: &T) -> CordResult<()>
    where
        T: ?Sized + Serialize,
    {
        value.serialize(CordSerializer::new(self.output))
    }

    fn end(self) -> CordResult<()> {
        Ok(())
    }
}

impl<W> ser::SerializeTuple for CordSerializer<'_, W>
where
    W: ?Sized + std::io::Write,
{
    type Ok = ();
    type Error = CordError;

    fn serialize_element<T>(&mut self, value: &T) -> CordResult<()>
    where
        T: ?Sized + Serialize,
    {
        value.serialize(CordSerializer::new(self.output))
    }

    fn end(self) -> CordResult<()> {
        Ok(())
    }
}

impl<W> ser::SerializeTupleStruct for CordSerializer<'_, W>
where
    W: ?Sized + std::io::Write,
{
    type Ok = ();
    type Error = CordError;

    fn serialize_field<T>(&mut self, value: &T) -> CordResult<()>
    where
        T: ?Sized + Serialize,
    {
        value.serialize(CordSerializer::new(self.output))
    }

    fn end(self) -> CordResult<()> {
        Ok(())
    }
}

impl<W> ser::SerializeTupleVariant for CordSerializer<'_, W>
where
    W: ?Sized + std::io::Write,
{
    type Ok = ();
    type Error = CordError;

    fn serialize_field<T>(&mut self, value: &T) -> CordResult<()>
    where
        T: ?Sized + Serialize,
    {
        value.serialize(CordSerializer::new(self.output))
    }

    fn end(self) -> CordResult<()> {
        Ok(())
    }
}

struct MapSerializer<'a, W: ?Sized> {
    serializer: CordSerializer<'a, W>,
    buf: Vec<u8>,
    // (key_start, key_end, val_end) — key is buf[key_start..key_end], value is buf[key_end..val_end]
    entries: Vec<(usize, usize, usize)>,
    key_start: Option<usize>,
}

impl<'a, W: ?Sized> MapSerializer<'a, W> {
    fn new(serializer: CordSerializer<'a, W>, capacity: usize) -> Self {
        MapSerializer {
            serializer,
            buf: Vec::new(),
            entries: Vec::with_capacity(capacity),
            key_start: None,
        }
    }
}

impl<W> ser::SerializeMap for MapSerializer<'_, W>
where
    W: ?Sized + std::io::Write,
{
    type Ok = ();
    type Error = CordError;

    fn serialize_key<T>(&mut self, key: &T) -> CordResult<()>
    where
        T: ?Sized + Serialize,
    {
        self.key_start = Some(self.buf.len());
        key.serialize(CordSerializer::new(&mut self.buf))
    }

    fn serialize_value<T>(&mut self, value: &T) -> CordResult<()>
    where
        T: ?Sized + Serialize,
    {
        let key_start = self.key_start.take().ok_or_else(|| {
            CordError::SerializationError("serialize_value called before serialize_key".into())
        })?;
        let key_end = self.buf.len();
        value.serialize(CordSerializer::new(&mut self.buf))?;
        let val_end = self.buf.len();
        self.entries.push((key_start, key_end, val_end));
        Ok(())
    }

    fn end(mut self) -> CordResult<()> {
        wire::sort_and_dedup_map(&self.buf, &mut self.entries)?;

        self.serializer.serialize_length(self.entries.len())?;
        for (key_start, _key_end, val_end) in self.entries {
            self.serializer
                .output
                .write_all(&self.buf[key_start..val_end])?;
        }
        Ok(())
    }
}

impl<W> ser::SerializeStruct for CordSerializer<'_, W>
where
    W: ?Sized + std::io::Write,
{
    type Ok = ();
    type Error = CordError;

    fn serialize_field<T>(&mut self, _key: &'static str, value: &T) -> CordResult<()>
    where
        T: ?Sized + Serialize,
    {
        value.serialize(CordSerializer::new(self.output))
    }

    fn end(self) -> CordResult<()> {
        Ok(())
    }
}

impl<W> ser::SerializeStructVariant for CordSerializer<'_, W>
where
    W: ?Sized + std::io::Write,
{
    type Ok = ();
    type Error = CordError;

    fn serialize_field<T>(&mut self, _key: &'static str, value: &T) -> CordResult<()>
    where
        T: ?Sized + Serialize,
    {
        value.serialize(CordSerializer::new(self.output))
    }

    fn end(self) -> CordResult<()> {
        Ok(())
    }
}

impl Serialize for crate::Bytes {
    fn serialize<S>(&self, serializer: S) -> CordResult<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_bytes(&self.0)
    }
}

#[cfg(feature = "datetime")]
use crate::private::SENTINEL_DATETIME;
#[cfg(feature = "decimal")]
use crate::private::SENTINEL_DECIMAL;
use crate::private::{PreSerialized, SENTINEL_SET};

impl<T: Serialize + Clone + std::hash::Hash + Eq> Serialize for crate::Set<T> {
    fn serialize<S>(&self, serializer: S) -> CordResult<S::Ok, S::Error>
    where
        S: Serializer,
    {
        // Collect elements into a vec for indexing
        let elements: Vec<&T> = self.iter().collect();

        // Serialize each to bytes for canonical sort ordering
        let mut buf = Vec::with_capacity(elements.len() * 16);
        let mut ranges: Vec<(usize, usize)> = Vec::with_capacity(elements.len());
        for x in &elements {
            let start = buf.len();
            x.serialize(CordSerializer::new(&mut buf))
                .map_err(serde::ser::Error::custom)?;
            ranges.push((start, buf.len()));
        }

        // Build sorted indices and reject duplicates
        let mut indices: Vec<usize> = (0..elements.len()).collect();
        indices.sort_by(|&a, &b| buf[ranges[a].0..ranges[a].1].cmp(&buf[ranges[b].0..ranges[b].1]));
        for w in indices.windows(2) {
            let (s1, e1) = ranges[w[0]];
            let (s2, e2) = ranges[w[1]];
            if buf[s1..e1] == buf[s2..e2] {
                return Err(serde::ser::Error::custom(CordError::DuplicateSetElement));
            }
        }

        // Serialize original elements in sorted order through the actual serializer.
        // Uses sentinel so ValueSerializer can produce Value::Set.
        serializer.serialize_newtype_struct(
            SENTINEL_SET,
            &SortedSetSeq {
                elements: &elements,
                indices: &indices,
            },
        )
    }
}

/// Helper that serializes a pre-sorted slice of element references as a seq.
struct SortedSetSeq<'a, T> {
    elements: &'a [&'a T],
    indices: &'a [usize],
}

impl<T: Serialize> Serialize for SortedSetSeq<'_, T> {
    fn serialize<S: Serializer>(&self, serializer: S) -> CordResult<S::Ok, S::Error> {
        use serde::ser::SerializeSeq;
        let mut seq = serializer.serialize_seq(Some(self.indices.len()))?;
        for &idx in self.indices {
            seq.serialize_element(self.elements[idx])?;
        }
        seq.end()
    }
}

#[cfg(feature = "datetime")]
impl ser::Serialize for crate::DateTime {
    fn serialize<S>(&self, serializer: S) -> CordResult<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let nanos = self.chrono.timestamp_subsec_nanos();
        if nanos >= 1_000_000_000 {
            return Err(serde::ser::Error::custom(
                "DateTime nanos must be < 1_000_000_000",
            ));
        }
        serializer.serialize_newtype_struct(
            SENTINEL_DATETIME,
            &DateTimeTuple {
                seconds: self.chrono.timestamp(),
                nanos,
            },
        )
    }
}

/// Inner tuple payload for DateTime serialization.
#[cfg(feature = "datetime")]
struct DateTimeTuple {
    seconds: i64,
    nanos: u32,
}

#[cfg(feature = "datetime")]
impl Serialize for DateTimeTuple {
    fn serialize<S: Serializer>(&self, serializer: S) -> CordResult<S::Ok, S::Error> {
        use ser::SerializeTuple;
        let mut tup = serializer.serialize_tuple(2)?;
        tup.serialize_element(&self.seconds)?;
        tup.serialize_element(&self.nanos)?;
        tup.end()
    }
}

#[cfg(feature = "decimal")]
impl ser::Serialize for crate::Decimal {
    fn serialize<S>(&self, serializer: S) -> CordResult<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_newtype_struct(
            SENTINEL_DECIMAL,
            &DecimalPayload {
                scale: self.scale,
                tc_bytes: self.unscaled.to_signed_bytes_be(),
            },
        )
    }
}

#[cfg(feature = "decimal")]
struct DecimalPayload {
    scale: u8,
    tc_bytes: Vec<u8>,
}

#[cfg(feature = "decimal")]
impl Serialize for DecimalPayload {
    fn serialize<S: Serializer>(&self, serializer: S) -> CordResult<S::Ok, S::Error> {
        use ser::SerializeTuple;
        let mut tup = serializer.serialize_tuple(2)?;
        tup.serialize_element(&self.scale)?;
        tup.serialize_element(&crate::Bytes::from(self.tc_bytes.clone()))?;
        tup.end()
    }
}

#[cfg(feature = "uuid")]
impl ser::Serialize for crate::Uuid {
    fn serialize<S>(&self, serializer: S) -> CordResult<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_newtype_struct(SENTINEL_UUID, &UuidPayload(self.inner))
    }
}

#[cfg(feature = "uuid")]
struct UuidPayload(uuid::Uuid);

#[cfg(feature = "uuid")]
impl Serialize for UuidPayload {
    fn serialize<S: Serializer>(&self, serializer: S) -> CordResult<S::Ok, S::Error> {
        use serde::ser::SerializeTuple;
        let bytes = self.0.as_bytes();
        let mut tup = serializer.serialize_tuple(bytes.len())?;
        for b in bytes {
            tup.serialize_element(b)?;
        }
        tup.end()
    }
}

impl<K: Serialize, V: Serialize> Serialize for crate::Map<K, V> {
    fn serialize<S>(&self, serializer: S) -> CordResult<S::Ok, S::Error>
    where
        S: Serializer,
    {
        use serde::ser::SerializeMap;
        let mut map = serializer.serialize_map(Some(self.len()))?;
        for (k, v) in self.iter() {
            map.serialize_entry(k, v)?;
        }
        map.end()
    }
}

/// Default Serialize impl for `Evolving<T>` — uses 32-bit length prefix.
///
/// For other widths (8-bit, 16-bit), use `#[cord(evolving = 8)]` or
/// `#[cord(evolving = 16)]` with `#[derive(Cord)]`.
impl<T: Serialize> Serialize for crate::Evolving<T> {
    fn serialize<S>(&self, serializer: S) -> CordResult<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match self {
            crate::Evolving::Known(t) => {
                serializer.serialize_newtype_struct(SENTINEL_EVOLVING32, t)
            }
            crate::Evolving::Unknown(bytes) => {
                serializer.serialize_newtype_struct(SENTINEL_EVOLVING32_RAW, &PreSerialized(bytes))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    #[cfg(feature = "datetime")]
    use crate::DateTime;
    use crate::{deserialize, serialize, Bytes, CordError, Map};
    #[cfg(feature = "datetime")]
    use chrono::Utc;
    use cord::Cord;
    use serde::Serialize;

    #[test]
    fn serialize_unit() {
        assert_eq!(serialize(&()).unwrap(), Vec::<u8>::new());
    }

    #[test]
    fn serialize_booleans() {
        assert_eq!(serialize(&true).unwrap(), [1]);
        assert_eq!(serialize(&false).unwrap(), [0]);
    }

    #[test]
    fn serialize_numbers_fixed() {
        let u8_val: u8 = 62;
        assert_eq!(serialize(&u8_val).unwrap(), [62]);

        let i8_val: i8 = -30;
        assert_eq!(serialize(&i8_val).unwrap(), (-30_i8).to_be_bytes());

        let u32_val: u32 = 1293012;
        assert_eq!(serialize(&u32_val).unwrap(), 1293012_u32.to_be_bytes());

        let i32_val: i32 = -1238470;
        assert_eq!(serialize(&i32_val).unwrap(), (-1238470_i32).to_be_bytes());

        let u32_xs: u32 = 12;
        assert_eq!(serialize(&u32_xs).unwrap(), 12_u32.to_be_bytes());

        let u64_val: u64 = 123456789;
        assert_eq!(serialize(&u64_val).unwrap(), 123456789_u64.to_be_bytes());

        let u128_val: u128 = 340282366920938463463374607431768211455;
        assert_eq!(serialize(&u128_val).unwrap(), u128::MAX.to_be_bytes());

        let i128_val: i128 = -170141183460469231731687303715884105728;
        assert_eq!(serialize(&i128_val).unwrap(), i128::MIN.to_be_bytes());
    }

    #[test]
    fn serialize_strings() {
        // Length prefix is u32 BE (4 bytes), then UTF-8 bytes
        let mut expected = 4_u32.to_be_bytes().to_vec();
        expected.extend_from_slice(b"test");
        assert_eq!(serialize("test").unwrap(), expected);
    }

    #[test]
    fn serialize_empty_strings() {
        assert_eq!(serialize("").unwrap(), 0_u32.to_be_bytes());
    }

    #[test]
    fn serialize_large_bytearrays() {
        let length: usize = 300;
        let value = vec![b'0'; length];

        let mut expected = (length as u32).to_be_bytes().to_vec();
        expected.extend(vec![b'0'; length]);

        assert_eq!(serialize(&value).unwrap(), expected);
    }

    #[test]
    fn serialize_empty_bytearrays() {
        let value: Vec<u8> = vec![];
        assert_eq!(serialize(&value).unwrap(), 0_u32.to_be_bytes());
    }

    #[test]
    fn serialize_bytes() {
        let bytes = Bytes::from(vec![0, 1, 2]);
        let mut expected = 3_u32.to_be_bytes().to_vec();
        expected.extend_from_slice(&[0, 1, 2]);
        assert_eq!(serialize(&bytes).unwrap(), expected);
    }

    #[test]
    fn serialize_raw_bytes() {
        let bytes: Vec<u8> = vec![0, 1, 2];
        let mut expected = 3_u32.to_be_bytes().to_vec();
        expected.extend_from_slice(&[0, 1, 2]);
        assert_eq!(serialize(&bytes).unwrap(), expected);
    }

    #[test]
    fn serialize_tuple() {
        let bytes: [u8; 3] = [0, 1, 2];
        assert_eq!(serialize(&bytes).unwrap(), vec![0, 1, 2]);
    }

    #[cfg(feature = "datetime")]
    #[test]
    fn serialize_datetime() {
        let datetime: DateTime = chrono::DateTime::parse_from_rfc3339("2023-10-05T14:30:00.000Z")
            .unwrap()
            .with_timezone(&Utc)
            .into();

        let result = serialize(&datetime).unwrap();
        let mut expected = Vec::new();
        expected.extend_from_slice(&datetime.chrono.timestamp().to_be_bytes());
        expected.extend_from_slice(&datetime.chrono.timestamp_subsec_nanos().to_be_bytes());
        assert_eq!(result, expected);
        assert_eq!(result.len(), 12);
    }

    #[test]
    fn serialize_set() {
        let set: crate::Set<String> = vec!["a", "b", "c", "d", "e", "f", "test"]
            .into_iter()
            .map(|s| s.to_string())
            .collect();

        let result = serialize(&set).unwrap();
        // Starts with length 7 as u32 BE
        assert_eq!(&result[..4], &7_u32.to_be_bytes());
        // Each string has a u32 BE length prefix
        // Verify roundtrip instead of hardcoding all bytes
        let deserialized: crate::Set<String> = crate::deserialize(&result).unwrap();
        assert_eq!(deserialized, set);
    }

    #[derive(Cord, PartialEq, Debug)]
    enum Enum {
        Unit,
        Container(u16),
        TupleContainer(u16, u16),
        Struct { field: u32 },
    }

    #[test]
    fn serialize_enum() {
        // Variant indices are u32 BE (4 bytes)
        assert_eq!(serialize(&Enum::Unit).unwrap(), 0_u32.to_be_bytes());

        let mut expected = 1_u32.to_be_bytes().to_vec();
        expected.extend_from_slice(&1_u16.to_be_bytes());
        assert_eq!(serialize(&Enum::Container(1)).unwrap(), expected);

        let mut expected = 2_u32.to_be_bytes().to_vec();
        expected.extend_from_slice(&1_u16.to_be_bytes());
        expected.extend_from_slice(&2_u16.to_be_bytes());
        assert_eq!(serialize(&Enum::TupleContainer(1, 2)).unwrap(), expected);

        let mut expected = 3_u32.to_be_bytes().to_vec();
        expected.extend_from_slice(&1_u32.to_be_bytes());
        assert_eq!(serialize(&Enum::Struct { field: 1 }).unwrap(), expected);
    }

    #[test]
    fn serialize_option() {
        let missing: Option<u8> = None;
        assert_eq!(serialize(&Some(7_u8)).unwrap(), vec![1, 7]);
        assert_eq!(serialize(&missing).unwrap(), vec![0]);
    }

    #[derive(Debug, Serialize, PartialEq)]
    struct Struct {
        int: u16,
        option: Option<u8>,
        seq: Vec<String>,
        boolean: bool,
    }

    #[test]
    fn serialize_struct() {
        let result = serialize(&Struct {
            int: 99,
            option: Some(7_u8),
            seq: vec![String::from("first"), String::from("second")],
            boolean: true,
        })
        .unwrap();

        let mut expected = Vec::new();
        expected.extend_from_slice(&99_u16.to_be_bytes()); // int
        expected.push(1); // option discriminant
        expected.push(7); // option value
        expected.extend_from_slice(&2_u32.to_be_bytes()); // seq length
        expected.extend_from_slice(&5_u32.to_be_bytes()); // "first" length
        expected.extend_from_slice(b"first");
        expected.extend_from_slice(&6_u32.to_be_bytes()); // "second" length
        expected.extend_from_slice(b"second");
        expected.push(1); // boolean

        assert_eq!(result, expected);
    }

    #[test]
    fn serialize_f64_roundtrip() {
        let value: f64 = 2.71828;
        let bytes = serialize(&value).unwrap();
        assert_eq!(bytes, value.to_be_bytes());
        let decoded: f64 = deserialize(&bytes).unwrap();
        assert_eq!(decoded, value);
    }

    #[test]
    fn serialize_f32_roundtrip() {
        let value: f32 = 3.14;
        let bytes = serialize(&value).unwrap();
        assert_eq!(bytes, value.to_be_bytes());
        let decoded: f32 = deserialize(&bytes).unwrap();
        assert_eq!(decoded, value);
    }

    #[test]
    fn serialize_f64_nan_rejected() {
        assert_eq!(serialize(&f64::NAN).unwrap_err(), CordError::NanNotAllowed);
    }

    #[test]
    fn serialize_f32_nan_rejected() {
        assert_eq!(serialize(&f32::NAN).unwrap_err(), CordError::NanNotAllowed);
    }

    #[test]
    fn serialize_f64_neg_zero_canonicalized() {
        let bytes = serialize(&(-0.0_f64)).unwrap();
        assert_eq!(bytes, 0.0_f64.to_be_bytes());
    }

    #[test]
    fn serialize_f32_neg_zero_canonicalized() {
        let bytes = serialize(&(-0.0_f32)).unwrap();
        assert_eq!(bytes, 0.0_f32.to_be_bytes());
    }

    #[test]
    fn serialize_f64_infinity() {
        let bytes = serialize(&f64::INFINITY).unwrap();
        let decoded: f64 = deserialize(&bytes).unwrap();
        assert_eq!(decoded, f64::INFINITY);
    }

    #[test]
    fn serialize_f64_neg_infinity() {
        let bytes = serialize(&f64::NEG_INFINITY).unwrap();
        let decoded: f64 = deserialize(&bytes).unwrap();
        assert_eq!(decoded, f64::NEG_INFINITY);
    }

    #[test]
    fn deserialize_f64_nan_rejected() {
        let bytes = f64::NAN.to_be_bytes();
        assert_eq!(
            deserialize::<f64>(&bytes).unwrap_err(),
            CordError::NanNotAllowed
        );
    }

    #[test]
    fn deserialize_f64_neg_zero_rejected() {
        let bytes = (-0.0_f64).to_be_bytes();
        assert_eq!(
            deserialize::<f64>(&bytes).unwrap_err(),
            CordError::NegativeZeroNotAllowed
        );
    }

    #[test]
    fn deserialize_f32_nan_rejected() {
        let bytes = f32::NAN.to_be_bytes();
        assert_eq!(
            deserialize::<f32>(&bytes).unwrap_err(),
            CordError::NanNotAllowed
        );
    }

    #[test]
    fn deserialize_f32_neg_zero_rejected() {
        let bytes = (-0.0_f32).to_be_bytes();
        assert_eq!(
            deserialize::<f32>(&bytes).unwrap_err(),
            CordError::NegativeZeroNotAllowed
        );
    }

    #[test]
    fn serialize_f32_infinity() {
        let bytes = serialize(&f32::INFINITY).unwrap();
        let decoded: f32 = deserialize(&bytes).unwrap();
        assert_eq!(decoded, f32::INFINITY);
    }

    #[test]
    fn serialize_f32_neg_infinity() {
        let bytes = serialize(&f32::NEG_INFINITY).unwrap();
        let decoded: f32 = deserialize(&bytes).unwrap();
        assert_eq!(decoded, f32::NEG_INFINITY);
    }

    #[test]
    fn serialize_f64_max_min() {
        for &v in &[f64::MAX, f64::MIN, f64::MIN_POSITIVE, f64::EPSILON] {
            let bytes = serialize(&v).unwrap();
            let decoded: f64 = deserialize(&bytes).unwrap();
            assert_eq!(decoded, v);
        }
    }

    #[test]
    fn serialize_f32_max_min() {
        for &v in &[f32::MAX, f32::MIN, f32::MIN_POSITIVE, f32::EPSILON] {
            let bytes = serialize(&v).unwrap();
            let decoded: f32 = deserialize(&bytes).unwrap();
            assert_eq!(decoded, v);
        }
    }

    #[test]
    fn serialize_f64_subnormals() {
        // Smallest subnormal
        let v = f64::from_bits(1u64);
        let bytes = serialize(&v).unwrap();
        let decoded: f64 = deserialize(&bytes).unwrap();
        assert_eq!(decoded.to_bits(), v.to_bits());
    }

    #[test]
    fn serialize_f32_subnormals() {
        let v = f32::from_bits(1u32);
        let bytes = serialize(&v).unwrap();
        let decoded: f32 = deserialize(&bytes).unwrap();
        assert_eq!(decoded.to_bits(), v.to_bits());
    }

    #[test]
    fn serialize_f64_in_struct() {
        use crate::Cord;

        #[derive(Cord, Debug, PartialEq)]
        struct Measurement {
            value: f64,
            scale: f32,
        }

        let m = Measurement {
            value: 98.6,
            scale: 1.5,
        };
        let bytes = serialize(&m).unwrap();
        let decoded: Measurement = deserialize(&bytes).unwrap();
        assert_eq!(decoded, m);
    }

    #[test]
    fn serialize_map() {
        let mut inner = std::collections::HashMap::new();
        inner.insert("aa".to_string(), 3_u32);
        inner.insert("b".to_string(), 2_u32);
        inner.insert("a".to_string(), 1_u32);

        let map = Map::from(inner);
        let result = serialize(&map).unwrap();

        // Keys sorted by serialized bytes (BE):
        // "a"  = [0,0,0,1, 97]
        // "b"  = [0,0,0,1, 98]
        // "aa" = [0,0,0,2, 97, 97]
        // So: "a" < "b" < "aa"
        let mut expected = Vec::new();
        expected.extend_from_slice(&3_u32.to_be_bytes()); // map length
                                                          // "a" -> 1
        expected.extend_from_slice(&1_u32.to_be_bytes());
        expected.extend_from_slice(b"a");
        expected.extend_from_slice(&1_u32.to_be_bytes());
        // "b" -> 2
        expected.extend_from_slice(&1_u32.to_be_bytes());
        expected.extend_from_slice(b"b");
        expected.extend_from_slice(&2_u32.to_be_bytes());
        // "aa" -> 3
        expected.extend_from_slice(&2_u32.to_be_bytes());
        expected.extend_from_slice(b"aa");
        expected.extend_from_slice(&3_u32.to_be_bytes());

        assert_eq!(result, expected);
    }

    #[test]
    fn serialize_char_ascii() {
        let value: char = 'A';
        let result = serialize(&value).unwrap();
        let mut expected = 1_u32.to_be_bytes().to_vec();
        expected.push(b'A');
        assert_eq!(result, expected);
    }

    #[test]
    fn serialize_char_multibyte() {
        let value: char = '\u{00e9}'; // é (NFC)
        let result = serialize(&value).unwrap();
        let encoded = "\u{00e9}";
        let mut expected = (encoded.len() as u32).to_be_bytes().to_vec();
        expected.extend_from_slice(encoded.as_bytes());
        assert_eq!(result, expected);
    }

    #[test]
    fn serialize_char_roundtrip() {
        let chars = ['A', 'z', '\u{00e9}', '\u{1F600}', '\u{4e16}'];
        for c in chars {
            let bytes = serialize(&c).unwrap();
            let decoded: char = crate::deserialize(&bytes).unwrap();
            assert_eq!(decoded, c);
        }
    }

    #[test]
    fn serialize_map_canonical() {
        let mut inner1 = std::collections::HashMap::new();
        inner1.insert("a".to_string(), 1_u32);
        inner1.insert("b".to_string(), 2_u32);

        let mut inner2 = std::collections::HashMap::new();
        inner2.insert("b".to_string(), 2_u32);
        inner2.insert("a".to_string(), 1_u32);

        let map1 = Map::from(inner1);
        let map2 = Map::from(inner2);

        assert_eq!(serialize(&map1).unwrap(), serialize(&map2).unwrap());
    }

    #[test]
    fn serialize_varint() {
        use crate::varint::VarIntEncoding;

        #[derive(Cord)]
        struct VarIntU32 {
            #[cord(varint)]
            inner: u32,
        }

        #[derive(Cord)]
        struct VarIntI8 {
            #[cord(varint)]
            inner: i8,
        }

        #[derive(Cord)]
        struct VarIntU128 {
            #[cord(varint)]
            inner: u128,
        }

        #[derive(Cord)]
        struct VarIntI128 {
            #[cord(varint)]
            inner: i128,
        }

        let val = VarIntU32 { inner: 1293012 };
        assert_eq!(serialize(&val).unwrap(), 1293012_u32.encode_var_vec());

        let val = VarIntU32 { inner: 12 };
        assert_eq!(serialize(&val).unwrap(), [12]);

        let val = VarIntI8 { inner: -30 };
        assert_eq!(serialize(&val).unwrap(), (-30_i8).encode_var_vec());

        // u128 varint
        let val = VarIntU128 { inner: 300 };
        assert_eq!(serialize(&val).unwrap(), 300_u128.encode_var_vec());

        // i128 varint
        let val = VarIntI128 { inner: -300 };
        assert_eq!(serialize(&val).unwrap(), (-300_i128).encode_var_vec());
    }

    #[test]
    fn serialize_varint_in_struct() {
        #[derive(Cord)]
        struct WithVarInt {
            fixed: u32,
            #[cord(varint)]
            compact: u32,
        }

        let result = serialize(&WithVarInt {
            fixed: 12,
            compact: 12,
        })
        .unwrap();

        // fixed: 4 bytes BE, compact: 1 byte varint
        assert_eq!(result, vec![0, 0, 0, 12, 12]);
    }

    #[cfg(feature = "datetime")]
    #[test]
    fn serialize_datetime_pre_epoch_ok() {
        let pre_epoch: DateTime = chrono::DateTime::parse_from_rfc3339("1969-12-31T23:59:59.500Z")
            .unwrap()
            .with_timezone(&Utc)
            .into();
        let result = serialize(&pre_epoch).unwrap();
        let mut expected = Vec::new();
        expected.extend_from_slice(&pre_epoch.chrono.timestamp().to_be_bytes());
        expected.extend_from_slice(&pre_epoch.chrono.timestamp_subsec_nanos().to_be_bytes());
        assert_eq!(result, expected);
        assert!(pre_epoch.chrono.timestamp() < 0);
    }

    #[test]
    fn serialize_set_canonical() {
        // Two sets with same elements should produce identical bytes
        let set1: crate::Set<String> = vec!["z", "a", "m"]
            .into_iter()
            .map(|s| s.to_string())
            .collect();
        let set2: crate::Set<String> = vec!["m", "z", "a"]
            .into_iter()
            .map(|s| s.to_string())
            .collect();
        assert_eq!(serialize(&set1).unwrap(), serialize(&set2).unwrap());
    }

    #[test]
    fn serialize_len8_string() {
        #[derive(Cord)]
        struct Len8String {
            #[cord(width = 8)]
            inner: String,
        }
        let val = Len8String {
            inner: "test".to_string(),
        };
        let mut expected = vec![4_u8]; // u8 length prefix
        expected.extend_from_slice(b"test");
        assert_eq!(serialize(&val).unwrap(), expected);
    }

    #[test]
    fn serialize_len16_string() {
        #[derive(Cord)]
        struct Len16String {
            #[cord(width = 16)]
            inner: String,
        }
        let val = Len16String {
            inner: "test".to_string(),
        };
        let mut expected = 4_u16.to_be_bytes().to_vec();
        expected.extend_from_slice(b"test");
        assert_eq!(serialize(&val).unwrap(), expected);
    }

    #[test]
    fn serialize_len64_bytes() {
        #[derive(Cord)]
        struct Len64Bytes {
            #[cord(width = 64)]
            inner: Bytes,
        }
        let val = Len64Bytes {
            inner: Bytes::from(vec![1, 2, 3]),
        };
        let mut expected = 3_u64.to_be_bytes().to_vec();
        expected.extend_from_slice(&[1, 2, 3]);
        assert_eq!(serialize(&val).unwrap(), expected);
    }

    #[test]
    fn serialize_len16_vec() {
        #[derive(Cord)]
        struct Len16Vec {
            #[cord(width = 16)]
            inner: Vec<u8>,
        }
        let val = Len16Vec {
            inner: vec![10_u8, 20, 30],
        };
        let mut expected = 3_u16.to_be_bytes().to_vec();
        expected.extend_from_slice(&[10, 20, 30]);
        assert_eq!(serialize(&val).unwrap(), expected);
    }

    #[test]
    fn serialize_var8_enum() {
        #[derive(Cord)]
        struct Var8Wrapper {
            #[cord(width = 8)]
            inner: Enum,
        }
        // Unit variant
        assert_eq!(
            serialize(&Var8Wrapper { inner: Enum::Unit }).unwrap(),
            vec![0_u8]
        );

        // Newtype variant
        let mut expected = vec![1_u8];
        expected.extend_from_slice(&1_u16.to_be_bytes());
        assert_eq!(
            serialize(&Var8Wrapper {
                inner: Enum::Container(1)
            })
            .unwrap(),
            expected
        );
    }

    #[test]
    fn serialize_var16_enum() {
        #[derive(Cord)]
        struct Var16Wrapper {
            #[cord(width = 16)]
            inner: Enum,
        }
        assert_eq!(
            serialize(&Var16Wrapper { inner: Enum::Unit }).unwrap(),
            0_u16.to_be_bytes()
        );

        let mut expected = 1_u16.to_be_bytes().to_vec();
        expected.extend_from_slice(&1_u16.to_be_bytes());
        assert_eq!(
            serialize(&Var16Wrapper {
                inner: Enum::Container(1)
            })
            .unwrap(),
            expected
        );
    }

    #[test]
    fn serialize_var64_enum() {
        #[derive(Cord)]
        struct Var64Wrapper {
            #[cord(width = 64)]
            inner: Enum,
        }
        assert_eq!(
            serialize(&Var64Wrapper { inner: Enum::Unit }).unwrap(),
            0_u64.to_be_bytes()
        );
    }

    #[test]
    fn serialize_var8_struct_variant() {
        #[derive(Cord)]
        struct Var8Wrapper {
            #[cord(width = 8)]
            inner: Enum,
        }
        let mut expected = vec![3_u8]; // variant index
        expected.extend_from_slice(&42_u32.to_be_bytes());
        assert_eq!(
            serialize(&Var8Wrapper {
                inner: Enum::Struct { field: 42 }
            })
            .unwrap(),
            expected
        );
    }

    #[test]
    fn serialize_var8_tuple_variant() {
        #[derive(Cord)]
        struct Var8Wrapper {
            #[cord(width = 8)]
            inner: Enum,
        }
        let mut expected = vec![2_u8]; // variant index
        expected.extend_from_slice(&5_u16.to_be_bytes());
        expected.extend_from_slice(&6_u16.to_be_bytes());
        assert_eq!(
            serialize(&Var8Wrapper {
                inner: Enum::TupleContainer(5, 6)
            })
            .unwrap(),
            expected
        );
    }

    #[test]
    fn serialize_width_wrappers_in_struct() {
        #[derive(Cord)]
        struct Packet {
            #[cord(width = 8)]
            kind: Enum,
            #[cord(width = 8)]
            name: String,
            fixed: u32,
        }

        let result = serialize(&Packet {
            kind: Enum::Container(7),
            name: "hi".to_string(),
            fixed: 99,
        })
        .unwrap();

        let mut expected = Vec::new();
        expected.push(1_u8); // Var8 variant index for Container
        expected.extend_from_slice(&7_u16.to_be_bytes()); // Container payload
        expected.push(2_u8); // Len8 string length
        expected.extend_from_slice(b"hi");
        expected.extend_from_slice(&99_u32.to_be_bytes()); // fixed u32
        assert_eq!(result, expected);
    }

    #[cfg(feature = "unicode")]
    #[test]
    fn serialize_string_nfc_normalized() {
        // "é" as NFD (e + combining acute) should produce same bytes as NFC (é)
        let nfd = "caf\u{0065}\u{0301}"; // NFD: e + combining acute
        let nfc = "caf\u{00e9}"; // NFC: precomposed é
        assert_eq!(serialize(nfd).unwrap(), serialize(nfc).unwrap());
    }

    #[test]
    fn serialize_string_already_nfc() {
        // Pure ASCII is always NFC — verify no overhead
        let s = "hello world";
        let mut expected = (s.len() as u32).to_be_bytes().to_vec();
        expected.extend_from_slice(s.as_bytes());
        assert_eq!(serialize(s).unwrap(), expected);
    }

    #[cfg(feature = "datetime")]
    #[test]
    fn serialize_datetime_sub_millisecond_ok() {
        use chrono::NaiveDateTime;
        // Nanosecond precision should now serialize successfully
        let naive =
            NaiveDateTime::parse_from_str("2023-10-05 14:30:00.000500", "%Y-%m-%d %H:%M:%S%.f")
                .unwrap();
        let dt: DateTime = chrono::DateTime::<Utc>::from_naive_utc_and_offset(naive, Utc).into();
        let result = serialize(&dt).unwrap();
        assert_eq!(result.len(), 12);
        // Roundtrip to verify precision is preserved
        let decoded: DateTime = crate::deserialize(&result).unwrap();
        assert_eq!(decoded, dt);
    }

    #[cfg(feature = "datetime")]
    #[test]
    fn serialize_datetime_nanosecond_precision() {
        use chrono::NaiveDateTime;
        let naive =
            NaiveDateTime::parse_from_str("2023-10-05 14:30:00.123456789", "%Y-%m-%d %H:%M:%S%.f")
                .unwrap();
        let dt: DateTime = chrono::DateTime::<Utc>::from_naive_utc_and_offset(naive, Utc).into();
        let result = serialize(&dt).unwrap();
        assert_eq!(result.len(), 12);
        let decoded: DateTime = crate::deserialize(&result).unwrap();
        assert_eq!(decoded, dt);
    }

    // Note: The conflicting wrapper nesting tests (serialize_conflicting_len_wrappers_fails,
    // serialize_conflicting_var_len_wrappers_fails, serialize_conflicting_varint_len_wrappers_fails)
    // were removed because the derive macro approach prevents these conflicts at compile time —
    // attributes are applied per-field and cannot be nested.

    #[cfg(feature = "decimal")]
    #[test]
    fn serialize_decimal_positive() {
        use crate::Decimal;
        use num_bigint::BigInt;
        // 12.345 = 12345 * 10^-3
        let d = Decimal::new(BigInt::from(12345), 3);
        let result = serialize(&d).unwrap();
        let decoded: Decimal = crate::deserialize(&result).unwrap();
        assert_eq!(decoded, d);
    }

    #[cfg(feature = "decimal")]
    #[test]
    fn serialize_decimal_negative() {
        use crate::Decimal;
        use num_bigint::BigInt;
        let d = Decimal::new(BigInt::from(-99), 1); // -9.9
        let result = serialize(&d).unwrap();
        let decoded: Decimal = crate::deserialize(&result).unwrap();
        assert_eq!(decoded, d);
    }

    #[cfg(feature = "decimal")]
    #[test]
    fn serialize_decimal_zero() {
        use crate::Decimal;
        use num_bigint::BigInt;
        let d = Decimal::new(BigInt::from(0), 5);
        let result = serialize(&d).unwrap();
        let decoded: Decimal = crate::deserialize(&result).unwrap();
        // Zero normalizes to scale=0
        assert_eq!(decoded, Decimal::new(BigInt::from(0), 0));
    }

    #[cfg(feature = "decimal")]
    #[test]
    fn serialize_decimal_trailing_zeros_normalized() {
        use crate::Decimal;
        use num_bigint::BigInt;
        // 1.200 (scale 3) should normalize to 1.2 (scale 1)
        let d = Decimal::new(BigInt::from(1200), 3);
        assert_eq!(d.scale(), 1);
        assert_eq!(*d.unscaled(), BigInt::from(12));
        let result = serialize(&d).unwrap();
        let decoded: Decimal = crate::deserialize(&result).unwrap();
        assert_eq!(decoded, d);
    }

    #[cfg(feature = "decimal")]
    #[test]
    fn serialize_decimal_canonical() {
        use crate::Decimal;
        use num_bigint::BigInt;
        // Two representations of 1.2 should produce identical bytes
        let d1 = Decimal::new(BigInt::from(12), 1);
        let d2 = Decimal::new(BigInt::from(1200), 3);
        assert_eq!(serialize(&d1).unwrap(), serialize(&d2).unwrap());
    }

    #[cfg(feature = "uuid")]
    #[test]
    fn serialize_uuid() {
        use crate::Uuid;
        let id = Uuid::new(uuid::Uuid::nil());
        let result = serialize(&id).unwrap();
        // Exactly 16 bytes, no length prefix
        assert_eq!(result.len(), 16);
        let decoded: Uuid = crate::deserialize(&result).unwrap();
        assert_eq!(decoded, id);
    }

    #[cfg(feature = "uuid")]
    #[test]
    fn serialize_uuid_roundtrip() {
        use crate::Uuid;
        // Use a known UUID
        let id = Uuid::from(uuid::Uuid::parse_str("550e8400-e29b-41d4-a716-446655440000").unwrap());
        let bytes = serialize(&id).unwrap();
        let decoded: Uuid = crate::deserialize(&bytes).unwrap();
        assert_eq!(decoded, id);
    }

    #[cfg(feature = "uuid")]
    #[test]
    fn uuid_wire_format_is_exactly_16_raw_bytes() {
        use crate::Uuid;
        let raw_bytes: [u8; 16] = [
            0x55, 0x0e, 0x84, 0x00, 0xe2, 0x9b, 0x41, 0xd4, 0xa7, 0x16, 0x44, 0x66, 0x55, 0x44,
            0x00, 0x00,
        ];
        let id = Uuid::from(uuid::Uuid::from_bytes(raw_bytes));
        let wire = serialize(&id).unwrap();
        // Wire format must be exactly the 16 UUID bytes with no length prefix
        assert_eq!(wire.len(), 16);
        assert_eq!(wire.as_slice(), &raw_bytes);
    }
}
