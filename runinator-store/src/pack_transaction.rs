//! Transaction boundary for one compiled-pack import.
//!
//! A pack spans several persistence domains, so this is a use-case contract like
//! [`crate::RuntimeStore`], not a new polymorphic artifact repository. Implementations return an
//! isolated store handle whose ordinary role methods all participate in the same transaction.

use std::future::Future;

use runinator_models::errors::SendableError;

pub trait PackTransactionStore: Sized + Send + Sync {
    fn begin_pack_transaction(&self) -> impl Future<Output = Result<Self, SendableError>> + Send;

    fn commit_pack_transaction(&self) -> impl Future<Output = Result<(), SendableError>> + Send;

    fn rollback_pack_transaction(&self) -> impl Future<Output = Result<(), SendableError>> + Send;
}
