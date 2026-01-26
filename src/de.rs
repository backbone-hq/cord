use crate::result::{CordError, CordResult};
use crate::Map;
use crate::Set;
use crate::{Bytes, DateTime};
use integer_encoding::VarInt;
use serde::de::IntoDeserializer;
use serde::{de, Deserialize, Deserializer, Serialize};
use std::collections::HashSet;
use std::fmt::Formatter;
use std::hash::Hash;
use std::marker::PhantomData;

pub fn deserialize<'a, T>(bytes: &'a [u8]) -> CordResult<T>
where
    T: Deserialize<'a>,
{
    let mut deserializer = CordDeserializer::new(bytes);
    let result = T::deserialize(&mut deserializer)?;
    deserializer.end()?;
    Ok(result)
}

struct CordDeserializer<'de> {
    input: &'de [u8],
}

impl<'de> CordDeserializer<'de> {
    fn new(input: &'de [u8]) -> Self {
        CordDeserializer { input }
    }

    fn end(&mut self) -> CordResult<()> {
        if self.input.is_empty() {
            Ok(())
        } else {
            Err(CordError::ValidationError("Unexpected trailing bytes"))
        }
    }
}

impl<'de> CordDeserializer<'de> {
    fn peek(&mut self) -> CordResult<u8> {
        self.input
            .first()
            .copied()
            .ok_or(CordError::ValidationError("Unexpected end of stream"))
    }

    fn next(&mut self) -> CordResult<u8> {
        let byte = self.peek()?;
        self.input = &self.input[1..];
        Ok(byte)
    }

    fn consume(&mut self, size: usize) -> CordResult<()> {
        self.input = &self.input[size..];
        Ok(())
    }

    fn parse_bool(&mut self) -> CordResult<bool> {
        let byte = self.next()?;

        match byte {
            0 => Ok(false),
            1 => Ok(true),
            _ => Err(CordError::ValidationError("Invalid boolean variant")),
        }
    }

    fn parse_varint<T: VarInt>(&mut self) -> CordResult<T> {
        T::decode_var(self.input)
            .ok_or(CordError::ValidationError("Invalid varint"))
            .and_then(|(value, size)| {
                self.consume(size)?;
                Ok(value)
            })
    }

    fn parse_variant_index(&mut self) -> CordResult<u32> {
        self.parse_varint::<u32>()
    }

    fn parse_bytes(&mut self) -> CordResult<&'de [u8]> {
        let len = self.parse_varint::<usize>()?;
        let slice = self
            .input
            .get(..len)
            .ok_or(CordError::ValidationError("Unexpected end of bytestream"))?;
        self.input = &self.input[len..];
        Ok(slice)
    }

    fn parse_string(&mut self) -> CordResult<&'de str> {
        let slice = self.parse_bytes()?;
        std::str::from_utf8(slice).map_err(|_| CordError::ValidationError("Invalid UTF-8 string"))
    }
}

macro_rules! deserialize_varints {
    ($(($int:ty, $deserialize:ident, $visit:ident)),*) => {
        $(
            fn $deserialize<V>(self, visitor: V) -> CordResult<V::Value>
            where
                V: de::Visitor<'de>,
            {
                visitor.$visit(self.parse_varint::<$int>()?)
            }
        )*
    };
}

