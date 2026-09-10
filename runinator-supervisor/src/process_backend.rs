//! native subprocess operations kept outside the supervisor state machine.
use crate::{os::send_terminate, types::DynError};
use std::{
    fmt::Debug,
    io,
    process::{Child, Command, ExitStatus},
};

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
pub trait ProcessBackend: Debug + Send + Sync {
    fn spawn(&self, command: &mut Command) -> io::Result<Box<dyn ManagedChild>>;
}
#[derive(Debug)]
pub struct NativeProcessBackend;
impl ProcessBackend for NativeProcessBackend {
    fn spawn(&self, command: &mut Command) -> io::Result<Box<dyn ManagedChild>> {
        command
            .spawn()
            .map(|child| Box::new(child) as Box<dyn ManagedChild>)
    }
}
