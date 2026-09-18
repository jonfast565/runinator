//! Cross-platform workspace tasks: build the workspace and start it via `runinator-supervisor`, or
//! build+deploy the Kubernetes stack. See `README.md`'s "Run Locally"/"Kubernetes" sections for
//! usage.

mod credential_tools;
mod exec;
mod fsutil;
mod k8s;
mod local;
mod paths;
mod platform;
mod service;

use std::path::PathBuf;

use clap::{Parser, Subcommand, ValueEnum};
use runinator_db_cli::DatabaseBackend;
use service::XtaskService;

#[derive(Subcommand)]
enum Command {
    /// Build the cargo workspace and the host-only credential tools.
    Build(BuildArgs),
    /// Local (non-Kubernetes) dev stack.
    Local {
        #[command(subcommand)]
        command: LocalCommand,
    },
    /// Kubernetes build + deploy.
    K8s {
        /// seconds to wait for another xtask Kubernetes mutation to release the cluster lease.
        #[arg(long, global = true, default_value_t = 900)]
        deploy_lock_timeout_secs: u64,
        #[command(subcommand)]
        command: K8sCommand,
    },
}

#[derive(Subcommand)]
enum LocalCommand {
    /// Build (unless --skip-build) and start the local stack in the foreground.
    Up(LocalUpArgs),
}

#[derive(Clone, Copy, clap::ValueEnum)]
enum LocalTopology {
    Standalone,
    Supervisor,
}

#[derive(Subcommand)]
enum K8sCommand {
    /// Quiesce and erase only durable workspace state for the new format cutover.
    ResetWorkspaces(K8sWorkspaceResetArgs),
    /// Build (unless --skip-build) and apply the runinator stack to a cluster.
    Deploy(K8sDeployArgs),
    /// Apply only the Grafana dashboard, datasource, provider, deployment, and service resources.
    RedeployGrafana(K8sGrafanaArgs),
    /// Apply only the PostgreSQL Service and StatefulSet resources.
    RedeployDatabase(K8sDatabaseArgs),
    /// Rebuild the RabbitMQ default vhost after a confirmed message-store corruption.
    RecoverRabbitmq(K8sRabbitMqRecoveryArgs),
    /// Tear down the runinator stack from a cluster.
    Delete(K8sDeleteArgs),
}

impl K8sCommand {
    fn kube_context(&self) -> Option<&str> {
        match self {
            Self::ResetWorkspaces(args) => args.kube_context.as_deref(),
            Self::Deploy(args) => args.kube_context.as_deref(),
            Self::RedeployGrafana(args) => args.kube_context.as_deref(),
            Self::RedeployDatabase(args) => args.kube_context.as_deref(),
            Self::RecoverRabbitmq(args) => args.kube_context.as_deref(),
            Self::Delete(args) => args.kube_context.as_deref(),
        }
    }

    fn operation(&self) -> &'static str {
        match self {
            Self::ResetWorkspaces(_) => "reset-workspaces",
            Self::Deploy(_) => "deploy",
            Self::RedeployGrafana(_) => "redeploy-grafana",
            Self::RedeployDatabase(_) => "redeploy-database",
            Self::RecoverRabbitmq(_) => "recover-rabbitmq",
            Self::Delete(_) => "delete",
        }
    }
}

fn main() -> anyhow::Result<()> {
    XtaskService::new().run()
}

fn run_process() -> anyhow::Result<()> {
    let cli = Cli::parse();
    let workspace_root = paths::workspace_root();

    match cli.command {
        Command::Build(args) => run_build(&workspace_root, &args),
        Command::Local { command } => match command {
            LocalCommand::Up(args) => run_local_up(&workspace_root, &args),
        },
        Command::K8s {
            deploy_lock_timeout_secs,
            command,
        } => {
            let kube_context = command.kube_context().map(str::to_owned);
            let operation = command.operation();
            let deploy_lock = k8s::lock::DeploymentLock::acquire(
                &workspace_root,
                kube_context.as_deref(),
                operation,
                std::time::Duration::from_secs(deploy_lock_timeout_secs),
            )?;

            let result = match command {
                K8sCommand::ResetWorkspaces(args) => {
                    anyhow::ensure!(
                        args.discard_workspaces || args.resume,
                        "pass --discard-workspaces to erase durable workspaces, or --resume after deploying the new format"
                    );
                    k8s::workspace_reset::reset(
                        &workspace_root,
                        args.kube_context.as_deref(),
                        args.resume,
                    )
                }
                K8sCommand::Deploy(args) => run_k8s_deploy(&workspace_root, &args),
                K8sCommand::RedeployGrafana(args) => {
                    run_k8s_redeploy_grafana(&workspace_root, &args)
                }
                K8sCommand::RedeployDatabase(args) => {
                    run_k8s_redeploy_database(&workspace_root, &args)
                }
                K8sCommand::RecoverRabbitmq(args) => {
                    run_k8s_recover_rabbitmq(&workspace_root, &args)
                }
                K8sCommand::Delete(args) => run_k8s_delete(&workspace_root, &args),
            };

            if result.is_ok() {
                deploy_lock.ensure_held()?;
            }
            result
        }
    }
}

