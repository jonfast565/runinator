use clap::Parser;

#[derive(Parser, Debug, Clone)]
pub struct Config {
    #[clap(long, default_value = "tasks.db")]
    pub database: String,

    #[clap(long, default_value = "3000")]
    pub port: u16,

    #[clap(long, default_value = "new_service")]
    pub marker_function: String,

    #[clap(long, default_value = "get_action_name")]
    pub action_name_function: String,

    #[clap(long, default_value = "./dlls")]
    pub dll_path: String,
}

pub fn parse_config() -> Result<Config, Box<dyn std::error::Error>> {
    let results = Config::try_parse()?;
    Ok(results)
}