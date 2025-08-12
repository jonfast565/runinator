use std::env;

use log::info;
use runinator_models::errors::SendableError;

pub fn set_exe_dir_as_cwd() -> Result<(), SendableError> {
    let exe_path = env::current_exe()?;
    let exe_dir = exe_path.parent().expect("path has parent");
    env::set_current_dir(exe_dir)?;
    let cwd = env::current_dir()?;
    info!("Current working directory: {:?}", cwd);
    Ok(())
}
