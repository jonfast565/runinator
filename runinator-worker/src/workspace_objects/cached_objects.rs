#[allow(unused_imports)]
use super::*;

pub(crate) struct CachedObjects<'a> {
    pub(super) path: &'a std::path::Path,
    pub(super) local: Option<&'a LocalObjects>,
}

impl ReadStore for CachedObjects<'_> {
    fn info(&self, id: Id) -> storage::Result<ObjectInfo> {
        if let Some(local) = self.local {
            match local.info(id) {
                Ok(info) => return Ok(info),
                Err(storage::Error::NotFound(_)) => {}
                Err(error) => return Err(error),
            }
        }
        match fs::File::open(self.path.join(id.to_string())) {
            Ok(file) => {
                let header = storage::record::header(&file, 0)?;
                if header.id != id || header.record_len()? != file.metadata()?.len() {
                    return Err(storage::Error::Corrupt(
                        "cached record metadata mismatch".into(),
                    ));
                }
                Ok(header.info())
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                Err(storage::Error::NotFound(id.to_string()))
            }
            Err(error) => Err(error.into()),
        }
    }

    fn get(&self, id: Id) -> storage::Result<Object> {
        if let Some(local) = self.local {
            match local.get(id) {
                Ok(object) => return Ok(object),
                Err(storage::Error::NotFound(_)) => {}
                Err(error) => return Err(error),
            }
        }
        match fs::File::open(self.path.join(id.to_string())) {
            Ok(file) => storage::record::read(&file, 0, Some(id)).map(|(_, object)| object),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                Err(storage::Error::NotFound(id.to_string()))
            }
            Err(error) => Err(error.into()),
        }
    }
}
