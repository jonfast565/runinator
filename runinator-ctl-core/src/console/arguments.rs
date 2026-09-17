#[allow(unused_imports)]
use super::*;

#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize)]
pub struct Arguments {
    pub args: Vec<String>,
    pub raw_args: Vec<String>,
    pub flags: BTreeMap<String, Vec<String>>,
    pub switches: Vec<String>,
}

impl Arguments {
    pub fn raw_after(&self, word: &str) -> Option<String> {
        let at = self.args.iter().position(|value| value == word)?;
        Some(self.raw_args[at + 1..].join(" "))
    }

    pub fn arg(&self, index: usize) -> Option<&str> {
        self.args.get(index).map(String::as_str)
    }

    pub fn required(&self, index: usize, name: &str) -> Result<&str, String> {
        self.arg(index)
            .filter(|value| !value.is_empty())
            .ok_or_else(|| format!("{name} is required"))
    }

    pub fn flag(&self, name: &str) -> Option<&str> {
        self.flags
            .get(name)
            .and_then(|values| values.last())
            .map(String::as_str)
    }

    pub fn flag_list(&self, name: &str) -> &[String] {
        self.flags.get(name).map(Vec::as_slice).unwrap_or_default()
    }

    pub fn is_set(&self, name: &str) -> bool {
        self.switches.iter().any(|candidate| candidate == name) || self.flags.contains_key(name)
    }
}
