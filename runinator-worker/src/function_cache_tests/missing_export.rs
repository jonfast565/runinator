#[allow(unused_imports)]
use super::*;

pub(super) struct MissingExport;

#[async_trait::async_trait]
impl FunctionExportResolver for MissingExport {
    async fn resolve_function_export(
        &self,
        _id: uuid::Uuid,
    ) -> runinator_api::Result<runinator_models::functions::FunctionInvocationTarget> {
        Err(runinator_api::ApiError::UnexpectedResponse(
            "export was removed".into(),
        ))
    }
}
