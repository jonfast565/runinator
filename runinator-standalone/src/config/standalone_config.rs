#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StandaloneConfig {
    pub state_dir: PathBuf,
    pub database: String,
    pub sqlite_path: PathBuf,
    pub database_url: Option<String>,
    pub workers: u32,
    pub engines: u32,
    pub wakers: u32,
    pub worker_concurrency: usize,
    pub engine_concurrency: usize,
    pub api_port: u16,
    pub broker_port: u16,
    pub blob_port: u16,
    pub adapter_port: u16,
    pub auth_enabled: bool,
    pub desktop_agent: bool,
    pub packs: Vec<PathBuf>,
    #[serde(default)]
    pub ctl_path: Option<PathBuf>,
}

impl StandaloneConfig {
    pub(super) fn resolve_paths(
        &mut self,
        base: &std::path::Path,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        validate_wsl_path(&self.state_dir)?;
        validate_wsl_path(&self.sqlite_path)?;
        self.state_dir = absolute_from(base, self.state_dir.clone());
        self.sqlite_path = absolute_from(base, self.sqlite_path.clone());
        for pack in &mut self.packs {
            validate_wsl_path(pack)?;
            *pack = absolute_from(base, pack.clone());
            if !pack.exists() {
                return Err(
                    format!("standalone pack path does not exist: {}", pack.display()).into(),
                );
            }
        }
        if let Some(path) = &mut self.ctl_path {
            validate_wsl_path(path)?;
            *path = absolute_from(base, path.clone());
        }
        Ok(())
    }
}
