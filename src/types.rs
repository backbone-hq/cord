#[cfg(any(feature = "datetime", feature = "decimal", feature = "uuid"))]
use crate::{CordError, CordResult};
#[cfg(feature = "decimal")]
use num_bigint::{BigInt, Sign};
#[cfg(feature = "decimal")]
use num_integer::Integer;
use std::collections::{HashMap, HashSet};
use std::hash::Hash;
#[cfg(any(feature = "datetime", feature = "decimal", feature = "uuid"))]
use std::str::FromStr;
/// Forward-compatible enum wrapper — unknown variant raw bytes are
/// preserved for round-tripping.
#[derive(Debug, Clone)]
pub enum Evolving<T> {
    Known(T),
    Unknown(Vec<u8>),
}

impl<T> Evolving<T> {
    pub fn new(value: T) -> Self {
        Evolving::Known(value)
    }

    pub fn known(&self) -> Option<&T> {
        match self {
            Evolving::Known(v) => Some(v),
            Evolving::Unknown(_) => None,
        }
    }

    pub fn into_known(self) -> Option<T> {
        match self {
            Evolving::Known(v) => Some(v),
            Evolving::Unknown(_) => None,
        }
    }

    pub fn is_known(&self) -> bool {
        matches!(self, Evolving::Known(_))
    }

    pub fn is_unknown(&self) -> bool {
        matches!(self, Evolving::Unknown(_))
    }

    pub fn unknown_bytes(&self) -> Option<&[u8]> {
        match self {
            Evolving::Unknown(bytes) => Some(bytes),
            Evolving::Known(_) => None,
        }
    }
}

impl<T: PartialEq> PartialEq for Evolving<T> {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Evolving::Known(a), Evolving::Known(b)) => a == b,
            (Evolving::Unknown(a), Evolving::Unknown(b)) => a == b,
            _ => false,
        }
    }
}

impl<T: Eq> Eq for Evolving<T> {}

impl<T: Hash> Hash for Evolving<T> {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        match self {
            Evolving::Known(v) => {
                0u8.hash(state);
                v.hash(state);
            }
            Evolving::Unknown(bytes) => {
                1u8.hash(state);
                bytes.hash(state);
            }
        }
    }
}

