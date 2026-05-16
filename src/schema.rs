use serde::{Deserialize, Serialize};

/// Width of a length prefix or variant index on the wire.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Width {
    /// 1 byte (u8)
    W8,
    /// 2 bytes (u16)
    W16,
    /// 4 bytes (u32) — the default
    W32,
    /// 8 bytes (u64)
    W64,
}

impl Width {
    /// Number of bytes this width occupies on the wire.
    pub fn bytes(self) -> u8 {
        match self {
            Width::W8 => 1,
            Width::W16 => 2,
            Width::W32 => 4,
            Width::W64 => 8,
        }
    }

    /// Convert from a byte count (1, 2, 4, 8) to a `Width`.
    ///
    /// Returns `None` for values other than 1, 2, 4, or 8.
    pub fn from_bytes(n: u8) -> Option<Width> {
        match n {
            1 => Some(Width::W8),
            2 => Some(Width::W16),
            4 => Some(Width::W32),
            8 => Some(Width::W64),
            _ => None,
        }
    }
}

/// Schema describes the wire format of a Cord-encoded value.
///
/// Schemas are themselves Cord-serializable, enabling canonical schema hashing.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Hash)]
pub enum Schema {
    // Unit
    Unit,
    // Primitives
    Bool,
    U8,
    U16,
    U32,
    U64,
    U128,
    I8,
    I16,
    I32,
    I64,
    I128,
    F32,
    F64,
    // Variable-length integer (wraps an integer schema)
    VarInt(Box<Schema>),
    // Length-prefixed string
    String(Width),
    // Length-prefixed byte array
    Bytes(Width),
    // DateTime: [i64 BE seconds][u32 BE nanos] = 12 bytes
    DateTime,
    // Decimal: [u8 scale][length-prefixed two's-complement big-endian unscaled bytes]
    Decimal(Width),
    // Uuid: exactly 16 raw bytes (no length prefix)
    Uuid,
    // Option: [1-byte discriminant][value if Some]
    Option(Box<Schema>),
    // Sequence: [width count][elements...]
    Seq(Box<Schema>, Width),
    // Tuple: [field1][field2]... (no length prefix)
    Tuple(Vec<Schema>),
    // Struct: positional fields, same wire format as Tuple
    // Names carried for schema identity / hashing, not on wire
    Struct(Vec<(std::string::String, Schema)>),
    // Enum: [width variant_index][payload per VariantSchema]
    Enum(Vec<(std::string::String, VariantSchema)>, Width),
    // Map: [width count][sorted key-value pairs]
    Map(Box<Schema>, Box<Schema>, Width),
    // Set: [width count][sorted elements]
    Set(Box<Schema>, Width),
    // Evolving: length-prefixed payload for forward compatibility
    Evolving(Box<Schema>, Width),
}

/// Describes the payload shape of an enum variant.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Hash)]
pub enum VariantSchema {
    Unit,
    Newtype(Schema),
    Tuple(Vec<Schema>),
    Struct(Vec<(std::string::String, Schema)>),
}

/// A dynamic value that can be encoded/decoded against a Schema.
///
/// Placeholder for feature-gated value variants when the feature is disabled.
/// Maintains stable discriminant indices regardless of enabled features.
#[derive(Clone, Debug, PartialEq)]
pub struct UnsupportedPayload(());

#[cfg(feature = "datetime")]
type DateTimePayload = chrono::DateTime<chrono::Utc>;
#[cfg(not(feature = "datetime"))]
type DateTimePayload = UnsupportedPayload;

#[cfg(feature = "decimal")]
type DecimalPayload = crate::Decimal;
#[cfg(not(feature = "decimal"))]
type DecimalPayload = UnsupportedPayload;

#[cfg(feature = "uuid")]
type UuidPayload = uuid::Uuid;
#[cfg(not(feature = "uuid"))]
type UuidPayload = UnsupportedPayload;