fn ensure_windows_rust_target(
    workspace_root: &std::path::Path,
    triple: &str,
) -> anyhow::Result<()> {
    if !cfg!(target_os = "windows") {
        return Ok(());
    }
    exec::require_tool("rustup")?;
    let installed = exec::capture("rustup", &["target", "list", "--installed"], workspace_root)?;
    if installed.lines().any(|line| line.trim() == triple) {
        return Ok(());
    }
    println!("==> Adding rustup target '{triple}'");
    exec::run("rustup", &["target", "add", triple], workspace_root)
}

fn cargo_build_workspace(workspace_root: &std::path::Path, profile: &str) -> anyhow::Result<()> {
    println!("==> Building workspace with cargo profile '{profile}'");
    exec::run(
        "cargo",
        &["build", "--profile", profile, "--workspace"],
        workspace_root,
    )
}

fn run_build(workspace_root: &std::path::Path, args: &BuildArgs) -> anyhow::Result<()> {
    ensure_windows_rust_target(workspace_root, &args.windows_target_triple)?;
    cargo_build_workspace(workspace_root, &args.profile)?;
    if !args.skip_credential_tools {
        credential_tools::build_credential_tools(workspace_root);
    }
    Ok(())
}

fn run_local_up(workspace_root: &std::path::Path, args: &LocalUpArgs) -> anyhow::Result<()> {
    let target_dir = paths::target_dir(workspace_root, &args.profile);

    if !args.skip_build {
        ensure_windows_rust_target(workspace_root, &args.windows_target_triple)?;
        cargo_build_workspace(workspace_root, &args.profile)?;
        credential_tools::build_credential_tools(workspace_root);
    }

    let database_backend = args
        .database_backend
        .to_possible_value()
        .expect("DatabaseBackend has no skipped variants")
        .get_name()
        .to_string();

    let database_path = match &args.database_path {
        Some(path) => path.clone(),
        None => runinator_platform::app_data::default_sqlite_path()
            .map_err(|err| anyhow::anyhow!("failed to resolve default sqlite path: {err}"))?,
    };
    let database_path = if database_path.is_absolute() {
        database_path
    } else {
        workspace_root.join(database_path)
    };
    if database_backend == "sqlite"
        && let Some(parent) = database_path.parent()
    {
        paths::ensure_dir(parent)?;
    }

    println!("==> Starting local Runinator stack");
    let options = local::LocalStackOptions {
        topology: match args.topology {
            LocalTopology::Standalone => local::LocalTopology::Standalone,
            LocalTopology::Supervisor => local::LocalTopology::Supervisor,
        },
        database_backend: &database_backend,
        database_path: &database_path,
        database_url: args.database_url.as_deref(),
    };
    local::start_local_stack(workspace_root, &target_dir, &options)
}

fn resolve_image_repository(
    image_repository: &Option<String>,
    local_registry: &Option<String>,
) -> Option<String> {
    match image_repository {
        Some(repository) if !repository.trim().is_empty() => Some(repository.clone()),
        _ => local_registry
            .as_deref()
            .map(str::trim)
            .filter(|registry| !registry.is_empty())
            .map(|registry| registry.trim_end_matches('/').to_string()),
    }
}

fn effective_local_registry<'a>(
    image_repository: &Option<String>,
    local_registry: &'a Option<String>,
) -> Option<&'a str> {
    if image_repository
        .as_deref()
        .is_some_and(|repository| !repository.trim().is_empty())
    {
        return None;
    }
    local_registry
        .as_deref()
        .map(str::trim)
        .filter(|registry| !registry.is_empty())
}

/// resolves `--service` values plus the `--command-center-only` shorthand into deploy targets.
/// an empty result means the whole stack.
fn selected_targets(
    services: &[String],
    command_center_only: bool,
) -> anyhow::Result<Vec<&'static k8s::deploy::DeployTarget>> {
    let mut keys: Vec<String> = services.to_vec();
    if command_center_only {
        keys.push("command-center".to_string());
    }
    k8s::deploy::DeployTarget::resolve(&keys)
}

