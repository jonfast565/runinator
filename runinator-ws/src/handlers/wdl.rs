use axum::Json;

pub(crate) async fn complete_wdl(
    Json(request): Json<runinator_wdl::WdlCompletionRequest>,
) -> Json<runinator_wdl::WdlCompletionResponse> {
    Json(runinator_wdl::complete_source(request))
}
