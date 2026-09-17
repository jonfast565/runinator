#[allow(unused_imports)]
use super::*;

/// a json pointer relative to a `LocationBase`. `path` is a sequence of object keys.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FieldLocation {
    pub base: LocationBase,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub path: Vec<String>,
}

impl FieldLocation {
    pub(super) fn new(base: LocationBase, path: &[&str]) -> Self {
        Self {
            base,
            path: path.iter().map(|segment| (*segment).to_string()).collect(),
        }
    }

    pub fn parameters(path: &[&str]) -> Self {
        Self::new(LocationBase::Parameters, path)
    }

    pub fn wait(path: &[&str]) -> Self {
        Self::new(LocationBase::Wait, path)
    }

    pub fn condition(path: &[&str]) -> Self {
        Self::new(LocationBase::Condition, path)
    }

    pub fn action(path: &[&str]) -> Self {
        Self::new(LocationBase::Action, path)
    }

    pub fn transitions(path: &[&str]) -> Self {
        Self::new(LocationBase::Transitions, path)
    }

    pub fn top_level(key: &str) -> Self {
        Self::new(LocationBase::TopLevel, &[key])
    }
}
