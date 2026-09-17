#[allow(unused_imports)]
use super::*;

pub(crate) struct Capture {
    pub(super) redirect: Option<platform::Redirect>,
}

impl Capture {
    pub(crate) fn install(dashboard: Arc<Dashboard>) -> io::Result<(Self, Screen)> {
        let (redirect, screen) = platform::install(dashboard)?;
        Ok((
            Self {
                redirect: Some(redirect),
            },
            screen,
        ))
    }

    /// Restore stdout/stderr and wait for the reader to finish draining their final writes.
    pub(crate) fn restore(&mut self) {
        if let Some(redirect) = self.redirect.take() {
            redirect.restore();
        }
    }
}

impl Drop for Capture {
    fn drop(&mut self) {
        self.restore();
    }
}
