#[allow(unused_imports)]
use super::*;

pub trait Index: private::Sealed {
    #[doc(hidden)]
    fn index_into<'v>(&self, value: &'v Value) -> Option<&'v Value>;
    #[doc(hidden)]
    fn index_into_mut<'v>(&self, value: &'v mut Value) -> Option<&'v mut Value>;
}

impl Index for usize {
    fn index_into<'v>(&self, value: &'v Value) -> Option<&'v Value> {
        match value {
            Value::Array(list) => list.get(*self),
            _ => None,
        }
    }

    fn index_into_mut<'v>(&self, value: &'v mut Value) -> Option<&'v mut Value> {
        match value {
            Value::Array(list) => list.get_mut(*self),
            _ => None,
        }
    }
}

impl Index for str {
    fn index_into<'v>(&self, value: &'v Value) -> Option<&'v Value> {
        match value {
            Value::Object(map) => map.get(self),
            _ => None,
        }
    }

    fn index_into_mut<'v>(&self, value: &'v mut Value) -> Option<&'v mut Value> {
        match value {
            Value::Object(map) => map.get_mut(self),
            _ => None,
        }
    }
}

impl Index for String {
    fn index_into<'v>(&self, value: &'v Value) -> Option<&'v Value> {
        self.as_str().index_into(value)
    }

    fn index_into_mut<'v>(&self, value: &'v mut Value) -> Option<&'v mut Value> {
        self.as_str().index_into_mut(value)
    }
}

impl<T> Index for &T
where
    T: ?Sized + Index,
{
    fn index_into<'v>(&self, value: &'v Value) -> Option<&'v Value> {
        (**self).index_into(value)
    }

    fn index_into_mut<'v>(&self, value: &'v mut Value) -> Option<&'v mut Value> {
        (**self).index_into_mut(value)
    }
}

impl<I: Index> std::ops::Index<I> for Value {
    type Output = Value;

    fn index(&self, index: I) -> &Value {
        static NULL: Value = Value::Null;
        index.index_into(self).unwrap_or(&NULL)
    }
}
