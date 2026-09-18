#[allow(unused_imports)]
use super::*;

#[derive(clap::Args)]
pub(super) struct K8sDeleteArgs {
    /// kubectl context to use; defaults to the current context.
    #[arg(long)]
    pub(super) kube_context: Option<String>,
    /// kustomize overlay directory or a raw manifest file.
    #[arg(long, default_value = "deploy/k8s/overlays/local")]
    pub(super) manifest: PathBuf,
    /// match whichever kustomize component was enabled on deploy, so its resources are torn down too.
    #[arg(long, default_value_t = false)]
    pub(super) expose_direct_ingress: bool,
    /// tear down only the named service, repeatable. omit to tear down the whole stack.
    #[arg(
        long = "service",
        value_name = "SERVICE",
        value_parser = clap::builder::PossibleValuesParser::new(k8s::deploy::DeployTarget::keys())
    )]
    pub(super) services: Vec<String>,
    /// shorthand for `--service command-center`.
    #[arg(long, default_value_t = false)]
    pub(super) command_center_only: bool,
}