/// Value variants correspond to data types, not encoding concerns.
/// Encoding details (VarInt, Len8, etc.) live in the Schema.
#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    Unit,
    Bool(bool),
    U8(u8),
    U16(u16),
    U32(u32),
    U64(u64),
    U128(u128),
    I8(i8),
    I16(i16),
    I32(i32),
    I64(i64),
    I128(i128),
    F32(f32),
    F64(f64),
    String(std::string::String),
    Bytes(Vec<u8>),
    DateTime(DateTimePayload),
    Decimal(DecimalPayload),
    Uuid(UuidPayload),
    None,
    Some(Box<Value>),
    Seq(Vec<Value>),
    Tuple(Vec<Value>),
    Struct(Vec<(std::string::String, Value)>),
    Enum {
        variant_index: u32,
        variant_name: std::string::String,
        payload: Box<VariantValue>,
    },
    Map(Vec<(Value, Value)>),
    Set(Vec<Value>),
    /// Raw bytes from an unknown Evolving payload
    UnknownEvolving(Vec<u8>),
}

/// Payload of an enum variant in a dynamic Value.
#[derive(Clone, Debug, PartialEq)]
pub enum VariantValue {
    Unit,
    Newtype(Value),
    Tuple(Vec<Value>),
    Struct(Vec<(std::string::String, Value)>),
}

/// `Value` implements `serde::Serialize` so that `cord::serialize(&value)` produces
/// the same canonical bytes as the equivalent typed Rust value would.
///
/// Each variant serializes as its natural serde type:
/// - Primitives call the corresponding `serialize_*` method
/// - `DateTime`, `Decimal`, `Uuid`, and `Set` use the same sentinel-based
///   serialization as their typed counterparts (`crate::DateTime`, etc.)
/// - `Struct` serializes positionally (like Cord structs on the wire)
/// - `Enum` serializes with its variant index
/// - `Map` serializes as a serde map (the `CordSerializer` handles sorting)
///
/// # Deserialize
///
/// `Value` does **not** implement `serde::Deserialize` because Cord is not a
/// self-describing format. The wire bytes contain no type tags, so the
/// deserializer cannot determine which `Value` variant to produce without an
/// external `Schema`. Use [`crate::dynamic::decode`] with a `Schema` instead.
impl Serialize for Value {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::{SerializeMap, SerializeSeq, SerializeStruct, SerializeTuple};
        match self {
            Value::Unit => serializer.serialize_unit(),
            Value::Bool(v) => serializer.serialize_bool(*v),
            Value::U8(v) => serializer.serialize_u8(*v),
            Value::U16(v) => serializer.serialize_u16(*v),
            Value::U32(v) => serializer.serialize_u32(*v),
            Value::U64(v) => serializer.serialize_u64(*v),
            Value::U128(v) => serializer.serialize_u128(*v),
            Value::I8(v) => serializer.serialize_i8(*v),
            Value::I16(v) => serializer.serialize_i16(*v),
            Value::I32(v) => serializer.serialize_i32(*v),
            Value::I64(v) => serializer.serialize_i64(*v),
            Value::I128(v) => serializer.serialize_i128(*v),
            Value::F32(v) => serializer.serialize_f32(*v),
            Value::F64(v) => serializer.serialize_f64(*v),
            Value::String(v) => serializer.serialize_str(v),
            Value::Bytes(v) => serializer.serialize_bytes(v),
            #[cfg(feature = "datetime")]
            Value::DateTime(dt) => crate::DateTime::from(*dt).serialize(serializer),
            #[cfg(not(feature = "datetime"))]
            Value::DateTime(_) => Err(serde::ser::Error::custom(
                "DateTime variant requires the `datetime` feature",
            )),
            #[cfg(feature = "decimal")]
            Value::Decimal(d) => d.serialize(serializer),
            #[cfg(not(feature = "decimal"))]
            Value::Decimal(_) => Err(serde::ser::Error::custom(
                "Decimal variant requires the `decimal` feature",
            )),
            #[cfg(feature = "uuid")]
            Value::Uuid(u) => crate::Uuid::from(*u).serialize(serializer),
            #[cfg(not(feature = "uuid"))]
            Value::Uuid(_) => Err(serde::ser::Error::custom(
                "Uuid variant requires the `uuid` feature",
            )),
            Value::None => serializer.serialize_none(),
            Value::Some(inner) => serializer.serialize_some(inner.as_ref()),
            Value::Seq(items) => {
                let mut seq = serializer.serialize_seq(Some(items.len()))?;
                for item in items {
                    seq.serialize_element(item)?;
                }
                seq.end()
            }
            Value::Tuple(items) => {
                let mut tup = serializer.serialize_tuple(items.len())?;
                for item in items {
                    tup.serialize_element(item)?;
                }
                tup.end()
            }
            Value::Struct(fields) => {
                // Cord structs are positional on the wire — field names don't appear.
                // We use serialize_struct so the CordSerializer treats it correctly.
                // The CordSerializer ignores field names, so we pass a fixed empty string
                // rather than leaking memory with Box::leak.
                let mut s = serializer.serialize_struct("", fields.len())?;
                for (_name, value) in fields {
                    s.serialize_field("", value)?;
                }
                s.end()
            }
            Value::Enum {
                variant_index,
                variant_name: _,
                payload,
            } => {
                // The CordSerializer ignores variant names (only the index matters),
                // so we pass a fixed empty string rather than leaking memory with Box::leak.
                payload.serialize_variant(serializer, *variant_index, "")
            }
            Value::Map(entries) => {
                let mut map = serializer.serialize_map(Some(entries.len()))?;
                for (k, v) in entries {
                    map.serialize_entry(k, v)?;
                }
                map.end()
            }
            Value::Set(items) => {
                use crate::private::SENTINEL_SET;
                // Wrap in SENTINEL_SET for consistency with typed Set<T>.
                serializer.serialize_newtype_struct(SENTINEL_SET, &SetItems(items))
            }
            Value::UnknownEvolving(bytes) => {
                // Raw bytes — serialize as-is (byte slice)
                serializer.serialize_bytes(bytes)
            }
        }
    }
}

