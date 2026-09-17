#[allow(unused_imports)]
use super::*;

#[derive(Clone)]
pub struct SlackProvider;

impl Provider for SlackProvider {
    fn name(&self) -> String {
        "slack".into()
    }

    fn metadata(&self) -> ProviderMetadata {
        let mut actions = vec![
            ActionMetadata::new("send_message", "Send a Slack message")
                .with_parameters(vec![
                    token_param(),
                    ParameterMetadata::required("channel", RuninatorType::String),
                    ParameterMetadata::required("text", RuninatorType::String),
                    ParameterMetadata::optional("team_id", RuninatorType::String)
                        .with_description("Slack workspace id retained in the delivery receipt for inbound thread correlation."),
                    ParameterMetadata::optional(
                        "attachments",
                        RuninatorType::array(attachment_type()),
                    )
                    .with_description("Slack attachment array (typed)."),
                    ParameterMetadata::optional(
                        "blocks",
                        RuninatorType::array(RuninatorType::map(RuninatorType::Any)),
                    )
                    .with_description("Slack block kit array."),
                    ParameterMetadata::optional("thread_ts", RuninatorType::String),
                    ParameterMetadata::optional("mrkdwn", RuninatorType::Boolean),
                    ParameterMetadata::optional("unfurl_links", RuninatorType::Boolean),
                    ParameterMetadata::optional("unfurl_media", RuninatorType::Boolean),
                ])
                .with_results(slack_results()),
        ];
        actions.extend(read::read_action_metadata());
        for action in &mut actions {
            action.authentication = Some(ActionAuthenticationMetadata::required(vec![
                ActionAuthenticationAlternative::secrets(["token"]),
            ]));
        }

        ProviderMetadata {
            name: self.name(),
            actions,
            metadata: ProviderRuntimeMetadata {
                credential_scopes: vec!["slack".into()],
                contract: None,
                execution_profile: Default::default(),
            },
        }
    }

    fn execute_service(
        &self,
        request: ProviderExecutionRequest,
        _sink: Option<Arc<dyn ProviderEventSink>>,
        _token: runinator_plugin::cancel::CancellationToken,
    ) -> Result<TaskExecutionResult, SendableError> {
        match request.action_function.as_str() {
            "send" | "send_message" => send_message(request),
            other => match read::find_action(other) {
                Some(def) => read::execute_read(def, &request),
                None => Err(UNSUPPORTED_ACTION.error(other)),
            },
        }
    }
}
