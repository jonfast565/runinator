//! the persistence contract shared by everything that reads or writes runinator state.
//!
//! this crate holds only trait definitions and the plain types they exchange. it has no sqlx
//! dependency and no backend, so a caller can depend on the operations it needs without compiling a
//! database driver, and a test can implement the traits in memory.
//!
//! `runinator-database` provides the concrete sqlite/postgres/mysql implementation and re-exports
//! these modules at their historical paths.

pub mod archive;
pub mod interfaces;
pub mod reducer_store;

pub use interfaces::DatabaseImpl;
pub use reducer_store::ReducerStore;
