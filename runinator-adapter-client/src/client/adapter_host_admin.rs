#[allow(unused_imports)]
use super::*;

#[async_trait]
pub trait AdapterHostAdmin: Send + Sync {
    fn host_url(&self) -> &str;
    fn token_configured(&self) -> bool;
    async fn kinds(&self) -> Result<Vec<AdapterKindCatalogEntry>>;
    async fn health(&self) -> Result<serde_json::Value>;
    async fn reload(&self) -> Result<serde_json::Value>;
}
