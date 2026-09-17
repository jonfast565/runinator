#[allow(unused_imports)]
use super::*;

#[derive(clap::Args)]
pub(super) struct K8sDatabaseArgs {
    /// kubectl context to use; defaults to the current context.
    #[arg(long)]
    pub(super) kube_context: Option<String>,
    /// kustomize overlay directory.
    #[arg(long, default_value = "deploy/k8s/overlays/local")]
    pub(super) manifest: PathBuf,
    /// discard the PostgreSQL data PVC before recreating the database. This destroys all durable
    /// Runinator state, then re-runs the web-service bootstrap.
    #[arg(long, default_value_t = false)]
    pub(super) from_scratch: bool,
}
