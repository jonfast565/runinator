//! Cross-platform replacement for the old `build.ps1`: build the workspace and start it via
//! `runinator-supervisor`, or build+deploy the Kubernetes stack. See `README.md`'s "Run
//! Locally"/"Kubernetes" sections for usage.

mod credential_tools;
mod exec;
mod fsutil;
mod k8s;
mod local;
mod paths;
mod platform;

use std::path::PathBuf;

use clap::{Parser, Subcommand, ValueEnum};
use runinator_db_cli::DatabaseBackend;

#[derive(Parser)]
#[command(name = "xtask", about = "Runinator workspace build and deploy tasks")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

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
        #[command(subcommand)]
        command: K8sCommand,
    },
}

#[derive(clap::Args)]
struct BuildArgs {
    /// cargo build profile (`dev` maps to the `target/debug` directory).
    #[arg(long, default_value = "dev")]
    profile: String,
    /// skip compiling tools/keychain-export and tools/runinator-secret-sync.
    #[arg(long, default_value_t = false)]
    skip_credential_tools: bool,
    /// rustup target to ensure is installed when building on windows.
    #[arg(long, default_value = "x86_64-pc-windows-msvc")]
    windows_target_triple: String,
}

#[derive(Subcommand)]
enum LocalCommand {
    /// Build (unless --skip-build) and start the supervisor stack in the foreground, against the
    /// checked-in `runinator-supervisor.json` (the same config `scripts/run-local.sh` uses).
    Up(LocalUpArgs),
}

#[derive(clap::Args)]
struct LocalUpArgs {
    /// cargo build profile (`dev` maps to the `target/debug` directory).
    #[arg(long, default_value = "dev")]
    profile: String,
    /// assume the workspace and credential tools are already built.
    #[arg(long, default_value_t = false)]
    skip_build: bool,
    /// rustup target to ensure is installed when building on windows.
    #[arg(long, default_value = "x86_64-pc-windows-msvc")]
    windows_target_triple: String,
    /// database backend for the local web service.
    #[arg(long = "database", value_enum, default_value = "sqlite")]
    database_backend: DatabaseBackend,
    /// sqlite file path (only used when --database sqlite). defaults to ~/.runinator/runinator.db.
    #[arg(long)]
    database_path: Option<PathBuf>,
    /// connection url (required for --database postgres/mysql/mariadb).
    #[arg(long)]
    database_url: Option<String>,
}

#[derive(Subcommand)]
enum K8sCommand {
    /// Build (unless --skip-build) and apply the runinator stack to a cluster.
    Deploy(K8sDeployArgs),
    /// Tear down the runinator stack from a cluster.
    Delete(K8sDeleteArgs),
}

#[derive(clap::Args)]
struct K8sDeployArgs {
    /// assume images are already built (and pushed, if applicable); only apply the manifest.
    #[arg(long, default_value_t = false)]
    skip_build: bool,
    /// registry/repository prefix for built images (e.g. `registry.example.com/runinator`); images
    /// are pushed automatically when this is set.
    #[arg(long)]
    image_repository: Option<String>,
    /// tag applied to built images. `local` (the default) is replaced with a fresh `kube-<timestamp>`
    /// tag so every deploy is distinguishable.
    #[arg(long, default_value = "local")]
    image_tag: String,
    /// shorthand for --image-repository pointing at a registry mirrored to the local cluster.
    #[arg(long)]
    local_registry: Option<String>,
    /// kubectl context to use; defaults to the current context.
    #[arg(long)]
    kube_context: Option<String>,
    /// kustomize overlay directory or a raw manifest file.
    #[arg(long, default_value = "deploy/k8s/overlays/local")]
    manifest: PathBuf,
    /// seconds to wait for the pack-import job to complete.
    #[arg(long, default_value_t = 600)]
    pack_import_timeout_secs: u32,
    /// re-apply the postgres/rabbitmq StatefulSets even if they already exist (may roll them).
    #[arg(long, default_value_t = false)]
    recreate_infra: bool,
    /// inject the `components/direct-ingress` kustomize component (host-based ingress + a
    /// debugging-only postgres NodePort). off by default so prod stays closed.
    #[arg(long, default_value_t = false)]
    expose_direct_ingress: bool,
    /// only deploy the command-center web resources.
    #[arg(long, default_value_t = false)]
    command_center_only: bool,
}

