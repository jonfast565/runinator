use clap::Parser;

#[derive(Parser, Debug, Clone)]
pub struct Config {
    #[arg(long, default_value = "/opt/runinator/tasks/tasks.json")]
    pub tasks_file: String,

    #[arg(long, default_value_t = 10)]
    pub poll_interval_seconds: u64,

    #[arg(long, default_value = "0.0.0.0")]
    pub gossip_bind: String,

    #[arg(long, default_value_t = 5000)]
    pub gossip_port: u16,

    #[arg(long, value_delimiter = ',', default_value = "")]
    pub gossip_targets: Vec<String>,
}
