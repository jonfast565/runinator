use crate::{
    Id,
    codec::{Binary, Decoder, Encoder, MAX_OBJECT},
    error::{Result, corrupt, invalid},
    index::DiskIndex,
    io_util,
};
use std::{
    collections::{BTreeMap, BTreeSet},
    io::Write,
    path::Path,
};
use tempfile::NamedTempFile;
#[derive(Clone, Debug, Default)]
pub struct Catalog {
    pub generation: u64,
    pub index: Option<Id>,
    pub packs: BTreeSet<Id>,
    pub refs: BTreeMap<String, Id>,
}
pub fn validate_ref(name: &str) -> Result<()> {
    if name.is_empty()
        || name.len() > 255
        || !name
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || b"_-./".contains(&c))
        || name
            .split('/')
            .any(|s| s.is_empty() || s == "." || s == "..")
    {
        return Err(invalid("invalid ref name"));
    }
    Ok(())
}
impl Binary for Catalog {
    fn encode(&self) -> Result<Vec<u8>> {
        if self.packs.len() > 65536 || self.refs.len() > 65536 {
            return Err(invalid("catalog limit exceeded; compact packs"));
        }
        let mut e = Encoder::new();
        e.u64(self.generation);
        e.optional_id(self.index);
        e.u32(self.packs.len() as u32);
        for id in &self.packs {
            e.id(*id);
        }
        e.u32(self.refs.len() as u32);
        for (name, id) in &self.refs {
            validate_ref(name)?;
            e.string(name)?;
            e.id(*id);
        }
        e.finish()
    }
    fn decode(b: &[u8]) -> Result<Self> {
        let mut d = Decoder::new(b)?;
        let generation = d.u64()?;
        let index = d.optional_id()?;
        let n = d.u32()?;
        if n > 65536 {
            return Err(corrupt("too many packs"));
        }
        let mut packs = BTreeSet::new();
        let mut prev = None;
        for _ in 0..n {
            let id = d.id()?;
            if prev.is_some_and(|p| p >= id) {
                return Err(corrupt("unordered packs"));
            }
            packs.insert(id);
            prev = Some(id);
        }
        let n = d.u32()?;
        if n > 65536 {
            return Err(corrupt("too many refs"));
        }
        let mut refs = BTreeMap::new();
        let mut prev = String::new();
        for _ in 0..n {
            let name = d.string(255)?;
            validate_ref(&name)?;
            if name <= prev {
                return Err(corrupt("unordered refs"));
            }
            prev = name.clone();
            refs.insert(name, d.id()?);
        }
        d.finish()?;
        if index.is_none() && (!packs.is_empty() || !refs.is_empty()) {
            return Err(corrupt("catalog lacks required index"));
        }
        Ok(Self {
            generation,
            index,
            packs,
            refs,
        })
    }
}
pub fn write(root: &Path, catalog: &Catalog) -> Result<Id> {
    let mut tmp = NamedTempFile::new_in(root.join("tmp"))?;
    tmp.write_all(&catalog.encode()?)?;
    io_util::install(tmp, &root.join("catalogs"), ".cat")
}
pub fn load(root: &Path) -> Result<(Id, Catalog, DiskIndex)> {
    let current = io_util::read_limited(&root.join("CURRENT"), 65)?;
    let current = std::str::from_utf8(&current).map_err(|_| corrupt("invalid CURRENT"))?;
    let digest: Id = current
        .strip_suffix('\n')
        .ok_or_else(|| corrupt("CURRENT must end in newline"))?
        .parse()?;
    let path = root.join("catalogs").join(format!("{digest}.cat"));
    let bytes = io_util::read_limited(&path, MAX_OBJECT)?;
    if Id::sha256(&bytes) != digest {
        return Err(corrupt("catalog digest mismatch"));
    }
    let catalog = Catalog::decode(&bytes)?;
    let index = match catalog.index {
        None => DiskIndex::default(),
        Some(id) => {
            let path = root.join("indexes").join(format!("{id}.idx"));
            io_util::verify_file(&path, id)?;
            let index = DiskIndex::open(&path)?;
            index.validate()?;
            index
        }
    };
    for i in 0..index.count {
        let entry = index.entry(i)?;
        if !catalog.packs.contains(&entry.pack) {
            return Err(corrupt("index references a pack absent from the catalog"));
        }
    }
    Ok((digest, catalog, index))
}