/// Helper for serializing Value::Set items through the SENTINEL_SET sentinel.
struct SetItems<'a>(&'a [Value]);

impl Serialize for SetItems<'_> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeSeq;
        let mut seq = serializer.serialize_seq(Some(self.0.len()))?;
        for item in self.0 {
            seq.serialize_element(item)?;
        }
        seq.end()
    }
}

impl VariantValue {
    /// Serialize this variant payload through a serde Serializer using the given
    /// variant index and name. This is called by `Value::Serialize` for the
    /// `Value::Enum` variant.
    fn serialize_variant<S>(
        &self,
        serializer: S,
        variant_index: u32,
        variant_name: &'static str,
    ) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::{SerializeStructVariant, SerializeTupleVariant};
        match self {
            VariantValue::Unit => {
                serializer.serialize_unit_variant("", variant_index, variant_name)
            }
            VariantValue::Newtype(value) => {
                serializer.serialize_newtype_variant("", variant_index, variant_name, value)
            }
            VariantValue::Tuple(items) => {
                let mut tv = serializer.serialize_tuple_variant(
                    "",
                    variant_index,
                    variant_name,
                    items.len(),
                )?;
                for item in items {
                    tv.serialize_field(item)?;
                }
                tv.end()
            }
            VariantValue::Struct(fields) => {
                let mut sv = serializer.serialize_struct_variant(
                    "",
                    variant_index,
                    variant_name,
                    fields.len(),
                )?;
                for (_name, value) in fields {
                    sv.serialize_field("", value)?;
                }
                sv.end()
            }
        }
    }
}

impl Default for Width {
    fn default() -> Self {
        Width::W32
    }
}

impl Schema {
    pub fn string() -> Self {
        Schema::String(Width::default())
    }

    pub fn bytes() -> Self {
        Schema::Bytes(Width::default())
    }

    pub fn decimal() -> Self {
        Schema::Decimal(Width::default())
    }

    pub fn uuid() -> Self {
        Schema::Uuid
    }

    pub fn option(inner: Schema) -> Self {
        Schema::Option(Box::new(inner))
    }

    pub fn seq(element: Schema) -> Self {
        Schema::Seq(Box::new(element), Width::default())
    }

    pub fn r#enum(variants: Vec<(std::string::String, VariantSchema)>) -> Self {
        Schema::Enum(variants, Width::default())
    }

    pub fn map(key: Schema, value: Schema) -> Self {
        Schema::Map(Box::new(key), Box::new(value), Width::default())
    }

    pub fn set(element: Schema) -> Self {
        Schema::Set(Box::new(element), Width::default())
    }

    pub fn varint(inner: Schema) -> Self {
        Schema::VarInt(Box::new(inner))
    }

    pub fn evolving(inner: Schema) -> Self {
        Schema::Evolving(Box::new(inner), Width::default())
    }
}

