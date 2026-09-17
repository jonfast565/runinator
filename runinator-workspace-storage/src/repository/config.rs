#[allow(unused_imports)]
use super::*;

#[derive(Clone, Debug)]
pub struct Config {
    pub layout: Layout,
    pub page_cache_bytes: usize,
    pub metadata_cache_bytes: usize,
    pub chunk_cache_bytes: usize,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            layout: Layout::default(),
            page_cache_bytes: 64 * 1024 * 1024,
            metadata_cache_bytes: 16 * 1024 * 1024,
            chunk_cache_bytes: 32 * 1024 * 1024,
        }
    }
}

impl Config {
    pub(super) fn validate(&self) -> Result<()> {
        self.layout.validate()?;
        if self.page_cache_bytes < self.layout.page_size as usize
            || self.metadata_cache_bytes < crate::model::TINY_LIMIT + 128
            || self.chunk_cache_bytes < self.layout.max as usize
        {
            return Err(invalid("cache budgets are too small for this layout"));
        }
        Ok(())
    }
}
