#[allow(unused_imports)]
use super::*;

/// a json object: an ordered string-keyed map of values.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Map {
    pub(super) inner: BTreeMap<String, Value>,
}

impl Map {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_capacity(_capacity: usize) -> Self {
        Self::default()
    }

    pub fn len(&self) -> usize {
        self.inner.len()
    }

    pub fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }

    pub fn clear(&mut self) {
        self.inner.clear();
    }

    pub fn get(&self, key: &str) -> Option<&Value> {
        self.inner.get(key)
    }

    pub fn get_mut(&mut self, key: &str) -> Option<&mut Value> {
        self.inner.get_mut(key)
    }

    pub fn contains_key(&self, key: &str) -> bool {
        self.inner.contains_key(key)
    }

    pub fn insert(&mut self, key: String, value: Value) -> Option<Value> {
        self.inner.insert(key, value)
    }

    pub fn remove(&mut self, key: &str) -> Option<Value> {
        self.inner.remove(key)
    }

    pub fn entry(&mut self, key: impl Into<String>) -> btree_map::Entry<'_, String, Value> {
        self.inner.entry(key.into())
    }

    pub fn keys(&self) -> btree_map::Keys<'_, String, Value> {
        self.inner.keys()
    }

    pub fn values(&self) -> btree_map::Values<'_, String, Value> {
        self.inner.values()
    }

    pub fn values_mut(&mut self) -> btree_map::ValuesMut<'_, String, Value> {
        self.inner.values_mut()
    }

    pub fn iter(&self) -> btree_map::Iter<'_, String, Value> {
        self.inner.iter()
    }

    pub fn iter_mut(&mut self) -> btree_map::IterMut<'_, String, Value> {
        self.inner.iter_mut()
    }
}

impl FromIterator<(String, Value)> for Map {
    fn from_iter<T: IntoIterator<Item = (String, Value)>>(iter: T) -> Self {
        Self {
            inner: iter.into_iter().collect(),
        }
    }
}

impl Extend<(String, Value)> for Map {
    fn extend<T: IntoIterator<Item = (String, Value)>>(&mut self, iter: T) {
        self.inner.extend(iter);
    }
}

impl IntoIterator for Map {
    type Item = (String, Value);
    type IntoIter = btree_map::IntoIter<String, Value>;

    fn into_iter(self) -> Self::IntoIter {
        self.inner.into_iter()
    }
}

impl<'a> IntoIterator for &'a Map {
    type Item = (&'a String, &'a Value);
    type IntoIter = btree_map::Iter<'a, String, Value>;

    fn into_iter(self) -> Self::IntoIter {
        self.inner.iter()
    }
}

impl<'a> IntoIterator for &'a mut Map {
    type Item = (&'a String, &'a mut Value);
    type IntoIter = btree_map::IterMut<'a, String, Value>;

    fn into_iter(self) -> Self::IntoIter {
        self.inner.iter_mut()
    }
}

impl std::ops::Index<&str> for Map {
    type Output = Value;

    fn index(&self, key: &str) -> &Value {
        static NULL: Value = Value::Null;
        self.inner.get(key).unwrap_or(&NULL)
    }
}

impl Serialize for Map {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(Some(self.inner.len()))?;
        for (key, value) in &self.inner {
            map.serialize_entry(key, value)?;
        }
        map.end()
    }
}

impl<'de> Deserialize<'de> for Map {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_map(MapVisitor)
    }
}