impl<T> From<T> for Evolving<T> {
    fn from(value: T) -> Self {
        Evolving::Known(value)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Bytes(pub(crate) Vec<u8>);

impl Bytes {
    pub fn to_vec(&self) -> Vec<u8> {
        self.0.clone()
    }
}

impl std::ops::Deref for Bytes {
    type Target = Vec<u8>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl std::ops::DerefMut for Bytes {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl From<Vec<u8>> for Bytes {
    fn from(vector: Vec<u8>) -> Self {
        Bytes(vector)
    }
}

impl From<Bytes> for Vec<u8> {
    fn from(bytes: Bytes) -> Self {
        bytes.0
    }
}

impl From<&Bytes> for Vec<u8> {
    fn from(bytes: &Bytes) -> Self {
        bytes.0.clone()
    }
}

/// A set with canonical (sorted) serialization.
#[derive(Debug, Clone)]
pub struct Set<T> {
    inner: HashSet<T>,
}

impl<T> Set<T> {
    /// Returns the number of elements in the set.
    pub fn len(&self) -> usize {
        self.inner.len()
    }

    /// Returns `true` if the set contains no elements.
    pub fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }

    /// Removes all elements from the set.
    pub fn clear(&mut self) {
        self.inner.clear();
    }

    /// Returns an iterator over the set elements.
    pub fn iter(&self) -> std::collections::hash_set::Iter<'_, T> {
        self.inner.iter()
    }
}

impl<T: Hash + Eq> Set<T> {
    /// Returns `true` if the set contains the given value.
    pub fn contains(&self, value: &T) -> bool {
        self.inner.contains(value)
    }

    /// Returns a reference to the value in the set, if any, that is equal to the given value.
    pub fn get(&self, value: &T) -> Option<&T> {
        self.inner.get(value)
    }

    /// Adds a value to the set. Returns whether the value was newly inserted.
    pub fn insert(&mut self, value: T) -> bool {
        self.inner.insert(value)
    }

    /// Removes a value from the set. Returns whether the value was present.
    pub fn remove(&mut self, value: &T) -> bool {
        self.inner.remove(value)
    }
}

impl<T> PartialEq for Set<T>
where
    HashSet<T>: PartialEq,
{
    fn eq(&self, other: &Self) -> bool {
        self.inner == other.inner
    }
}

impl<T> IntoIterator for Set<T> {
    type Item = T;
    type IntoIter = std::collections::hash_set::IntoIter<T>;

    fn into_iter(self) -> Self::IntoIter {
        self.inner.into_iter()
    }
}

impl<'a, T> IntoIterator for &'a Set<T> {
    type Item = &'a T;
    type IntoIter = std::collections::hash_set::Iter<'a, T>;

    fn into_iter(self) -> Self::IntoIter {
        self.inner.iter()
    }
}

impl<T: Clone> From<&Set<T>> for Vec<T> {
    fn from(set: &Set<T>) -> Self {
        set.inner.iter().cloned().collect()
    }
}

impl<T: Hash + Eq> From<HashSet<T>> for Set<T> {
    fn from(inner: HashSet<T>) -> Self {
        Self { inner }
    }
}

impl<T: Hash + Eq> From<std::collections::BTreeSet<T>> for Set<T> {
    fn from(btree: std::collections::BTreeSet<T>) -> Self {
        Self {
            inner: btree.into_iter().collect(),
        }
    }
}

impl<T: Hash + Eq> From<Set<T>> for HashSet<T> {
    fn from(set: Set<T>) -> Self {
        set.inner
    }
}

impl<T: Ord> From<Set<T>> for std::collections::BTreeSet<T> {
    fn from(set: Set<T>) -> Self {
        set.inner.into_iter().collect()
    }
}

impl<T> From<Vec<T>> for Set<T>
where
    T: Hash + Eq,
{
    fn from(vector: Vec<T>) -> Self {
        Set::from_iter(vector)
    }
}

impl<T: Hash + Eq> FromIterator<T> for Set<T> {
    fn from_iter<E: IntoIterator<Item = T>>(iter: E) -> Self {
        Set {
            inner: HashSet::from_iter(iter),
        }
    }
}

#[cfg(feature = "datetime")]
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd)]
pub struct DateTime {
    pub chrono: chrono::DateTime<chrono::Utc>,
}

#[cfg(feature = "datetime")]
impl DateTime {
    pub fn now() -> Self {
        Self {
            chrono: chrono::Utc::now(),
        }
    }
}

#[cfg(feature = "datetime")]
impl std::ops::Deref for DateTime {
    type Target = chrono::DateTime<chrono::Utc>;

    fn deref(&self) -> &Self::Target {
        &self.chrono
    }
}

#[cfg(feature = "datetime")]
impl std::ops::DerefMut for DateTime {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.chrono
    }
}

#[cfg(feature = "datetime")]
impl FromStr for DateTime {
    type Err = CordError;

    fn from_str(s: &str) -> CordResult<Self, Self::Err> {
        let chrono = chrono::DateTime::<chrono::Utc>::from_str(s)
            .map_err(|_| CordError::ValidationError("Failed to parse datetime"))?;
        Ok(Self { chrono })
    }
}

#[cfg(feature = "datetime")]
impl From<chrono::DateTime<chrono::Utc>> for DateTime {
    fn from(chrono: chrono::DateTime<chrono::Utc>) -> Self {
        Self { chrono }
    }
}

/// A map with canonical (key-sorted) serialization.
#[derive(Debug, Clone)]
pub struct Map<K, V> {
    inner: HashMap<K, V>,
}

impl<K, V> Map<K, V> {
    /// Returns the number of entries in the map.
    pub fn len(&self) -> usize {
        self.inner.len()
    }

    /// Returns `true` if the map contains no entries.
    pub fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }

    /// Removes all entries from the map.
    pub fn clear(&mut self) {
        self.inner.clear();
    }

    /// Returns an iterator over the key-value pairs.
    pub fn iter(&self) -> std::collections::hash_map::Iter<'_, K, V> {
        self.inner.iter()
    }

    /// Returns an iterator over the keys.
    pub fn keys(&self) -> std::collections::hash_map::Keys<'_, K, V> {
        self.inner.keys()
    }

    /// Returns an iterator over the values.
    pub fn values(&self) -> std::collections::hash_map::Values<'_, K, V> {
        self.inner.values()
    }
}

