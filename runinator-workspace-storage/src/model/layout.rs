#[allow(unused_imports)]
use super::*;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Layout {
    pub page_size: u32,
    pub min: u32,
    pub avg: u32,
    pub max: u32,
}

impl Default for Layout {
    fn default() -> Self {
        Self {
            page_size: 4 * 1024 * 1024,
            min: 64 * 1024,
            avg: 256 * 1024,
            max: 1024 * 1024,
        }
    }
}

impl Layout {
    /// Select a page size before ingestion. It remains fixed for this file.
    /// A caller with an unknown input length uses the default 4 MiB layout.
    pub fn for_size(size: u64) -> Self {
        let page_size = if size < 16 * 1024 * 1024 {
            1024 * 1024
        } else if size < 1024 * 1024 * 1024 {
            4 * 1024 * 1024
        } else {
            8 * 1024 * 1024
        };
        Self {
            page_size,
            ..Self::default()
        }
    }

    pub fn validate(&self) -> Result<()> {
        if !self.page_size.is_power_of_two() || !(4096..=8 * 1024 * 1024).contains(&self.page_size)
        {
            return Err(invalid(
                "page size must be a power of two from 4 KiB through 8 MiB",
            ));
        }
        if !(64..=1_048_576).contains(&self.min)
            || !(256..=4_194_304).contains(&self.avg)
            || !(1024..=16_777_216).contains(&self.max)
            || self.min > self.avg
            || self.avg > self.max
            || self.max > self.page_size
        {
            return Err(invalid("invalid FastCDC sizes"));
        }
        Ok(())
    }
    pub(super) fn write(&self, e: &mut Encoder) {
        e.u32(self.page_size);
        e.u32(self.min);
        e.u32(self.avg);
        e.u32(self.max);
        e.u8(1);
        e.u64(0);
    }
    pub(super) fn read(d: &mut Decoder<'_>) -> Result<Self> {
        let value = Self {
            page_size: d.u32()?,
            min: d.u32()?,
            avg: d.u32()?,
            max: d.u32()?,
        };
        if d.u8()? != 1 || d.u64()? != 0 {
            return Err(corrupt("unsupported FastCDC normalization/seed"));
        }
        value.validate()?;
        Ok(value)
    }
}
