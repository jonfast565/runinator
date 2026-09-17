#[allow(unused_imports)]
use super::*;

#[derive(clap::Args)]
pub(super) struct K8sGrafanaArgs {
    /// kubectl context to use; defaults to the current context.
    #[arg(long)]
    pub(super) kube_context: Option<String>,
    /// kustomize overlay directory or a raw manifest file.
    #[arg(long, default_value = "deploy/k8s/overlays/local")]
    pub(super) manifest: PathBuf,
}
