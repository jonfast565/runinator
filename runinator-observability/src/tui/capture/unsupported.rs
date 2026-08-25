use std::fs::File;
use std::io;
use std::sync::Arc;

use super::{Dashboard, Screen};

pub(super) struct Redirect;

impl Redirect {
    pub(super) fn restore(self) {}
}

pub(super) fn install(_: Arc<Dashboard>) -> io::Result<(Redirect, Screen)> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "the runtime dashboard cannot capture process output on this platform",
    ))
}

#[allow(dead_code)]
fn _screen(_: File) {}
