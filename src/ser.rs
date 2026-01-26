use crate::result::{CordError, CordResult};
use integer_encoding::VarInt;
use serde::{ser, Serialize, Serializer};

pub fn serialize<T>(value: &T) -> CordResult<Vec<u8>>
where
    T: ?Sized + Serialize,
{
    let mut output = Vec::new();
    value.serialize(CordSerializer::new(&mut output))?;
    Ok(output)
}

struct CordSerializer<'a, W: ?Sized> {
    output: &'a mut W,
}

impl<'a, W> CordSerializer<'a, W>
where
    W: ?Sized + std::io::Write,
{
    fn new(output: &'a mut W) -> Self {
        Self { output }
    }

    fn serialize_usize(&mut self, v: usize) -> CordResult<()> {
        self.write_varint(v)
    }

    fn serialize_variant_index(&mut self, v: u32) -> CordResult<()> {
        self.write_varint(v)
    }

    fn write_varint<T: VarInt>(&mut self, v: T) -> CordResult<()> {
        self.output.write_all(&v.encode_var_vec())?;
        Ok(())
    }
}

macro_rules! serialize_varints {
    ($(($int:ty, $name:ident)),*) => {
        $(
            fn $name(mut self, v: $int) -> CordResult<()> {
                self.write_varint(v)
            }
        )*
    };
}

