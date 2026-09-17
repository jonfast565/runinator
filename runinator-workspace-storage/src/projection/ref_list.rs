#[allow(unused_imports)]
use super::*;

/// Canonically sorted set of structural PathRef object IDs.
#[derive(Clone, Debug)]
pub struct RefList {
    pub refs: Vec<Id>,
}

impl Binary for RefList {
    fn encode(&self) -> Result<Vec<u8>> {
        if self.refs.len() < 2 || self.refs.len() > 1_000_000 {
            return Err(invalid("invalid hard-link ref list"));
        }
        let mut e = Encoder::new();
        e.u32(self.refs.len() as u32);
        let mut previous: Option<Id> = None;
        for id in &self.refs {
            if previous.is_some_and(|p| p >= *id) {
                return Err(invalid("noncanonical hard-link ref list"));
            }
            e.id(*id);
            previous = Some(*id);
        }
        e.finish()
    }

    fn decode(bytes: &[u8]) -> Result<Self> {
        let mut d = Decoder::new(bytes)?;
        let count = d.u32()?;
        if !(2..=1_000_000).contains(&count) {
            return Err(corrupt("invalid hard-link ref list"));
        }
        let mut refs = Vec::with_capacity(count as usize);
        let mut previous: Option<Id> = None;
        for _ in 0..count {
            let id = d.id()?;
            if previous.is_some_and(|p| p >= id) {
                return Err(corrupt("noncanonical hard-link ref list"));
            }
            refs.push(id);
            previous = Some(id);
        }
        d.finish()?;
        Ok(Self { refs })
    }
}
