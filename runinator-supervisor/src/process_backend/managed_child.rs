#[allow(unused_imports)]
use super::*;

pub trait ManagedChild: Debug + Send {
    fn id(&self) -> u32;
    fn try_wait(&mut self) -> io::Result<Option<ExitStatus>>;
    fn wait(&mut self) -> io::Result<ExitStatus>;
    fn terminate(&mut self) -> Result<(), DynError>;
    fn kill(&mut self) -> io::Result<()>;
}

impl ManagedChild for Child {
    fn id(&self) -> u32 {
        Child::id(self)
    }
    fn try_wait(&mut self) -> io::Result<Option<ExitStatus>> {
        Child::try_wait(self)
    }
    fn wait(&mut self) -> io::Result<ExitStatus> {
        Child::wait(self)
    }
    fn terminate(&mut self) -> Result<(), DynError> {
        send_terminate(self.id())
    }
    fn kill(&mut self) -> io::Result<()> {
        Child::kill(self)
    }
}
