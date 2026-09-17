#[allow(unused_imports)]
use super::*;

pub trait DirectiveHandler: Send + Sync + 'static {
    fn handle<'a>(
        &'a self,
        kind: &'a AgentDirectiveKind,
    ) -> Pin<Box<dyn Future<Output = DirectiveResponse> + Send + 'a>>;
}
