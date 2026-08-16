// runinator-wdl-syntax: the wdl surface syntax — everything between source text and the ast,
// in both directions. it owns the pest grammar, the ast the rest of the compiler walks, comment
// attachment, the canonical formatter, and `file(...)`/include resolution.
//
// this is the bottom of the wdl stack: it knows nothing about diagnostics, the workflow json
// model, or provider metadata, and depends on no other runinator crate but `runinator-models`.
// every other wdl crate reads a document through `parse_document` and this crate's `ast`.

pub mod ast;
pub mod comments;
pub mod errors;
pub mod format;
pub mod includes;
pub mod parser;
pub mod vocabulary;

pub use errors::{Span, WdlError};
pub use format::format_document;
pub use includes::included_file_paths;
pub use parser::{
    parse_condition_fragment, parse_do_fragment, parse_document, parse_expression_fragment,
    parse_pipeline_document, parse_secrets_document,
};
