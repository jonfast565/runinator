//! the backend-agnostic seam: what it means to run a container.

use std::sync::Arc;

use crate::Result;
use crate::spec::{ContainerOutput, ContainerSpec};

/// which stream a line of output came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stream {
    Stdout,
    Stderr,
}

impl Stream {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Stdout => "stdout",
            Self::Stderr => "stderr",
        }
    }
}

/// receives output as it is produced.
///
/// the runner has to drain both pipes concurrently anyway (see the crate docs), so handing each
/// line to a sink on the way past costs nothing and is what makes live logs possible without a
/// second read of the same bytes.
pub trait LineSink: Send + Sync {
    fn line(&self, stream: Stream, text: &str);
}

/// polled while a container runs to decide whether to abort it.
///
/// a trait rather than a concrete token so this crate does not depend on whichever cancellation
/// type a caller already has; a plain `Fn() -> bool` satisfies it.
pub trait CancelSignal: Send + Sync {
    fn is_cancelled(&self) -> bool;
}

impl<F> CancelSignal for F
where
    F: Fn() -> bool + Send + Sync,
{
    fn is_cancelled(&self) -> bool {
        self()
    }
}

/// a signal that never fires, for callers with nothing to cancel.
pub fn never_cancelled() -> impl CancelSignal {
    || false
}

/// runs a [`ContainerSpec`] to completion.
///
/// implementations must guarantee three things regardless of what the payload does: the container
/// is gone when this returns (however it returns), output is bounded, and the deadline is enforced
/// by the host rather than trusted to the container.
pub trait ContainerRunner: Send + Sync {
    /// names the backend, for diagnostics.
    fn backend(&self) -> &'static str;

    fn run(
        &self,
        spec: &ContainerSpec,
        logs: Option<Arc<dyn LineSink>>,
        cancel: &dyn CancelSignal,
    ) -> Result<ContainerOutput>;
}
