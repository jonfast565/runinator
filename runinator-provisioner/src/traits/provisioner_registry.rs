#[allow(unused_imports)]
use super::*;

/// holds every configured provisioner so handlers can fan out or target one backend.
#[derive(Clone, Default)]
pub struct ProvisionerRegistry {
    pub(super) provisioners: Vec<Arc<dyn Provisioner>>,
}

impl ProvisionerRegistry {
    pub fn new(provisioners: Vec<Arc<dyn Provisioner>>) -> Self {
        Self { provisioners }
    }

    pub fn is_empty(&self) -> bool {
        self.provisioners.is_empty()
    }

    /// the provisioner for a backend, if configured.
    pub fn get(&self, backend: ProvisionBackend) -> Option<Arc<dyn Provisioner>> {
        self.provisioners
            .iter()
            .find(|p| p.backend() == backend)
            .cloned()
    }

    /// resolve a backend or return the standard unknown-backend error.
    pub fn require(
        &self,
        backend: ProvisionBackend,
    ) -> Result<Arc<dyn Provisioner>, SendableError> {
        self.get(backend)
            .ok_or_else(|| UNKNOWN_BACKEND.error(backend.as_str()))
    }

    /// describe every configured backend and the kinds it supports.
    pub async fn backends(&self) -> Vec<NodeBackendInfo> {
        let mut out = Vec::with_capacity(self.provisioners.len());
        for provisioner in &self.provisioners {
            out.push(NodeBackendInfo {
                backend: provisioner.backend(),
                kinds: provisioner.supported_kinds(),
                available: provisioner.available().await,
            });
        }
        out
    }

    /// list groups across every configured backend, skipping backends that error.
    pub async fn list_all(&self) -> Vec<ProvisionedGroup> {
        let mut groups = Vec::new();
        for provisioner in &self.provisioners {
            match provisioner.list().await {
                Ok(mut found) => groups.append(&mut found),
                Err(err) => log::warn!(
                    "provisioner backend {} list failed: {err}",
                    provisioner.backend().as_str()
                ),
            }
        }
        groups
    }
}
