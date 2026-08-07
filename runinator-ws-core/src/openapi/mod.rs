//! the shared openapi documentation vocabulary.
//!
//! this is the model each handler crate writes its `DOCS` slice against; assembling those slices
//! into a served document is `runinator-ws`'s job, not this crate's.

pub mod docs;
pub mod examples;
