#[allow(unused_imports)]
use super::*;

#[derive(clap::Args)]
pub(super) struct K8sWorkspaceResetArgs {
    #[arg(long)]
    pub(super) kube_context: Option<String>,
    #[arg(long, conflicts_with = "resume")]
    pub(super) discard_workspaces: bool,
    #[arg(long)]
    pub(super) resume: bool,
}
