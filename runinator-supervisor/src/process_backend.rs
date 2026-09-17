//! native subprocess operations kept outside the supervisor state machine.
use crate::{os::send_terminate, types::DynError};
use std::{
    fmt::Debug,
    io,
    process::{Child, Command, ExitStatus},
};

mod managed_child;
pub use managed_child::ManagedChild;

mod process_backend;
pub use process_backend::ProcessBackend;

mod native_process_backend;
pub use native_process_backend::NativeProcessBackend;
