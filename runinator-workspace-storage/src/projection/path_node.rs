#[allow(unused_imports)]
use super::*;

#[derive(Clone, Debug)]
pub struct PathNode {
    pub inode_number: u64,
    pub inode_id: Id,
    pub children: Option<Id>,
}

impl PathNode {
    pub fn child<S: ReadStore + ?Sized>(&self, store: &S, name: &str) -> Result<Option<Id>> {
        radix::get(store, self.children, name.as_bytes())
    }
    pub fn entries<S: ReadStore + ?Sized>(&self, store: &S) -> Result<Vec<(String, Id)>> {
        let mut entries = Vec::new();
        radix::visit(store, self.children, &mut |name, id| {
            let name = std::str::from_utf8(name).map_err(|_| corrupt("invalid projected name"))?;
            validate_name(name)?;
            entries.push((name.to_owned(), id));
            Ok(())
        })?;
        Ok(entries)
    }
}

impl Binary for PathNode {
    fn encode(&self) -> Result<Vec<u8>> {
        if self.inode_number == 0 {
            return Err(invalid("invalid projection inode"));
        }
        let mut e = Encoder::new();
        e.u64(self.inode_number);
        e.id(self.inode_id);
        e.optional_id(self.children);
        e.finish()
    }
    fn decode(bytes: &[u8]) -> Result<Self> {
        let mut d = Decoder::new(bytes)?;
        let node = Self {
            inode_number: d.u64()?,
            inode_id: d.id()?,
            children: d.optional_id()?,
        };
        if node.inode_number == 0 {
            return Err(corrupt("invalid projection inode"));
        }
        d.finish()?;
        Ok(node)
    }
}