/// Override the width of a schema's outermost length prefix or variant index.
///
/// Used by `#[derive(Cord)]` to apply `#[cord(width = N)]` attributes.
pub fn with_width(schema: Schema, width: Width) -> Schema {
    match schema {
        Schema::String(_) => Schema::String(width),
        Schema::Bytes(_) => Schema::Bytes(width),
        Schema::Seq(elem, _) => Schema::Seq(elem, width),
        Schema::Enum(variants, _) => Schema::Enum(variants, width),
        Schema::Map(k, v, _) => Schema::Map(k, v, width),
        Schema::Set(elem, _) => Schema::Set(elem, width),
        Schema::Decimal(_) => Schema::Decimal(width),
        Schema::Evolving(inner, _) => Schema::Evolving(inner, width),
        // For types without a width component, return as-is
        other => other,
    }
}

/// Derive the canonical [`Schema`] for a Rust type.
///
/// Implemented automatically by `#[derive(Cord)]` and manually for primitive types.
/// This enables schema-aware operations (hashing, cross-language exchange) from
/// typed Rust code.
pub trait CordSchema {
    /// Returns the canonical schema for this type.
    fn schema() -> Schema;
}

impl CordSchema for () {
    fn schema() -> Schema {
        Schema::Unit
    }
}
impl CordSchema for bool {
    fn schema() -> Schema {
        Schema::Bool
    }
}
impl CordSchema for u8 {
    fn schema() -> Schema {
        Schema::U8
    }
}
impl CordSchema for u16 {
    fn schema() -> Schema {
        Schema::U16
    }
}
impl CordSchema for u32 {
    fn schema() -> Schema {
        Schema::U32
    }
}
impl CordSchema for u64 {
    fn schema() -> Schema {
        Schema::U64
    }
}
impl CordSchema for u128 {
    fn schema() -> Schema {
        Schema::U128
    }
}
impl CordSchema for i8 {
    fn schema() -> Schema {
        Schema::I8
    }
}
impl CordSchema for i16 {
    fn schema() -> Schema {
        Schema::I16
    }
}
impl CordSchema for i32 {
    fn schema() -> Schema {
        Schema::I32
    }
}
impl CordSchema for i64 {
    fn schema() -> Schema {
        Schema::I64
    }
}
impl CordSchema for i128 {
    fn schema() -> Schema {
        Schema::I128
    }
}
impl CordSchema for f32 {
    fn schema() -> Schema {
        Schema::F32
    }
}
impl CordSchema for f64 {
    fn schema() -> Schema {
        Schema::F64
    }
}
impl CordSchema for String {
    fn schema() -> Schema {
        Schema::string()
    }
}
impl CordSchema for str {
    fn schema() -> Schema {
        Schema::string()
    }
}

impl<T: CordSchema> CordSchema for Option<T> {
    fn schema() -> Schema {
        Schema::option(T::schema())
    }
}

impl<T: CordSchema> CordSchema for Vec<T> {
    fn schema() -> Schema {
        Schema::seq(T::schema())
    }
}

/// Trait for converting Rust values into [`Value`].
///
/// Used by the [`cord_value!`](crate::cord_value) macro to convert literals
/// into dynamic values. Rust's type inference determines which impl is
/// selected — unsuffixed integer literals default to `i32`.
pub trait IntoValue {
    fn into_value(self) -> Value;
}

impl IntoValue for bool {
    fn into_value(self) -> Value {
        Value::Bool(self)
    }
}

impl IntoValue for u8 {
    fn into_value(self) -> Value {
        Value::U8(self)
    }
}

impl IntoValue for u16 {
    fn into_value(self) -> Value {
        Value::U16(self)
    }
}

impl IntoValue for u32 {
    fn into_value(self) -> Value {
        Value::U32(self)
    }
}

impl IntoValue for u64 {
    fn into_value(self) -> Value {
        Value::U64(self)
    }
}

impl IntoValue for u128 {
    fn into_value(self) -> Value {
        Value::U128(self)
    }
}

impl IntoValue for i8 {
    fn into_value(self) -> Value {
        Value::I8(self)
    }
}

impl IntoValue for i16 {
    fn into_value(self) -> Value {
        Value::I16(self)
    }
}

impl IntoValue for i32 {
    fn into_value(self) -> Value {
        Value::I32(self)
    }
}

impl IntoValue for i64 {
    fn into_value(self) -> Value {
        Value::I64(self)
    }
}

