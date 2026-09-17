#[allow(unused_imports)]
use super::*;

pub(super) struct ArtifactSource {
    pub(super) bytes: Vec<u8>,
    pub(super) calls: Arc<std::sync::atomic::AtomicUsize>,
}

#[async_trait::async_trait]
impl FunctionArtifactSource for ArtifactSource {
    async fn download_function_artifact(&self, _digest: &str) -> runinator_api::Result<Vec<u8>> {
        self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Ok(self.bytes.clone())
    }
}
