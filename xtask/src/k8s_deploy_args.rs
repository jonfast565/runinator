#[allow(unused_imports)]
use super::*;

#[derive(clap::Args)]
pub(super) struct K8sDeployArgs {
    /// assume images are already built (and pushed, if applicable); only apply the manifest.
    #[arg(long, default_value_t = false)]
    pub(super) skip_build: bool,
    /// registry/repository prefix for built images (e.g. `registry.example.com/runinator`); images
    /// are pushed automatically when this is set.
    #[arg(long)]
    pub(super) image_repository: Option<String>,
    /// tag applied to built images. `local` becomes `<workspace-version>-kube-<timestamp>` so every
    /// deploy is versioned and distinguishable.
    #[arg(long, default_value = "local")]
    pub(super) image_tag: String,
    /// Database driver compiled into Kubernetes runtime images. The manifest must configure the
    /// same backend (the bundled overlays use postgres).
    #[arg(long, value_parser = ["sqlite", "postgres", "mariadb"], default_value = "postgres")]
    pub(super) database_backend: String,
    /// Broker transport compiled into Kubernetes runtime images. The manifest must configure the
    /// same backend (the bundled overlays use rabbitmq).
    #[arg(long, value_parser = ["http", "tcp", "kafka", "rabbitmq"], default_value = "rabbitmq")]
    pub(super) broker_backend: String,
    /// shorthand for --image-repository pointing at a registry mirrored to the local cluster.
    #[arg(long)]
    pub(super) local_registry: Option<String>,
    /// number of timestamped Runinator releases to retain in --local-registry after a push; zero
    /// disables registry cleanup.
    #[arg(long, default_value_t = 5)]
    pub(super) registry_retention: usize,
    /// kubectl context to use; defaults to the current context.
    #[arg(long)]
    pub(super) kube_context: Option<String>,
    /// kustomize overlay directory or a raw manifest file.
    #[arg(long, default_value = "deploy/k8s/overlays/local")]
    pub(super) manifest: PathBuf,
    /// re-apply the postgres/rabbitmq StatefulSets even if they already exist (may roll them).
    #[arg(long, default_value_t = false)]
    pub(super) recreate_infra: bool,
    /// inject the `components/direct-ingress` kustomize component (host-based ingress + a
    /// debugging-only postgres NodePort). off by default so prod stays closed.
    #[arg(long, default_value_t = false)]
    pub(super) expose_direct_ingress: bool,
    /// deploy only the named service, repeatable; builds just that workload's images and applies
    /// just its resources. omit to deploy the whole stack.
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
