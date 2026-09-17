#[allow(unused_imports)]
use super::*;

pub(crate) struct Capture {
    pub(super) inner: Option<platform::Redirect>,
}

impl Capture {
    /// redirect stdout and stderr into a fresh transcript.
    ///
    /// returns the log, and the terminal to draw on.
    pub(crate) fn install(limit: usize) -> Result<(Self, Screen, Shared)> {
        let (redirect, screen, transcript) = platform::install(limit)?;
        Ok((
            Self {
                inner: Some(redirect),
            },
            screen,
            transcript,
        ))
    }

    /// put the streams back and finish reading whatever was still in flight.
    ///
    /// separate from `Drop` because the console replays the transcript to the real terminal
    /// afterwards, and that has to happen with stdout pointing at the terminal again.
    pub(crate) fn restore(&mut self) {
        if let Some(redirect) = self.inner.take() {
            redirect.restore();
        }
    }
}

impl Drop for Capture {
    fn drop(&mut self) {
        self.restore();
    }
}
