//! Immutable, paged workspace storage. See README.md and docs/FORMAT.md.
//!
//! The reference repository targets Unix; generic stores support portable workers. This library is
//! not a mounted POSIX filesystem or an OCI registry client.
#![forbid(unsafe_code)]
pub mod cache;
mod catalog;
mod chunkblock;
pub mod codec;
pub mod diff;
mod disk;
pub mod error;
pub mod errors;
pub mod gc;
pub mod id;
pub mod index;
mod io_util;
pub mod model;
pub mod namespace;
pub mod oci;
pub mod packs;
pub mod pages;
pub mod projection;
pub mod radix;
pub mod record;
pub mod repository;
pub mod staging;
pub mod store;
pub mod tiny;
pub mod transaction;
pub mod view;
pub use error::{Error, Result};
pub use id::Id;
pub use model::{Layout, Metadata};
pub use repository::{CommitPoint, Config, Repository, Snapshot, Transaction};