macro_rules! serialize_unsupported {
    ($(($type:ty, $name:ident)),*) => {
        $(
            fn $name(self, _v: $type) -> CordResult<()> {
                Err(CordError::NotSupported(stringify!($type)))
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
        self.serialize_u8(v.into())
    }

    serialize_varints!(
        (i8, serialize_i8),
        (i16, serialize_i16),
        (i32, serialize_i32),
        (i64, serialize_i64),
        (u8, serialize_u8),
        (u16, serialize_u16),
        (u32, serialize_u32),
        (u64, serialize_u64)
    );

    serialize_unsupported!(
        (f32, serialize_f32),
        (f64, serialize_f64),
        (char, serialize_char)
    );

    fn serialize_str(self, v: &str) -> CordResult<()> {
        self.serialize_bytes(v.as_bytes())
    }

    fn serialize_bytes(mut self, v: &[u8]) -> CordResult<()> {
        self.serialize_usize(v.len())?;
        self.output.write_all(v)?;
        Ok(())
    }

    fn serialize_none(self) -> CordResult<()> {
        self.serialize_u8(0)
    }

    fn serialize_some<T>(self, value: &T) -> CordResult<()>
    where
        T: ?Sized + Serialize,
    {
        self.output.write_all(&[1])?;
        value.serialize(self)
    }

    fn serialize_unit(self) -> CordResult<()> {
        Ok(())
    }

    #[allow(unused_mut)]
    fn serialize_unit_struct(mut self, _name: &'static str) -> CordResult<()> {
        self.serialize_unit()
    }

    #[allow(unused_mut)]
    fn serialize_unit_variant(
        mut self,
        _name: &'static str,
        variant_index: u32,
        _variant: &'static str,
    ) -> CordResult<()> {
        self.serialize_u32(variant_index)
    }

    #[allow(unused_mut)]
    fn serialize_newtype_struct<T>(mut self, _name: &'static str, value: &T) -> CordResult<()>
    where
        T: ?Sized + Serialize,
    {
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
            self.serialize_usize(len)?;
            Ok(self)
        } else {
            Err(CordError::NotSupported("unsized sequences"))
        }
    }

    fn serialize_tuple(mut self, len: usize) -> CordResult<Self::SerializeTuple> {
        self.serialize_usize(len)?;
        Ok(self)
    }

    #[allow(unused_mut)]
    fn serialize_tuple_struct(
        mut self,
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

    fn serialize_map(self, _len: Option<usize>) -> CordResult<Self::SerializeMap> {
        Ok(MapSerializer::new(self))
    }

    #[allow(unused_mut)]
    fn serialize_struct(
        mut self,
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

    fn serialize_field<T>(&mut self, _value: &T) -> CordResult<()>
    where
        T: ?Sized + Serialize,
    {
        Err(CordError::NotSupported("tuple struct"))
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
    entries: Vec<(Vec<u8>, Vec<u8>)>,
    next_key: Option<Vec<u8>>,
}

impl<'a, W: ?Sized> MapSerializer<'a, W> {
    fn new(serializer: CordSerializer<'a, W>) -> Self {
        MapSerializer {
            serializer,
            entries: Vec::new(),
            next_key: None,
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
        let mut key_bytes = Vec::new();
        key.serialize(CordSerializer::new(&mut key_bytes))?;
        self.next_key = Some(key_bytes);
        Ok(())
    }

    fn serialize_value<T>(&mut self, value: &T) -> CordResult<()>
    where
        T: ?Sized + Serialize,
    {
        let key_bytes = self.next_key.take().ok_or_else(|| {
            CordError::SerializationError("serialize_value called before serialize_key".into())
        })?;

        let mut value_bytes = Vec::new();
        value.serialize(CordSerializer::new(&mut value_bytes))?;

        self.entries.push((key_bytes, value_bytes));
        Ok(())
    }

    fn end(mut self) -> CordResult<()> {
        self.entries.sort_by(|(a, _), (b, _)| a.cmp(b));

        for i in 1..self.entries.len() {
            if self.entries[i - 1].0 == self.entries[i].0 {
                return Err(CordError::ValidationError("Duplicate keys in map"));
            }
        }

        self.serializer.serialize_usize(self.entries.len())?;
        for (key, value) in self.entries {
            self.serializer.output.write_all(&key)?;
            self.serializer.output.write_all(&value)?;
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
        serializer.serialize_bytes(self.to_vec().as_slice())
    }
}

impl<T: Serialize + std::clone::Clone + std::cmp::Ord> Serialize for crate::Set<T> {
    fn serialize<S>(&self, serializer: S) -> CordResult<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut vec: Vec<T> = self.into();
        vec.sort_by_key(|x| serialize(&x).unwrap());
        vec.serialize(serializer)
    }
}

impl ser::Serialize for crate::DateTime {
    fn serialize<S>(&self, serializer: S) -> CordResult<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_u64(self.chrono.timestamp_millis() as u64)
    }
}

impl<K: Serialize, V: Serialize> Serialize for crate::Map<K, V> {
    fn serialize<S>(&self, serializer: S) -> CordResult<S::Ok, S::Error>
    where
        S: Serializer,
    {
        use serde::ser::SerializeMap;
        let mut map = serializer.serialize_map(Some(self.inner.len()))?;
        for (k, v) in &self.inner {
            map.serialize_entry(k, v)?;
        }
        map.end()
    }
}

#[cfg(test)]
mod tests {
    use crate::{serialize, Bytes, CordError, DateTime, Map};
    use chrono::Utc;
    use integer_encoding::VarInt;
    use serde::Serialize;

    #[test]
    fn serialize_unit() {
        assert_eq!(serialize(&()).unwrap(), []);
    }

    #[test]
    fn serialize_booleans() {
        assert_eq!(serialize(&true).unwrap(), [1]);
        assert_eq!(serialize(&false).unwrap(), [0]);
    }

    #[test]
    fn serialize_numbers_as_varints() {
        let u8: u8 = 62;
        assert_eq!(serialize(&u8).unwrap(), [62]);
        assert_eq!(serialize(&u8).unwrap(), u8.encode_var_vec().as_slice());

        let i8: i8 = -30;
        assert_eq!(serialize(&i8).unwrap(), [59]);
        assert_eq!(serialize(&i8).unwrap(), i8.encode_var_vec().as_slice());

        let u32: u32 = 1293012;
        assert_eq!(serialize(&u32).unwrap(), [212, 245, 78]);
        assert_eq!(serialize(&u32).unwrap(), u32.encode_var_vec().as_slice());

        let i32: i32 = -1238470;
        assert_eq!(serialize(&i32).unwrap(), [139, 151, 151, 1]);
        assert_eq!(serialize(&i32).unwrap(), i32.encode_var_vec().as_slice());

        let u32_xs: u32 = 12;
        assert_eq!(serialize(&u32_xs).unwrap(), [12]);
        assert_eq!(
            serialize(&u32_xs).unwrap(),
            u32_xs.encode_var_vec().as_slice()
        );
    }

    #[test]
    fn serialize_strings() {
        assert_eq!(serialize("test").unwrap(), [4, 116, 101, 115, 116]);
    }

    #[test]
    fn serialize_empty_strings() {
        assert_eq!(serialize("").unwrap(), [0]);
    }

    #[test]
    fn serialize_large_bytearrays() {
        let length = 300;
        let value = vec![b'0'; length];

        let mut expected: Vec<u8> = serialize(&length).unwrap();
        expected.extend(vec![b'0'; length]);

        assert_eq!(serialize(&value).unwrap(), expected);
    }

    #[test]
    fn serialize_empty_bytearrays() {
        let value: Vec<u8> = vec![];
        assert_eq!(serialize(&value).unwrap(), [0]);
    }

    #[test]
    fn serialize_bytes() {
        let bytes = Bytes::from(vec![0, 1, 2]);
        assert_eq!(serialize(&bytes).unwrap(), vec![3, 0, 1, 2]);
    }

    #[test]
    fn serialize_raw_bytes() {
        let bytes: Vec<u8> = vec![0, 1, 2];
        assert_eq!(serialize(&bytes).unwrap(), [3, 0, 1, 2]);
    }

    #[test]
    fn serialize_tuple() {
        let bytes: [u8; 3] = [0, 1, 2];
        assert_eq!(serialize(&bytes).unwrap(), vec![3, 0, 1, 2]);
    }

    #[test]
    fn serialize_datetime() {
        let datetime: DateTime = chrono::DateTime::parse_from_rfc3339("2023-10-05T14:30:00.000Z")
            .unwrap()
            .with_timezone(&Utc)
            .into();

        assert_eq!(
            serialize(&datetime).unwrap(),
            vec![192, 172, 251, 129, 176, 49]
        );
    }

    #[test]
    fn serialize_set() {
        let set: crate::Set<String> = vec!["a", "b", "c", "d", "e", "f", "test"]
            .into_iter()
            .map(|s| s.to_string())
            .collect();

        assert_eq!(
            serialize(&set).unwrap(),
            [7, 1, 97, 1, 98, 1, 99, 1, 100, 1, 101, 1, 102, 4, 116, 101, 115, 116]
        );
    }

    #[derive(Debug, Serialize, PartialEq)]
    enum Enum {
        Unit,
        Container(u16),
        TupleContainer(u16, u16),
        Struct { field: u32 },
    }

    #[test]
    fn serialize_enum() {
        assert_eq!(serialize(&Enum::Unit).unwrap(), vec![0]);
        assert_eq!(serialize(&Enum::Container(1)).unwrap(), vec![1, 1]);
        assert_eq!(
            serialize(&Enum::TupleContainer(1, 2)).unwrap(),
            vec![2, 1, 2]
        );
        assert_eq!(serialize(&Enum::Struct { field: 1 }).unwrap(), vec![3, 1]);
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
        assert_eq!(
            serialize(&Struct {
                int: 99,
                option: Some(7_u8),
                seq: vec![String::from("first"), String::from("second")],
                boolean: true
            })
            .unwrap(),
            vec![
                99, // Serialize `int`
                1, 7, // Serialize `option`
                2, // Serialize the length of `seq`
                5, 102, 105, 114, 115, 116, // Serialize seq[0]
                6, 115, 101, 99, 111, 110, 100, // Serialize seq[1]
                1    // Serialize `boolean``
            ]
        );
    }

    #[test]
    fn serialize_unsupported_f64() {
        let value: f64 = 2.71828;
        assert_eq!(
            serialize(&value).unwrap_err(),
            CordError::NotSupported("f64")
        );
    }

    #[test]
    fn serialize_map() {
        let mut inner = std::collections::HashMap::new();
        inner.insert("aa".to_string(), 3_u32);
        inner.insert("b".to_string(), 2_u32);
        inner.insert("a".to_string(), 1_u32);

        let map = Map::from(inner);
        // "a" (1, 97) < "b" (1, 98) < "aa" (2, 97, 97)
        assert_eq!(
            serialize(&map).unwrap(),
            vec![3, 1, 97, 1, 1, 98, 2, 2, 97, 97, 3]
        );
    }

    #[test]
    fn serialize_unsupported_char() {
        let value: char = 'A';
        assert_eq!(
            serialize(&value).unwrap_err(),
            CordError::NotSupported("char")
        );
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
}
