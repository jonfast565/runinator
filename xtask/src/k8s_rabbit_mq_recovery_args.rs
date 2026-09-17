#[allow(unused_imports)]
use super::*;

#[derive(clap::Args)]
pub(super) struct K8sRabbitMqRecoveryArgs {
    /// kubectl context to use; defaults to the current context.
    #[arg(long)]
    pub(super) kube_context: Option<String>,
    /// Required acknowledgement that the corrupted RabbitMQ vhost's in-flight messages are lost.
    #[arg(long)]
    pub(super) discard_broker_messages: bool,
}