impl<K: Hash + Eq, V> Map<K, V> {
    /// Returns a reference to the value corresponding to the key.
    pub fn get(&self, key: &K) -> Option<&V> {
        self.inner.get(key)
    }

    /// Returns `true` if the map contains the given key.
    pub fn contains_key(&self, key: &K) -> bool {
        self.inner.contains_key(key)
    }

    /// Inserts a key-value pair into the map.
    /// If the map already had this key present, the value is updated and the old value is returned.
    pub fn insert(&mut self, key: K, value: V) -> Option<V> {
        self.inner.insert(key, value)
    }

    /// Removes a key from the map, returning the value if it was present.
    pub fn remove(&mut self, key: &K) -> Option<V> {
        self.inner.remove(key)
    }
}

impl<K, V> PartialEq for Map<K, V>
where
    K: Hash + Eq,
    V: PartialEq,
{
    fn eq(&self, other: &Self) -> bool {
        self.inner == other.inner
    }
}

impl<K, V> Eq for Map<K, V>
where
    K: Hash + Eq,
    V: Eq,
{
}

impl<K, V> IntoIterator for Map<K, V> {
    type Item = (K, V);
    type IntoIter = std::collections::hash_map::IntoIter<K, V>;

    fn into_iter(self) -> Self::IntoIter {
        self.inner.into_iter()
    }
}

impl<'a, K, V> IntoIterator for &'a Map<K, V> {
    type Item = (&'a K, &'a V);
    type IntoIter = std::collections::hash_map::Iter<'a, K, V>;

    fn into_iter(self) -> Self::IntoIter {
        self.inner.iter()
    }
}

impl<K: Hash + Eq, V> From<HashMap<K, V>> for Map<K, V> {
    fn from(inner: HashMap<K, V>) -> Self {
        Self { inner }
    }
}

impl<K: Hash + Eq, V> From<std::collections::BTreeMap<K, V>> for Map<K, V> {
    fn from(btree: std::collections::BTreeMap<K, V>) -> Self {
        Self {
            inner: btree.into_iter().collect(),
        }
    }
}

impl<K: Hash + Eq, V> From<Map<K, V>> for HashMap<K, V> {
    fn from(map: Map<K, V>) -> Self {
        map.inner
    }
}

impl<K: Ord, V> From<Map<K, V>> for std::collections::BTreeMap<K, V> {
    fn from(map: Map<K, V>) -> Self {
        map.inner.into_iter().collect()
    }
}

impl<K, V> FromIterator<(K, V)> for Map<K, V>
where
    K: Hash + Eq,
{
    fn from_iter<I: IntoIterator<Item = (K, V)>>(iter: I) -> Self {
        Self {
            inner: HashMap::from_iter(iter),
        }
    }
}

impl<K, V, const N: usize> From<[(K, V); N]> for Map<K, V>
where
    K: Hash + Eq,
{
    fn from(arr: [(K, V); N]) -> Self {
        Self {
            inner: HashMap::from_iter(arr),
        }
    }
}

/// Arbitrary-precision decimal number.
///
/// Wire format: `(u8 scale, Bytes two's-complement big-endian unscaled)`.
#[cfg(feature = "decimal")]
#[derive(Debug, Clone, Eq)]
pub struct Decimal {
    pub(crate) unscaled: BigInt,
    pub(crate) scale: u8,
}

#[cfg(feature = "decimal")]
impl Decimal {
    /// Create a new `Decimal` from an unscaled value and scale, normalizing it.
    pub fn new(unscaled: BigInt, scale: u8) -> Self {
        let mut d = Decimal { unscaled, scale };
        d.normalize();
        d
    }

    /// Create a `Decimal` from an `i64` with the given scale.
    pub fn from_i64(value: i64, scale: u8) -> Self {
        Self::new(BigInt::from(value), scale)
    }

    /// Returns the unscaled value.
    pub fn unscaled(&self) -> &BigInt {
        &self.unscaled
    }

    /// Returns the scale (number of fractional digits).
    pub fn scale(&self) -> u8 {
        self.scale
    }

