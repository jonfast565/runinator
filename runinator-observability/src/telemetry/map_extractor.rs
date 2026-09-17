#[allow(unused_imports)]
use super::*;

pub(super) struct MapExtractor<'a>(pub(super) &'a TraceContext);

impl Extractor for MapExtractor<'_> {
    fn get(&self, key: &str) -> Option<&str> {
        self.0.get(key).map(String::as_str)
    }

    fn keys(&self) -> Vec<&str> {
        self.0.keys().map(String::as_str).collect()
    }
}
