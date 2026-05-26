use std::env;

use log::info;
use runinator_models::errors::{RuntimeError, SendableError};

pub fn set_exe_dir_as_cwd() -> Result<(), SendableError> {
    let exe_path = env::current_exe()?;
    let exe_dir = exe_path.parent().ok_or_else(|| {
        Box::new(RuntimeError::new(
            "utilities.cwd.executable_parent_missing".into(),
            format!("Executable path has no parent: {}", exe_path.display()),
        )) as SendableError
    })?;
    env::set_current_dir(exe_dir)?;
    let cwd = env::current_dir()?;
    info!("Current working directory: {:?}", cwd);
    Ok(())
}
