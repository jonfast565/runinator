#[allow(unused_imports)]
use super::*;

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
