#[allow(unused_imports)]
use super::*;

#[derive(Debug)]
pub(super) struct FakeChild(pub(super) Arc<Mutex<Vec<&'static str>>>);

impl ManagedChild for FakeChild {
    fn id(&self) -> u32 {
        42
    }
    fn try_wait(&mut self) -> io::Result<Option<ExitStatus>> {
        Ok(None)
    }
    fn wait(&mut self) -> io::Result<ExitStatus> {
        self.0.lock().unwrap().push("wait");
        Err(io::Error::other("fake wait"))
    }
    fn terminate(&mut self) -> Result<(), DynError> {
        self.0.lock().unwrap().push("terminate");
        Ok(())
    }
    fn kill(&mut self) -> io::Result<()> {
        self.0.lock().unwrap().push("kill");
        Ok(())
    }
}