fn run_k8s_deploy(workspace_root: &std::path::Path, args: &K8sDeployArgs) -> anyhow::Result<()> {
    let image_repository = resolve_image_repository(&args.image_repository, &args.local_registry);
    let local_registry = effective_local_registry(&args.image_repository, &args.local_registry);
    let image_tag = k8s::images::versioned_image_tag(&args.image_tag);

    let manifest_path = if args.manifest.is_absolute() {
        args.manifest.clone()
    } else {
        workspace_root.join(&args.manifest)
    };

    let targets = selected_targets(&args.services, args.command_center_only)?;
    let include_names: Option<Vec<&str>> =
        (!targets.is_empty()).then(|| k8s::deploy::DeployTarget::images_for(&targets));
    if !targets.is_empty() {
        println!(
            "==> Deploying only: {}",
            targets
                .iter()
                .map(|target| target.key)
                .collect::<Vec<_>>()
                .join(", ")
        );
    }
    let image_map = if args.skip_build {
        anyhow::ensure!(
            args.image_tag != "local" && !args.image_tag.trim().is_empty(),
            "--skip-build requires an explicit --image-tag naming existing images"
        );
        Some(k8s::images::prebuilt_image_map(
            image_repository.as_deref(),
            &image_tag,
            include_names.as_deref(),
        ))
    } else {
        let should_push = image_repository.is_some();

        println!("==> Building container images (tag: {image_tag})");
        let built = k8s::images::build_container_images(
            workspace_root,
            k8s::images::ContainerImageBuild {
                repository: image_repository.as_deref(),
                tag: &image_tag,
                include_names: include_names.as_deref(),
                exclude_names: None,
                push_images: should_push,
                database_backend: &args.database_backend,
                broker_backend: &args.broker_backend,
            },
        )?;

        if let Some(registry) = local_registry
            && args.registry_retention > 0
        {
            println!(
                "==> Pruning local registry to the newest {} release versions",
                args.registry_retention
            );
            k8s::registry::prune_release_images(
                registry,
                built.keys().map(String::as_str),
                args.registry_retention,
            )?;
        }

        Some(built)
    };

    println!("==> Deploying Runinator to the Kubernetes cluster");
    k8s::deploy::deploy_kubernetes_stack(k8s::deploy::DeployOptions {
        workspace_root,
        manifest_path: &manifest_path,
        kube_context: args.kube_context.as_deref(),
        image_map,
        delete: false,
        targets,
        recreate_infra: args.recreate_infra,
        expose_direct_ingress: args.expose_direct_ingress,
    })
}

fn run_k8s_delete(workspace_root: &std::path::Path, args: &K8sDeleteArgs) -> anyhow::Result<()> {
    let targets = selected_targets(&args.services, args.command_center_only)?;
    let manifest_path = if args.manifest.is_absolute() {
        args.manifest.clone()
    } else {
        workspace_root.join(&args.manifest)
    };

    println!("==> Tearing down Runinator from the Kubernetes cluster");
    k8s::deploy::deploy_kubernetes_stack(k8s::deploy::DeployOptions {
        workspace_root,
        manifest_path: &manifest_path,
        kube_context: args.kube_context.as_deref(),
        image_map: None,
        delete: true,
        targets,
        recreate_infra: false,
        expose_direct_ingress: args.expose_direct_ingress,
    })
}

fn run_k8s_redeploy_grafana(
    workspace_root: &std::path::Path,
    args: &K8sGrafanaArgs,
) -> anyhow::Result<()> {
    let manifest_path = if args.manifest.is_absolute() {
        args.manifest.clone()
    } else {
        workspace_root.join(&args.manifest)
    };

    println!("==> Redeploying only Grafana observability resources");
    k8s::deploy::redeploy_grafana(k8s::deploy::GrafanaRedeployOptions {
        workspace_root,
        manifest_path: &manifest_path,
        kube_context: args.kube_context.as_deref(),
    })
}

fn run_k8s_redeploy_database(
    workspace_root: &std::path::Path,
    args: &K8sDatabaseArgs,
) -> anyhow::Result<()> {
    let manifest_path = if args.manifest.is_absolute() {
        args.manifest.clone()
    } else {
        workspace_root.join(&args.manifest)
    };

    println!("==> Redeploying only the PostgreSQL database resources");
    k8s::deploy::redeploy_database(k8s::deploy::DatabaseRedeployOptions {
        workspace_root,
        manifest_path: &manifest_path,
        kube_context: args.kube_context.as_deref(),
        from_scratch: args.from_scratch,
    })
}

fn run_k8s_recover_rabbitmq(
    workspace_root: &std::path::Path,
    args: &K8sRabbitMqRecoveryArgs,
) -> anyhow::Result<()> {
    anyhow::ensure!(
        args.discard_broker_messages,
        "refusing RabbitMQ recovery without --discard-broker-messages"
    );
    println!("==> Rebuilding RabbitMQ's corrupted default vhost");
    k8s::deploy::recover_rabbitmq(workspace_root, args.kube_context.as_deref())
}

mod cli;
use cli::Cli;

mod build_args;
use build_args::BuildArgs;

mod local_up_args;
use local_up_args::LocalUpArgs;

mod k8s_workspace_reset_args;
use k8s_workspace_reset_args::K8sWorkspaceResetArgs;

mod k8s_grafana_args;
use k8s_grafana_args::K8sGrafanaArgs;

mod k8s_database_args;
use k8s_database_args::K8sDatabaseArgs;

mod k8s_rabbit_mq_recovery_args;
use k8s_rabbit_mq_recovery_args::K8sRabbitMqRecoveryArgs;

mod k8s_deploy_args;
use k8s_deploy_args::K8sDeployArgs;

mod k8s_delete_args;
use k8s_delete_args::K8sDeleteArgs;
