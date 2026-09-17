#[allow(unused_imports)]
use super::*;

#[derive(Clone, Copy, Default)]
pub struct HttpProvider;

impl Provider for HttpProvider {
    fn name(&self) -> String {
        "http".into()
    }

    fn metadata(&self) -> ProviderMetadata {
        ProviderMetadata {
            name: self.name(),
            actions: vec![
                ActionMetadata::new("request", "Issue a bounded authenticated HTTP request")
                    .with_parameters(vec![
                        // the enum arm keeps editor completion on the verbs; the string arm lets a
                        // plain literal (`method: "GET"`) type-check, since a bare literal infers
                        // as `string` and neither assignment nor an explicit cast reaches an enum.
                        // `execute` still uppercases and rejects anything outside ALLOWED_METHODS,
                        // so the runtime contract is unchanged.
                        ParameterMetadata::required(
                            "method",
                            RuninatorType::Union(vec![
                                RuninatorType::Enum(
                                    ALLOWED_METHODS
                                        .iter()
                                        .map(|value| (*value).into())
                                        .collect(),
                                ),
                                RuninatorType::String,
                            ]),
                        ),
                        ParameterMetadata::required("url", RuninatorType::String),
                        ParameterMetadata::optional(
                            "headers",
                            RuninatorType::map(RuninatorType::String),
                        ),
                        ParameterMetadata::optional(
                            "query",
                            RuninatorType::map(RuninatorType::String),
                        ),
                        ParameterMetadata::optional("body", RuninatorType::Any),
                        ParameterMetadata::optional(
                            "body_format",
                            RuninatorType::Enum(vec![
                                "json".into(),
                                "form".into(),
                                "text".into(),
                                "bytes".into(),
                            ]),
                        )
                        .with_default(json!("json")),
                        ParameterMetadata::optional("timeout_seconds", RuninatorType::Integer),
                        ParameterMetadata::optional("follow_redirects", RuninatorType::Boolean)
                            .with_default(json!(false)),
                        ParameterMetadata::optional(
                            "expect_status",
                            RuninatorType::array(RuninatorType::Integer),
                        ),
                    ])
                    .with_results(vec![ResultMetadata::new("response", RuninatorType::Any)])
                    .with_authentication(ActionAuthenticationMetadata::optional(vec![
                        ActionAuthenticationAlternative::ExecutionProfile,
                    ]))
                    .with_credential_scopes(["http"]),
            ],
            metadata: ProviderRuntimeMetadata {
                credential_scopes: vec!["http".into()],
                contract: Some("bounded HTTP request/response".into()),
                execution_profile: ExecutionProfileSupport::InProcess,
            },
        }
    }

    fn execute_service(
        &self,
        request: ProviderExecutionRequest,
        _sink: Option<Arc<dyn runinator_plugin::provider::ProviderEventSink>>,
        token: CancellationToken,
    ) -> Result<TaskExecutionResult, SendableError> {
        if token.is_cancelled() {
            return Err(REQUEST_FAILED.error("HTTP request was canceled before dispatch"));
        }
        let params: RequestParams = serde_json::from_value(request.parameters.clone().into())
            .map_err(|error| INVALID_PARAMS.error(error))?;
        execute_request(&request, params)
    }
}
