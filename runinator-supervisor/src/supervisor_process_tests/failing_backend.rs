#[allow(unused_imports)]
use super::*;

#[derive(Debug)]
pub(super) struct FailingBackend;

impl ProcessBackend for FailingBackend {
    fn spawn(&self, _: &mut Command) -> io::Result<Box<dyn ManagedChild>> {
        Err(io::Error::other("fake spawn failure"))
    }
}
