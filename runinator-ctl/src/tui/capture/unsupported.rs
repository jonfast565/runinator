//! platforms with neither `dup2` nor `SetStdHandle`.
//!
//! there is no such target in this workspace, so this exists to keep `Capture` free of `cfg` rather
//! than to serve anything. reporting the console unavailable is not a failure: `console.rs` falls
//! back to the reedline prompt, which needs no capture because it does not own the screen.

use std::fs::File;

use super::{Screen, Shared};
use crate::commands::{Result, err};

pub(super) enum Redirect {}

impl Redirect {
    pub(super) fn restore(self) {
        match self {}
    }
}

pub(super) fn install(limit: usize) -> Result<(Redirect, Screen, Shared)> {
    let _ = limit;
    let _: Option<File> = None;
    Err(err(
        "capturing command output needs a terminal this platform can redirect",
    ))
}
