#[allow(unused_imports)]
use super::*;

#[derive(Clone)]
pub(super) struct HostState {
    pub(super) token: Arc<String>,
    pub(super) paths: Arc<Vec<PathBuf>>,
    pub(super) limits: HostLimits,
    pub(super) catalog: Arc<RwLock<BTreeMap<String, AdapterKindCatalogEntry>>>,
}
