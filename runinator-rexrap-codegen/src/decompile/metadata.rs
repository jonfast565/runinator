use std::collections::{HashMap, HashSet};

use runinator_models::{
    types::RuninatorType,
    value::{Map, Value},
};

use super::expr;

/// a `fn` definition recovered for decompilation.

/// a recovered function body: a single lowered expression or a compute program.
pub(super) enum FnBodyForm {
    Expr(Value),
    Program(Vec<Value>),
}

fn fallback_signature(object: &Map) -> String {
    let params = object
        .get("params")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|param| {
                    param.as_str().map(str::to_string).or_else(|| {
                        param
                            .as_object()
                            .and_then(|param| param.get("name"))
                            .and_then(Value::as_str)
                            .map(str::to_string)
                    })
                })
                .map(|name| format!("{name}: any"))
                .collect::<Vec<_>>()
                .join(", ")
        })
        .unwrap_or_default();
    format!("({params})")
}

mod parallel_surface;
pub(super) use parallel_surface::ParallelSurface;

mod resource_import;
pub(super) use resource_import::ResourceImport;

mod metadata_reader;
pub(super) use metadata_reader::MetadataReader;

mod control_vars;
pub(super) use control_vars::ControlVars;

mod fn_entry;
pub(super) use fn_entry::FnEntry;
