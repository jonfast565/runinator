//! the author-time analysis seam published for editor tooling.
//!
//! these items are the whole reason anything inside `lower` or `namespace` is public: the editor
//! crate (`runinator-wdl-ide`) needs to resolve a document's namespaces and lower its type
//! annotations to answer completion and hover queries. keep this list short — an editor feature
//! that needs a new item from the language core should get it added here deliberately, not by
//! widening a module.

pub use crate::lower::types::{NamedTypes, lower_type, lower_type_with, resolve_named_types};
pub use crate::namespace::resolve as resolve_namespaces;
