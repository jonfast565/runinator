//! platforms with neither `dup2` nor `SetStdHandle`.
//!
//! there is no such target in this workspace — the images are linux, and the desktop builds are
//! macos and windows — so this exists to keep `OutputCapture` free of `cfg` rather than to serve
//! anything. `Redirect` is uninhabited, which is how the compiler is told that the methods below
//! cannot be reached rather than merely never being called.

use std::fs::File;

use crate::commands::{Result, err};

pub(super) enum Redirect {}

impl Redirect {
    pub(super) fn take(&mut self) -> String {
        match *self {}
    }

    pub(super) fn restore(self) {
        match self {}
    }
}

pub(super) fn install() -> Result<(Redirect, File)> {
    Err(err(
        "the mcp server captures command output, which this platform has no way to redirect",
    ))
}
