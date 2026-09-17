#[allow(unused_imports)]
use super::*;

#[async_trait]
pub trait AdapterVerifier: Send + Sync {
    async fn verify_normalize(
        &self,
        kind: &str,
        request: AdapterRequest,
    ) -> Result<AdapterResponse>;
}