#[derive(clap::Args)]
struct K8sDeleteArgs {
    /// kubectl context to use; defaults to the current context.
    #[arg(long)]
    kube_context: Option<String>,
    /// kustomize overlay directory or a raw manifest file.
    #[arg(long, default_value = "deploy/k8s/overlays/local")]
    manifest: PathBuf,
    /// match whichever kustomize component was enabled on deploy, so its resources are torn down too.
    #[arg(long, default_value_t = false)]
    expose_direct_ingress: bool,
    /// only tear down the command-center web resources.
    #[arg(long, default_value_t = false)]
    command_center_only: bool,
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    let workspace_root = paths::workspace_root();

    match cli.command {
        Command::Build(args) => run_build(&workspace_root, &args),
        Command::Local { command } => match command {
            LocalCommand::Up(args) => run_local_up(&workspace_root, &args),
        },
        Command::K8s { command } => match command {
            K8sCommand::Deploy(args) => run_k8s_deploy(&workspace_root, &args),
            K8sCommand::Delete(args) => run_k8s_delete(&workspace_root, &args),
        },
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
        None => runinator_utilities::app_data::default_sqlite_path()
            .map_err(|err| anyhow::anyhow!("failed to resolve default sqlite path: {err}"))?,
    };
    let database_path = if database_path.is_absolute() {
        database_path
    } else {
        workspace_root.join(database_path)
    };
    if database_backend == "sqlite" {
        if let Some(parent) = database_path.parent() {
            paths::ensure_dir(parent)?;
        }
    }

    println!("==> Starting local Runinator stack");
    let options = local::LocalStackOptions {
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

fn run_k8s_deploy(workspace_root: &std::path::Path, args: &K8sDeployArgs) -> anyhow::Result<()> {
    let image_repository = resolve_image_repository(&args.image_repository, &args.local_registry);
    let image_tag = k8s::images::versioned_image_tag(&args.image_tag);

    let manifest_path = if args.manifest.is_absolute() {
        args.manifest.clone()
    } else {
        workspace_root.join(&args.manifest)
    };

    let image_map = if args.skip_build {
        None
    } else {
        let should_push = image_repository.is_some();
        let include_names: Option<Vec<&str>> = args
            .command_center_only
            .then_some(vec!["runinator-command-center"]);

        println!("==> Building container images (tag: {image_tag})");
        Some(k8s::images::build_container_images(
            workspace_root,
            image_repository.as_deref(),
            &image_tag,
            include_names.as_deref(),
            None,
            should_push,
        )?)
    };

    println!("==> Deploying Runinator to the Kubernetes cluster");
    k8s::deploy::deploy_kubernetes_stack(k8s::deploy::DeployOptions {
        workspace_root,
        manifest_path: &manifest_path,
        kube_context: args.kube_context.as_deref(),
        pack_import_timeout_secs: args.pack_import_timeout_secs,
        image_map,
        delete: false,
        command_center_only: args.command_center_only,
        recreate_infra: args.recreate_infra,
        expose_direct_ingress: args.expose_direct_ingress,
    })
}

fn run_k8s_delete(workspace_root: &std::path::Path, args: &K8sDeleteArgs) -> anyhow::Result<()> {
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
        pack_import_timeout_secs: 600,
        image_map: None,
        delete: true,
        command_center_only: args.command_center_only,
        recreate_infra: false,
        expose_direct_ingress: args.expose_direct_ingress,
    })
}
