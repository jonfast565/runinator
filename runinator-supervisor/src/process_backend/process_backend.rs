#[allow(unused_imports)]
use super::*;

pub trait ProcessBackend: Debug + Send + Sync {
    fn spawn(&self, command: &mut Command) -> io::Result<Box<dyn ManagedChild>>;
}