    /// Normalize: strip trailing zeros from the unscaled value and adjust scale.
    fn normalize(&mut self) {
        if self.unscaled == BigInt::from(0) {
            self.unscaled = BigInt::from(0);
            self.scale = 0;
            return;
        }

        let ten = BigInt::from(10);
        while self.scale > 0 {
            let (quotient, remainder) = self.unscaled.div_rem(&ten);
            if remainder == BigInt::from(0) {
                self.unscaled = quotient;
                self.scale -= 1;
            } else {
                break;
            }
        }
    }
}

#[cfg(feature = "decimal")]
impl PartialEq for Decimal {
    fn eq(&self, other: &Self) -> bool {
        // Both are normalized, so direct comparison works.
        self.scale == other.scale && self.unscaled == other.unscaled
    }
}

#[cfg(feature = "decimal")]
impl std::hash::Hash for Decimal {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.unscaled.hash(state);
        self.scale.hash(state);
    }
}

#[cfg(feature = "decimal")]
impl std::fmt::Display for Decimal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.scale == 0 {
            write!(f, "{}", self.unscaled)
        } else {
            let (sign, bytes) = self.unscaled.to_bytes_be();
            let abs = num_bigint::BigUint::from_bytes_be(&bytes);
            let abs_str = abs.to_string();
            let sign_str = if sign == Sign::Minus { "-" } else { "" };
            let scale = self.scale as usize;
            if abs_str.len() <= scale {
                let zeros = scale - abs_str.len();
                write!(f, "{}0.{}{}", sign_str, "0".repeat(zeros), abs_str)
            } else {
                let (integer, fraction) = abs_str.split_at(abs_str.len() - scale);
                write!(f, "{}{}.{}", sign_str, integer, fraction)
            }
        }
    }
}

#[cfg(feature = "decimal")]
impl FromStr for Decimal {
    type Err = CordError;

    fn from_str(s: &str) -> CordResult<Self> {
        let s = s.trim();
        if let Some(dot_pos) = s.find('.') {
            let frac_len = s.len() - dot_pos - 1;
            let scale: u8 = frac_len
                .try_into()
                .map_err(|_| CordError::ValidationError("Decimal scale exceeds u8::MAX"))?;
            let without_dot: String = s.chars().filter(|c| *c != '.').collect();
            let unscaled = BigInt::from_str(&without_dot)
                .map_err(|_| CordError::ValidationError("Invalid decimal string"))?;
            Ok(Decimal::new(unscaled, scale))
        } else {
            let unscaled = BigInt::from_str(s)
                .map_err(|_| CordError::ValidationError("Invalid decimal string"))?;
            Ok(Decimal::new(unscaled, 0))
        }
    }
}

/// Canonical serialization wrapper for [`uuid::Uuid`].
#[cfg(feature = "uuid")]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Uuid {
    pub(crate) inner: uuid::Uuid,
}

#[cfg(feature = "uuid")]
impl Uuid {
    pub fn new(inner: uuid::Uuid) -> Self {
        Self { inner }
    }

    /// Return the inner [`uuid::Uuid`].
    pub fn into_inner(self) -> uuid::Uuid {
        self.inner
    }
}

#[cfg(feature = "uuid")]
impl std::ops::Deref for Uuid {
    type Target = uuid::Uuid;

    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

#[cfg(feature = "uuid")]
impl std::ops::DerefMut for Uuid {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.inner
    }
}

#[cfg(feature = "uuid")]
impl std::fmt::Display for Uuid {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.inner)
    }
}

#[cfg(feature = "uuid")]
impl FromStr for Uuid {
    type Err = CordError;

    fn from_str(s: &str) -> CordResult<Self> {
        let inner = uuid::Uuid::parse_str(s)
            .map_err(|_| CordError::ValidationError("Invalid UUID string"))?;
        Ok(Self { inner })
    }
}

#[cfg(feature = "uuid")]
impl From<uuid::Uuid> for Uuid {
    fn from(inner: uuid::Uuid) -> Self {
        Self { inner }
    }
}

#[cfg(feature = "uuid")]
impl From<Uuid> for uuid::Uuid {
    fn from(wrapper: Uuid) -> Self {
        wrapper.inner
    }
}

// Set<T>: Eq requires only T: Eq + Hash (HashSet<T> provides this unconditionally).
impl<T: Eq + Hash> Eq for Set<T> {}
