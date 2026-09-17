#[allow(unused_imports)]
use super::*;

/// fail-closed handler used by the generic/headless runtime.
#[derive(Default)]
pub struct DefaultDirectiveHandler;

impl DirectiveHandler for DefaultDirectiveHandler {
    fn handle<'a>(
        &'a self,
        kind: &'a AgentDirectiveKind,
    ) -> Pin<Box<dyn Future<Output = DirectiveResponse> + Send + 'a>> {
        Box::pin(async move {
            DirectiveResponse::unsupported(format!(
                "{} is unavailable on this agent host",
                directive_name(kind)
            ))
        })
    }
}