impl IntoValue for i128 {
    fn into_value(self) -> Value {
        Value::I128(self)
    }
}

impl IntoValue for f32 {
    fn into_value(self) -> Value {
        Value::F32(self)
    }
}

impl IntoValue for f64 {
    fn into_value(self) -> Value {
        Value::F64(self)
    }
}

impl IntoValue for &str {
    fn into_value(self) -> Value {
        Value::String(self.to_string())
    }
}

impl IntoValue for std::string::String {
    fn into_value(self) -> Value {
        Value::String(self)
    }
}

#[cfg(feature = "decimal")]
impl IntoValue for crate::Decimal {
    fn into_value(self) -> Value {
        Value::Decimal(self)
    }
}

#[cfg(feature = "uuid")]
impl IntoValue for uuid::Uuid {
    fn into_value(self) -> Value {
        Value::Uuid(self)
    }
}

impl IntoValue for Value {
    fn into_value(self) -> Value {
        self
    }
}

/// Infer a [`Schema`] from a [`Value`]'s structure.
///
/// Sequences, maps, and sets infer their element/key/value schema from
/// the first element; empty collections use `Unit` as the element schema.
pub fn infer_schema(value: &Value) -> Schema {
    match value {
        Value::Unit => Schema::Unit,
        Value::Bool(_) => Schema::Bool,
        Value::U8(_) => Schema::U8,
        Value::U16(_) => Schema::U16,
        Value::U32(_) => Schema::U32,
        Value::U64(_) => Schema::U64,
        Value::U128(_) => Schema::U128,
        Value::I8(_) => Schema::I8,
        Value::I16(_) => Schema::I16,
        Value::I32(_) => Schema::I32,
        Value::I64(_) => Schema::I64,
        Value::I128(_) => Schema::I128,
        Value::F32(_) => Schema::F32,
        Value::F64(_) => Schema::F64,
        Value::String(_) => Schema::string(),
        Value::Bytes(_) => Schema::bytes(),
        Value::DateTime(_) => Schema::DateTime,
        Value::Decimal(_) => Schema::decimal(),
        Value::Uuid(_) => Schema::uuid(),
        Value::None => Schema::option(Schema::Unit),
        Value::Some(inner) => Schema::option(infer_schema(inner)),
        Value::Seq(items) => {
            let elem = items.first().map(infer_schema).unwrap_or(Schema::Unit);
            Schema::seq(elem)
        }
        Value::Tuple(items) => Schema::Tuple(items.iter().map(infer_schema).collect()),
        Value::Struct(fields) => Schema::Struct(
            fields
                .iter()
                .map(|(name, val)| (name.clone(), infer_schema(val)))
                .collect(),
        ),
        Value::Enum {
            variant_index,
            variant_name,
            payload,
            ..
        } => {
            let variant_schema = infer_variant_schema(payload);
            // We only know about this one variant; build an enum schema with
            // placeholder Unit variants for indices below it.
            let mut variants: Vec<(std::string::String, VariantSchema)> = (0..*variant_index)
                .map(|i| (format!("__unknown_{}", i), VariantSchema::Unit))
                .collect();
            variants.push((variant_name.clone(), variant_schema));
            Schema::r#enum(variants)
        }
        Value::Map(entries) => {
            let (key_s, val_s) = entries
                .first()
                .map(|(k, v)| (infer_schema(k), infer_schema(v)))
                .unwrap_or((Schema::Unit, Schema::Unit));
            Schema::map(key_s, val_s)
        }
        Value::Set(items) => {
            let elem = items.first().map(infer_schema).unwrap_or(Schema::Unit);
            Schema::set(elem)
        }
        Value::UnknownEvolving(_) => Schema::evolving(Schema::Unit),
    }
}

fn infer_variant_schema(payload: &VariantValue) -> VariantSchema {
    match payload {
        VariantValue::Unit => VariantSchema::Unit,
        VariantValue::Newtype(val) => VariantSchema::Newtype(infer_schema(val)),
        VariantValue::Tuple(items) => {
            VariantSchema::Tuple(items.iter().map(infer_schema).collect())
        }
        VariantValue::Struct(fields) => VariantSchema::Struct(
            fields
                .iter()
                .map(|(name, val)| (name.clone(), infer_schema(val)))
                .collect(),
        ),
    }
}
