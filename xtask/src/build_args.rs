#[allow(unused_imports)]
use super::*;

#[derive(clap::Args)]
pub(super) struct BuildArgs {
    /// cargo build profile (`dev` maps to the `target/debug` directory).
    #[arg(long, default_value = "dev")]
    pub(super) profile: String,
    /// skip compiling the packaged tools/keychain-export collector command.
    #[arg(long, default_value_t = false)]
    pub(super) skip_credential_tools: bool,
    /// rustup target to ensure is installed when building on windows.
    #[arg(long, default_value = "x86_64-pc-windows-msvc")]
    pub(super) windows_target_triple: String,
}
