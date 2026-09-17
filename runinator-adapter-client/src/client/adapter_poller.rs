#[allow(unused_imports)]
use super::*;

#[async_trait]
pub trait AdapterPoller: Send + Sync {
    async fn poll(&self, kind: &str, request: AdapterPollRequest) -> Result<AdapterPollResponse>;
}

impl<T: AdapterPoller + AdapterVerifier + AdapterValidator + AdapterHostAdmin> AdapterHostClient
    for T
{
}
