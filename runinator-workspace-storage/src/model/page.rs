#[allow(unused_imports)]
use super::*;

#[derive(Clone, Debug)]
pub struct Page {
    pub page_size: u32,
    pub used: u32,
    pub extents: Vec<PageExtent>,
}

impl Page {
    pub(super) fn validate(&self) -> Result<()> {
        if !(4096..=8 * 1024 * 1024).contains(&self.page_size)
            || !self.page_size.is_power_of_two()
            || self.used == 0
            || self.used > self.page_size
            || self.extents.is_empty()
            || self.extents.len() > 131072
        {
            return Err(corrupt("invalid page manifest"));
        }
        let mut total = 0u64;
        let mut previous_zero = false;
        for extent in &self.extents {
            if extent.is_empty() {
                return Err(corrupt("empty page extent"));
            }
            let zero = matches!(extent, PageExtent::Zero(_));
            if zero && (previous_zero || extent.len() < ZERO_RUN_MIN as u32) {
                return Err(corrupt("noncanonical zero extent"));
            }
            previous_zero = zero;
            total += extent.len() as u64;
        }
        if previous_zero || total != self.used as u64 {
            return Err(corrupt("invalid extent lengths or explicit trailing zeros"));
        }
        Ok(())
    }
}

impl Binary for Page {
    fn encode(&self) -> Result<Vec<u8>> {
        self.validate()?;
        let mut e = Encoder::new();
        e.u32(self.page_size);
        e.u32(self.used);
        e.u32(self.extents.len() as u32);
        for extent in &self.extents {
            match extent {
                PageExtent::Data(c) => {
                    e.u8(1);
                    e.id(c.id);
                    e.u32(c.len);
                }
                PageExtent::Zero(n) => {
                    e.u8(0);
                    e.u32(*n);
                }
            }
        }
        e.finish()
    }
    fn decode(b: &[u8]) -> Result<Self> {
        let mut d = Decoder::new(b)?;
        let page_size = d.u32()?;
        let used = d.u32()?;
        let n = d.u32()?;
        if n == 0 || n > 131072 {
            return Err(corrupt("invalid extent count"));
        }
        let mut extents = Vec::new();
        for _ in 0..n {
            extents.push(match d.u8()? {
                0 => PageExtent::Zero(d.u32()?),
                1 => PageExtent::Data(ChunkRef {
                    id: d.id()?,
                    len: d.u32()?,
                }),
                _ => return Err(corrupt("unknown page extent tag")),
            });
        }
        d.finish()?;
        let value = Self {
            page_size,
            used,
            extents,
        };
        value.validate()?;
        Ok(value)
    }
}
