//! the author-time analysis seam published for editor tooling.
//!
//! these items are the whole reason `runinator-wdl-sema`'s `types` and `namespace` modules are
//! public: the editor crate (`runinator-wdl-ide`) needs to resolve a document's namespaces and
//! lower its type annotations to answer completion and hover queries. keep this list short — an
//! editor feature that needs a new item from the language core should get it added here
//! deliberately, not by reaching into a core crate directly.

pub use runinator_wdl_sema::namespace::resolve as resolve_namespaces;
pub use runinator_wdl_sema::types::{NamedTypes, lower_type, lower_type_with, resolve_named_types};
