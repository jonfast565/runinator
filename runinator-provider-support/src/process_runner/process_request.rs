#[allow(unused_imports)]
use super::*;

pub struct ProcessRequest<'a> {
    pub command: &'a mut Command,
    pub input: Option<Vec<u8>>,
    pub timeout: Duration,
    pub cancellation: &'a CancellationToken,
    pub sink: Option<Arc<dyn ProviderEventSink>>,
}
