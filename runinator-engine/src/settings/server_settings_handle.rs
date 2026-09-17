#[allow(unused_imports)]
use super::*;

/// Cheap, cloneable snapshot shared by all loops in an engine replica.
#[derive(Clone)]
pub struct ServerSettingsHandle {
    pub(super) current: Arc<RwLock<ServerSettings>>,
    pub(super) configured: Arc<AtomicBool>,
}

impl ServerSettingsHandle {
    pub async fn load<T: RuntimeStore + SettingStore>(db: &T) -> Result<Self, SendableError> {
        let persisted = load_persisted_server_settings(db).await?;
        let configured = persisted.is_some();
        let current = match persisted {
            Some(settings) => settings,
            None => load_server_settings(db).await?,
        };
        Ok(Self {
            current: Arc::new(RwLock::new(current)),
            configured: Arc::new(AtomicBool::new(configured)),
        })
    }

    pub fn current(&self) -> ServerSettings {
        self.current
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }

    /// Whether an administrator has saved the unified policy at least once.
    pub fn configured(&self) -> bool {
        self.configured.load(Ordering::SeqCst)
    }

    pub(super) async fn refresh<T: RuntimeStore + SettingStore>(
        &self,
        db: &T,
    ) -> Result<(), SendableError> {
        let persisted = load_persisted_server_settings(db).await?;
        let configured = persisted.is_some();
        let next = match persisted {
            Some(settings) => settings,
            None => load_server_settings(db).await?,
        };
        *self
            .current
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = next;
        self.configured.store(configured, Ordering::SeqCst);
        Ok(())
    }
}