macro_rules! deserialize_unsupported {
    ($(($type:ty, $deserialize:ident, $visit:ident)),*) => {
        $(
            fn $deserialize<V>(self, _visitor: V) -> CordResult<V::Value>
            where
                V: de::Visitor<'de>,
            {
                Err(CordError::NotSupported(stringify!($type)))
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

    deserialize_varints!(
        (i8, deserialize_i8, visit_i8),
        (i16, deserialize_i16, visit_i16),
        (i32, deserialize_i32, visit_i32),
        (i64, deserialize_i64, visit_i64),
        (u8, deserialize_u8, visit_u8),
        (u16, deserialize_u16, visit_u16),
        (u32, deserialize_u32, visit_u32),
        (u64, deserialize_u64, visit_u64)
    );

    deserialize_unsupported!(
        (f32, deserialize_f32, visit_f32),
        (f64, deserialize_f64, visit_f64),
        (char, deserialize_char, visit_char)
    );

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
            1 => visitor.visit_some(self),
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

    fn deserialize_newtype_struct<V>(self, _name: &'static str, visitor: V) -> CordResult<V::Value>
    where
        V: de::Visitor<'de>,
    {
        visitor.visit_newtype_struct(&mut *self)
    }

    fn deserialize_seq<V>(self, visitor: V) -> CordResult<V::Value>
    where
        V: de::Visitor<'de>,
    {
        let len = self.parse_varint::<usize>()?;
        visitor.visit_seq(SeqDeserializer::new(self, len))
    }

    fn deserialize_tuple<V>(self, len: usize, visitor: V) -> CordResult<V::Value>
    where
        V: de::Visitor<'de>,
    {
        visitor.visit_seq(SeqDeserializer::new(self, len))
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
        visitor.visit_seq(SeqDeserializer::new(self, len))
    }

    fn deserialize_map<V>(self, visitor: V) -> CordResult<V::Value>
    where
        V: de::Visitor<'de>,
    {
        let len = self.parse_varint::<usize>()?;
        visitor.visit_map(MapDeserializer::new(self, len))
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
        visitor.visit_seq(SeqDeserializer::new(self, fields.len()))
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
        visitor.visit_enum(&mut *self)
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
    previous_key: Option<Vec<u8>>,
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

            let start_input = self.de.input;
            let key = seed.deserialize(&mut *self.de)?;
            let end_input = self.de.input;
            let key_bytes = &start_input[..start_input.len() - end_input.len()];

            if let Some(ref prev) = self.previous_key {
                if prev.as_slice() >= key_bytes {
                    return Err(CordError::ValidationError(
                        "Unordered or duplicate map keys",
                    ));
                }
            }
            self.previous_key = Some(key_bytes.to_vec());

            Ok(Some(key))
        }
    }

    fn next_value_seed<V>(&mut self, seed: V) -> CordResult<V::Value>
    where
        V: de::DeserializeSeed<'de>,
    {
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
        Ok((result?, self))
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

struct SetVisitor<T: Hash + PartialEq> {
    marker: PhantomData<fn() -> Set<T>>,
}

impl<T: Hash + PartialEq> SetVisitor<T> {
    fn new() -> Self {
        SetVisitor {
            marker: PhantomData,
        }
    }
}

impl<'de, T> de::Visitor<'de> for SetVisitor<T>
where
    T: Hash + Eq + Serialize + Deserialize<'de>,
{
    type Value = Set<T>;

    fn expecting(&self, formatter: &mut Formatter) -> std::fmt::Result {
        formatter.write_str("set")
    }

    fn visit_seq<A>(self, mut seq: A) -> CordResult<Self::Value, A::Error>
    where
        A: de::SeqAccess<'de>,
    {
        let mut hashset: HashSet<T> = HashSet::with_capacity(seq.size_hint().unwrap_or(0));
        let mut previous_element: Option<Vec<u8>> = None;
        while let Some(element) = seq.next_element::<T>()? {
            let current_element = Some(crate::serialize(&element).unwrap());
            if previous_element.is_some() && previous_element > current_element {
                return Err(de::Error::custom("unordered set"));
            }

            previous_element = current_element;
            hashset.insert(element);
        }
        Ok(Set::from(hashset))
    }
}

impl<'de, T> de::Deserialize<'de> for Set<T>
where
    T: Deserialize<'de> + Hash + Eq + Serialize,
{
    fn deserialize<D>(deserializer: D) -> CordResult<Set<T>, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_seq(SetVisitor::<T>::new())
    }
}

struct DateTimeVisitor;

impl de::Visitor<'_> for DateTimeVisitor {
    type Value = DateTime;

    fn expecting(&self, formatter: &mut Formatter) -> std::fmt::Result {
        formatter.write_str("datetime")
    }

    fn visit_u64<E>(self, v: u64) -> std::result::Result<Self::Value, E>
    where
        E: de::Error,
    {
        let millis = v as i64;
        let seconds = millis / 1000;
        let remaining_ns = (millis % 1000) * 1000000;

        let utc_dt = chrono::DateTime::<chrono::Utc>::from_timestamp(seconds, remaining_ns as u32)
            .ok_or_else(|| de::Error::custom(format!("timestamp {v} is invalid")))?;

        Ok(utc_dt.into())
    }
}

impl<'de> de::Deserialize<'de> for DateTime {
    fn deserialize<D>(deserializer: D) -> CordResult<DateTime, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_u64(DateTimeVisitor)
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
    K: Eq + Hash + Deserialize<'de>,
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
        let mut inner = std::collections::HashMap::with_capacity(map.size_hint().unwrap_or(0));
        while let Some((key, value)) = map.next_entry()? {
            inner.insert(key, value);
        }
        Ok(Map::from(inner))
    }
}

impl<'de, K, V> de::Deserialize<'de> for Map<K, V>
where
    K: Eq + Hash + Deserialize<'de> + Serialize,
    V: Deserialize<'de>,
{
    fn deserialize<D>(deserializer: D) -> CordResult<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_map(MapVisitor::new())
    }
}

#[cfg(test)]
mod tests {
    use super::deserialize;
    use crate::{Bytes, DateTime, Map};
    use chrono::Utc;
    use serde::Deserialize;

    #[derive(Debug, Deserialize, PartialEq)]
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
        let unsigned_8: Vec<u8> = vec![62];
        assert_eq!(deserialize::<u8>(&unsigned_8).unwrap(), 62_u8);

        let signed_8: Vec<u8> = vec![59];
        assert_eq!(deserialize::<i8>(&signed_8).unwrap(), -30_i8);

        let unsigned_32: Vec<u8> = vec![212, 245, 78];
        assert_eq!(deserialize::<u32>(&unsigned_32).unwrap(), 1293012_u32);

        let signed_32: Vec<u8> = vec![139, 151, 151, 1];
        assert_eq!(deserialize::<i32>(&signed_32).unwrap(), -1238470);

        let small_unsigned_32: Vec<u8> = vec![12];
        assert_eq!(deserialize::<u32>(&small_unsigned_32).unwrap(), 12_u32);
    }

    #[test]
    fn deserialize_strings() {
        let string: Vec<u8> = vec![4, 116, 101, 115, 116];
        assert_eq!(deserialize::<String>(&string).unwrap(), "test");
    }

    #[test]
    fn deserialize_empty_strings() {
        let string: Vec<u8> = vec![0];
        assert_eq!(deserialize::<String>(&string).unwrap(), "");
    }

    #[test]
    fn deserialize_large_bytearrays() {
        let length = 300;

        let mut input: Vec<u8> = vec![172, 2];
        input.extend(vec![b'0'; length]);

        assert_eq!(deserialize::<Vec<u8>>(&input).unwrap(), vec![b'0'; length]);
    }

    #[test]
    fn deserialize_empty_bytearrays() {
        let input: Vec<u8> = vec![0];
        assert_eq!(deserialize::<Vec<u8>>(&input).unwrap(), vec![]);
    }

    #[test]
    fn deserialize_bytes() {
        let input: Vec<u8> = vec![3, 0, 1, 2];
        assert_eq!(
            deserialize::<Bytes>(&input).unwrap(),
            Bytes::from(vec![0, 1, 2])
        );
    }

    #[test]
    fn deserialize_datetime() {
        let input: Vec<u8> = vec![192, 172, 251, 129, 176, 49];
        let expected_datetime: DateTime =
            chrono::DateTime::parse_from_rfc3339("2023-10-05T14:30:00.000Z")
                .unwrap()
                .with_timezone(&Utc)
                .into();

        assert_eq!(deserialize::<DateTime>(&input).unwrap(), expected_datetime);
    }

    #[test]
    fn deserialize_set() {
        let input: Vec<u8> = vec![
            7, 1, 97, 1, 98, 1, 99, 1, 100, 1, 101, 1, 102, 4, 116, 101, 115, 116,
        ];
        let expected: crate::Set<String> = vec!["a", "b", "c", "d", "e", "f", "test"]
            .into_iter()
            .map(|s| s.to_string())
            .collect();

        assert_eq!(deserialize::<crate::Set<String>>(&input).unwrap(), expected);
    }

    #[test]
    fn deserialize_enum() {
        let input: Vec<u8> = vec![0];
        assert_eq!(deserialize::<Enum>(&input).unwrap(), Enum::Unit);

        let input: Vec<u8> = vec![1, 1];
        assert_eq!(deserialize::<Enum>(&input).unwrap(), Enum::Container(1));

        let input: Vec<u8> = vec![2, 1, 2];
        assert_eq!(
            deserialize::<Enum>(&input).unwrap(),
            Enum::TupleContainer(1, 2)
        );

        let input: Vec<u8> = vec![3, 1];
        assert_eq!(
            deserialize::<Enum>(&input).unwrap(),
            Enum::Struct { field: 1 }
        );
    }

    #[test]
    fn deserialize_struct() {
        let input: Vec<u8> = vec![
            99, 1, 7, 2, 5, 102, 105, 114, 115, 116, 6, 115, 101, 99, 111, 110, 100, 1,
        ];

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
        let input: Vec<u8> = vec![2, 1, 97, 1, 1, 98, 2];
        let expected = Map::from([("a".to_string(), 1_u32), ("b".to_string(), 2_u32)]);

        assert_eq!(deserialize::<Map<String, u32>>(&input).unwrap(), expected);
    }

    #[test]
    fn deserialize_map_disordered_keys_fails() {
        let input: Vec<u8> = vec![2, 1, 98, 2, 1, 97, 1];
        let result = deserialize::<Map<String, u32>>(&input);
        assert!(result.is_err());
    }

    #[test]
    fn deserialize_map_duplicate_keys_fails() {
        let input: Vec<u8> = vec![2, 1, 97, 1, 1, 97, 2];
        let result = deserialize::<Map<String, u32>>(&input);
        assert!(result.is_err());
    }
}
