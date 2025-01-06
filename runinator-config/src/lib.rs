use clap::Parser;

#[derive(Parser, Debug)]
pub struct Config {
    #[clap(long, default_value = "tasks.db")]
    database: String,

    #[clap(long, default_value = "3000")]
    port: u16,

    #[clap(long, default_value = "get_library_name")]
    marker_function: String,

    #[clap(long, default_value = "get_action_name")]
    action_name_function: String,

    #[clap(long, default_value = "./dlls")]
    dll_path: String,
}

pub fn parse_config() {

}