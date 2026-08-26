//! One outer SQL transaction around every mutable database operation in a compiled-pack import.
//!
//! Existing role methods execute through a pool and some open their own transactions. The isolated
//! pool created here has exactly one connection; beginning through the driver's transaction
//! manager records transaction depth on that connection, so an inner `pool.begin()` becomes a
//! savepoint. No unrelated request can acquire this private pool.

use runinator_models::errors::SendableError;
use runinator_store::PackTransactionStore;
use sqlx::{Database, TransactionManager, pool::PoolOptions};

use crate::backend::{SqlBackend, SqlStore};

impl<B> PackTransactionStore for SqlStore<B>
where
    B: SqlBackend,
    <<B::Db as Database>::Connection as sqlx::Connection>::Options: Clone,
{
    async fn begin_pack_transaction(&self) -> Result<Self, SendableError> {
        let options = self.pool().connect_options().as_ref().clone();
        let pool = PoolOptions::<B::Db>::new()
            .max_connections(1)
            .min_connections(1)
            .connect_with(options)
            .await?;
        let mut connection = pool.acquire().await?;
        <B::Db as Database>::TransactionManager::begin(&mut connection, None).await?;
        drop(connection);
        Ok(SqlStore::from_backend(B::from_pool(pool)))
    }

    async fn commit_pack_transaction(&self) -> Result<(), SendableError> {
        let mut connection = self.pool().acquire().await?;
        <B::Db as Database>::TransactionManager::commit(&mut connection).await?;
        Ok(())
    }

    async fn rollback_pack_transaction(&self) -> Result<(), SendableError> {
        let mut connection = self.pool().acquire().await?;
        <B::Db as Database>::TransactionManager::rollback(&mut connection).await?;
        Ok(())
    }
}
