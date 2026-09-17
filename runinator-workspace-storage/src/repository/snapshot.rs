#[allow(unused_imports)]
use super::*;

pub struct Snapshot<'a> {
    pub(super) _guard: RwLockReadGuard<'a, ()>,
    pub(crate) repository: &'a Repository,
    pub(super) store: DiskView,
    pub id: Id,
    pub revision: Revision,
    pub workspace: Workspace,
}

impl ReadStore for Snapshot<'_> {
    fn info(&self, id: Id) -> Result<ObjectInfo> {
        self.store.info(id)
    }
    fn get(&self, id: Id) -> Result<Object> {
        self.store.get(id)
    }
    fn contains(&self, id: Id) -> Result<bool> {
        self.store.contains(id)
    }
}

impl Snapshot<'_> {
    pub fn stat(&self, path: &str) -> Result<(u64, Inode)> {
        namespace::stat(self, &self.workspace, path)
    }
    pub fn file_id(&self, path: &str) -> Result<Id> {
        namespace::file(self, &self.workspace, path)
    }
    pub fn file_id_follow(&self, path: &str) -> Result<Id> {
        let n = namespace::resolve_follow(self, &self.workspace, path)?;
        match namespace::inode(self, &self.workspace, n)?.data {
            InodeData::File(id) => Ok(id),
            _ => Err(invalid("not a regular file")),
        }
    }
    pub fn read_into(&self, path: &str, offset: u64, output: &mut [u8]) -> Result<usize> {
        pages::read_into(
            self,
            &self.repository.pages,
            self.file_id(path)?,
            offset,
            output,
        )
    }
    pub fn read_range(&self, path: &str, offset: u64, len: usize) -> Result<Vec<u8>> {
        pages::read_range(
            self,
            &self.repository.pages,
            self.file_id(path)?,
            offset,
            len,
        )
    }
    pub fn copy_to<W: Write>(&self, path: &str, writer: W) -> Result<u64> {
        pages::copy_to(self, &self.repository.pages, self.file_id(path)?, writer)
    }
    pub fn list<F: FnMut(&str, u64) -> Result<()>>(&self, path: &str, f: &mut F) -> Result<()> {
        namespace::list(self, &self.workspace, path, f)
    }
    pub fn reader(&self, path: &str) -> Result<pages::FileReader<'_, Self>> {
        pages::FileReader::new(self, &self.repository.pages, self.file_id(path)?)
    }
    pub fn read_link(&self, path: &str) -> Result<String> {
        namespace::read_link(self, &self.workspace, path)
    }
}
