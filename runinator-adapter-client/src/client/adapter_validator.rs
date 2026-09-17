#[allow(unused_imports)]
use super::*;

#[async_trait]
pub trait AdapterValidator: Send + Sync {
    async fn validate(
        &self,
        kind: &str,
        request: AdapterValidationRequest,
    ) -> Result<AdapterValidationResponse>;
}
