#[allow(unused_imports)]
use super::*;

pub trait SqlBackend: Send + Sync + 'static {
    /// the concrete sqlx database driver.
    type Db: Database;

    /// the pool generic operations execute against.
    fn pool(&self) -> &Pool<Self::Db>;

    /// Rebuild this backend around an already-connected pool. Pack imports use this to create an
    /// isolated single-connection pool whose connection remains inside one outer transaction.
    fn from_pool(pool: Pool<Self::Db>) -> Self;

    /// the sql dialect used to render queries.
    fn dialect(&self) -> SqlDialect;

    /// render a `?`-placeholder template for this backend's dialect.
    fn render(&self, sql: &str) -> RenderedSql {
        RenderedSql(self.dialect().render(sql))
    }

    /// run embedded bootstrap work and any extra init scripts.
    ///
    /// sql bootstrap files are embedded per backend (the `sqlx::migrate!` macro is dir-specific),
    /// so this stays backend-owned rather than living in the generic operations blanket impl.
    fn init(&self, paths: &[String]) -> impl Future<Output = Result<(), SendableError>> + Send;
}
