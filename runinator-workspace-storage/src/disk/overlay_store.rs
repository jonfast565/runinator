#[allow(unused_imports)]
use super::*;

pub struct OverlayStore {
    pub(crate) base: RawDisk,
    pub(super) dir: TempDir,
    pub(super) caches: Arc<ObjectCaches>,
    pub(super) write_lock: Mutex<()>,
}

impl OverlayStore {
    pub fn new(base: RawDisk, caches: Arc<ObjectCaches>) -> Result<Self> {
        let dir = tempfile::Builder::new()
            .prefix("txn-")
            .tempdir_in(base.root.join("tmp"))?;
        Ok(Self {
            base,
            dir,
            caches,
            write_lock: Mutex::new(()),
        })
    }
    pub(super) fn path(&self, id: Id) -> PathBuf {
        let hex = id.to_string();
        self.dir
            .path()
            .join(&hex[..2])
            .join(format!("{}.rec", &hex[2..]))
    }
    pub(super) fn raw_info(&self, id: Id) -> Result<ObjectInfo> {
        let path = self.path(id);
        match File::open(&path) {
            Ok(f) => {
                let h = record::header(&f, 0)?;
                if h.id != id || h.record_len()? != f.metadata()?.len() {
                    return Err(corrupt("invalid staged record"));
                }
                Ok(h.info())
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => self.base.info(id),
            Err(e) => Err(e.into()),
        }
    }
    pub(super) fn raw_get(&self, id: Id) -> Result<Object> {
        match File::open(self.path(id)) {
            Ok(f) => {
                let (h, obj) = record::read(&f, 0, Some(id))?;
                if h.record_len()? != f.metadata()?.len() {
                    return Err(corrupt("staged record has trailing bytes"));
                }
                Ok(obj)
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => self.base.get(id),
            Err(e) => Err(e.into()),
        }
    }
}

impl ReadStore for OverlayStore {
    fn info(&self, id: Id) -> Result<ObjectInfo> {
        self.raw_info(id)
    }
    fn get(&self, id: Id) -> Result<Object> {
        self.caches.get(&RawOverlay(self), id)
    }
    fn contains(&self, id: Id) -> Result<bool> {
        Ok(self.path(id).try_exists()? || self.base.contains(id)?)
    }
}

impl WriteStore for OverlayStore {
    fn put(&self, kind: Kind, raw: &[u8]) -> Result<Id> {
        if raw.len() > crate::codec::MAX_OBJECT {
            return Err(invalid("object too large"));
        }
        let id = Id::object(kind, raw);
        if self.contains(id)? {
            return Ok(id);
        }
        let mut encoded = Vec::new();
        record::write(&mut encoded, kind, raw)?;
        let _lock = self.write_lock.lock().map_err(|_| Error::Poisoned)?;
        if self.path(id).try_exists()? {
            return Ok(id);
        }
        let path = self.path(id);
        let parent = path.parent().unwrap();
        fs::create_dir_all(parent)?;
        let mut tmp = NamedTempFile::new_in(parent)?;
        tmp.write_all(&encoded)?;
        tmp.flush()?;
        tmp.persist_noclobber(path).map_err(|e| e.error)?;
        Ok(id)
    }
}
