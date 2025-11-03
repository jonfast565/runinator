use clap::Parser;
use runinator_models::errors::SendableError;
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct Config {
    pub dll_path: String,
    pub gossip_bind: String,
    pub gossip_port: u16,
    pub gossip_interval_seconds: u64,
    pub gossip_targets: Vec<String>,
    pub announce_address: String,
    pub command_bind: String,
    pub command_port: u16,
    pub worker_id: Uuid,
}

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct CliArgs {
    #[arg(long, default_value = "/opt/runinator/plugins")]
    dll_path: String,

    #[arg(long, default_value = "0.0.0.0")]
    gossip_bind: String,

    #[arg(long, default_value_t = 5000)]
    gossip_port: u16,

    #[arg(long, default_value_t = 5)]
    gossip_interval_seconds: u64,

    #[arg(long, value_delimiter = ',', default_value = "")]
    gossip_targets: Vec<String>,

    #[arg(long, default_value = "127.0.0.1")]
    announce_address: String,

    #[arg(long, default_value = "0.0.0.0")]
    command_bind: String,

    #[arg(long, default_value_t = 7100)]
    command_port: u16,

    #[arg(long)]
    worker_id: Option<String>,
}

pub fn parse_config() -> Result<Config, SendableError> {
    let args = CliArgs::parse();
    let worker_id = match args.worker_id {
        Some(ref value) if !value.is_empty() => {
            Uuid::parse_str(value).map_err(|err| -> SendableError { Box::new(err) })?
        }
        _ => Uuid::new_v4(),
    };

    let gossip_targets = args
        .gossip_targets
        .into_iter()
        .filter(|value| !value.trim().is_empty())
        .collect();

    Ok(Config {
        dll_path: args.dll_path,
        gossip_bind: args.gossip_bind,
        gossip_port: args.gossip_port,
        gossip_interval_seconds: args.gossip_interval_seconds,
        gossip_targets,
        announce_address: args.announce_address,
        command_bind: args.command_bind,
        command_port: args.command_port,
        worker_id,
    })
}
