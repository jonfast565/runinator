#[allow(unused_imports)]
use super::*;

#[derive(Debug)]
pub struct NativeProcessBackend;

impl ProcessBackend for NativeProcessBackend {
    fn spawn(&self, command: &mut Command) -> io::Result<Box<dyn ManagedChild>> {
        command
            .spawn()
            .map(|child| Box::new(child) as Box<dyn ManagedChild>)
    }
}
