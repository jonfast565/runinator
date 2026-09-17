#[allow(unused_imports)]
use super::*;

#[derive(Debug, Parser)]
#[command(
    name = "runinatorctl",
    no_binary_name = true,
    disable_help_subcommand = true,
    about = "Every runinatorctl command, prefixed with `:` inside the console"
)]
pub struct ReplCommand {
    /// Print this command's output as json.
    #[arg(long, global = true)]
    pub json: bool,
    #[command(subcommand)]
    pub command: Commands,
}
