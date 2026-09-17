#[allow(unused_imports)]
use super::*;

pub struct SqlStore<B: SqlBackend> {
    pub(super) backend: B,
}

impl<B: SqlBackend> SqlStore<B> {
    /// wrap an already-connected backend. driver modules expose a `new` that connects first; this
    /// is the seam a test or a caller with its own pool uses.
    pub fn from_backend(backend: B) -> Self {
        Self { backend }
    }

    pub fn backend(&self) -> &B {
        &self.backend
    }
}

impl<B: SqlBackend> SqlBackend for SqlStore<B> {
    type Db = B::Db;

    fn pool(&self) -> &Pool<Self::Db> {
        self.backend.pool()
    }

    fn from_pool(pool: Pool<Self::Db>) -> Self {
        SqlStore::from_backend(B::from_pool(pool))
    }

    fn dialect(&self) -> SqlDialect {
        self.backend.dialect()
    }

    fn init(&self, paths: &[String]) -> impl Future<Output = Result<(), SendableError>> + Send {
        self.backend.init(paths)
    }
}
