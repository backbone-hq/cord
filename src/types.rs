use crate::{CordError, CordResult};
use std::collections::HashSet;
use std::hash::Hash;
use std::iter::FromIterator;
use std::str::FromStr;

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

#[derive(Debug, Clone)]
pub struct Set<T> {
    pub hashset: HashSet<T>,
}

impl<T> PartialEq for Set<T>
where
    HashSet<T>: PartialEq,
{
    fn eq(&self, other: &Self) -> bool {
        self.hashset == other.hashset
    }
}

impl<T> std::ops::Deref for Set<T> {
    type Target = HashSet<T>;

    fn deref(&self) -> &Self::Target {
        &self.hashset
    }
}

impl<T> std::ops::DerefMut for Set<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.hashset
    }
}

impl<T: Clone> From<&Set<T>> for Vec<T> {
    fn from(set: &Set<T>) -> Self {
        set.hashset.iter().cloned().collect()
    }
}

impl<T> From<HashSet<T>> for Set<T> {
    fn from(hashset: HashSet<T>) -> Self {
        Self { hashset }
    }
}

impl<T> From<Set<T>> for HashSet<T> {
    fn from(set: Set<T>) -> Self {
        set.hashset
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
        Set::from(HashSet::from_iter(iter))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd)]
pub struct DateTime {
    pub chrono: chrono::DateTime<chrono::Utc>,
}

impl DateTime {
    pub fn now() -> Self {
        Self {
            chrono: chrono::Utc::now(),
        }
    }
}

impl std::ops::Deref for DateTime {
    type Target = chrono::DateTime<chrono::Utc>;

    fn deref(&self) -> &Self::Target {
        &self.chrono
    }
}

impl std::ops::DerefMut for DateTime {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.chrono
    }
}

impl FromStr for DateTime {
    type Err = CordError;

    fn from_str(s: &str) -> CordResult<Self, Self::Err> {
        let chrono = chrono::DateTime::<chrono::Utc>::from_str(s)
            .map_err(|_| CordError::ValidationError("Failed to parse datetime"))?;
        Ok(Self { chrono })
    }
}

impl From<chrono::DateTime<chrono::Utc>> for DateTime {
    fn from(chrono: chrono::DateTime<chrono::Utc>) -> Self {
        Self { chrono }
    }
}

#[derive(Debug, Clone)]
pub struct Map<K, V> {
    pub(crate) inner: std::collections::HashMap<K, V>,
}

impl<K, V> PartialEq for Map<K, V>
where
    K: Eq + Hash,
    V: PartialEq,
{
    fn eq(&self, other: &Self) -> bool {
        self.inner == other.inner
    }
}

impl<K, V> Eq for Map<K, V>
where
    K: Eq + Hash,
    V: Eq,
{
}

impl<K, V> std::ops::Deref for Map<K, V> {
    type Target = std::collections::HashMap<K, V>;

    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

impl<K, V> std::ops::DerefMut for Map<K, V> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.inner
    }
}

impl<K, V> From<std::collections::HashMap<K, V>> for Map<K, V> {
    fn from(inner: std::collections::HashMap<K, V>) -> Self {
        Self { inner }
    }
}

impl<K, V> From<Map<K, V>> for std::collections::HashMap<K, V> {
    fn from(map: Map<K, V>) -> Self {
        map.inner
    }
}

impl<K, V> FromIterator<(K, V)> for Map<K, V>
where
    K: Eq + Hash,
{
    fn from_iter<I: IntoIterator<Item = (K, V)>>(iter: I) -> Self {
        Self {
            inner: std::collections::HashMap::from_iter(iter),
        }
    }
}
impl<K, V, const N: usize> From<[(K, V); N]> for Map<K, V>
where
    K: Eq + Hash,
{
    fn from(arr: [(K, V); N]) -> Self {
        Self {
            inner: std::collections::HashMap::from_iter(arr),
        }
    }
}
