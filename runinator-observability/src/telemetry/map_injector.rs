#[allow(unused_imports)]
use super::*;

pub(super) struct MapInjector<'a>(pub(super) &'a mut TraceContext);

impl Injector for MapInjector<'_> {
    fn set(&mut self, key: &str, value: String) {
        self.0.insert(key.to_string(), value);
    }
}
