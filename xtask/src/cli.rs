#[allow(unused_imports)]
use super::*;

#[derive(Parser)]
#[command(name = "xtask", about = "Runinator workspace build and deploy tasks")]
pub(super) struct Cli {
    #[command(subcommand)]
    pub(super) command: Command,
}
