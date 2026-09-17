#[allow(unused_imports)]
use super::*;

pub struct ProfileLease {
    pub context: MaterializedExecutionProfile,
    pub credential_scopes: Vec<String>,
    pub(super) root: PathBuf,
}

impl Drop for ProfileLease {
    fn drop(&mut self) {
        if let Err(error) = fs::remove_dir_all(&self.root)
            && error.kind() != std::io::ErrorKind::NotFound
        {
            tracing::warn!(path = %self.root.display(), %error, "failed to clean execution profile directory");
        }
    }
}
