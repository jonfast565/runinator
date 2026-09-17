#[allow(unused_imports)]
use super::*;

#[derive(clap::Args)]
pub(super) struct LocalUpArgs {
    /// local process topology.
    #[arg(long, value_enum, default_value = "standalone")]
    pub(super) topology: LocalTopology,
    /// cargo build profile (`dev` maps to the `target/debug` directory).
    #[arg(long, default_value = "dev")]
    pub(super) profile: String,
    /// assume the workspace and credential tools are already built.
    #[arg(long, default_value_t = false)]
    pub(super) skip_build: bool,
    /// rustup target to ensure is installed when building on windows.
    #[arg(long, default_value = "x86_64-pc-windows-msvc")]
    pub(super) windows_target_triple: String,
    /// database backend for the local web service.
    #[arg(long = "database", value_enum, default_value = "sqlite")]
    pub(super) database_backend: DatabaseBackend,
    /// sqlite file path (only used when --database sqlite). defaults to ~/.runinator/runinator.db.
    #[arg(long)]
    pub(super) database_path: Option<PathBuf>,
    /// connection URL (required for --database postgres/mariadb).
    #[arg(long)]
    pub(super) database_url: Option<String>,
}
