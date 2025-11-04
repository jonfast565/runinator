use std::env;
use log::info;
use crate::{dirutils, logger::{self, print_env}};


pub fn startup(name: &str) -> Result<(), Box<dyn std::error::Error + Send + Sync + 'static>> {
    unsafe {
        env::set_var("RUST_BACKTRACE", "1");
    }
    dirutils::set_exe_dir_as_cwd()?;
    logger::setup_logger()?;
    log_panics::init();

    info!("--- {} ---", name);
    print_env()?;

    Ok(())
}
