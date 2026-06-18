// maps the wdl `params { }` type expression onto RuninatorType.

use std::collections::{BTreeMap, HashSet};

use runinator_models::types::{RuninatorField, RuninatorType};

use crate::ast::{TypeDecl, TypeExpr};
use crate::errors::WdlError;

/// a table of resolved `type <Name>` declarations consulted when a named type is referenced.
pub(crate) type NamedTypes = BTreeMap<String, RuninatorType>;

/// resolve `type <Name>` declarations into `RuninatorType` in dependency order. each declaration
/// may reference earlier ones; cyclic or duplicate declarations are rejected. shared by lowering
/// and sema so both type-check against the same resolved names.
pub(crate) fn resolve_named_types(decls: &[TypeDecl]) -> Result<NamedTypes, WdlError> {
    let mut named = NamedTypes::new();
    let mut seen = HashSet::new();
    for decl in decls {
        if !seen.insert(decl.name.as_str()) {
            return Err(WdlError::lower(format!(
                "duplicate type declaration '{}'",
                decl.name
            )));
        }
    }
    let mut pending: Vec<&TypeDecl> = decls.iter().collect();
    while !pending.is_empty() {
        let before = pending.len();
        pending.retain(|decl| {
            if type_refs_resolved(&decl.ty, &named)
                && let Ok(ty) = lower_type_with(&decl.ty, &named)
            {
                named.insert(decl.name.clone(), ty);
                return false;
            }
            true
        });
        if pending.len() == before {
            let names = pending
                .iter()
                .map(|decl| decl.name.clone())
                .collect::<Vec<_>>()
                .join(", ");
            return Err(WdlError::lower(format!(
                "cyclic or unresolved type declarations: {names}"
            )));
        }
    }
    Ok(named)
}

/// whether every named reference in `ty` is a primitive or an already-resolved declaration.
fn type_refs_resolved(ty: &TypeExpr, named: &NamedTypes) -> bool {
    match ty {
        TypeExpr::Named(name) => is_primitive_type_name(name) || named.contains_key(name),
        TypeExpr::Enum(_) => true,
        TypeExpr::Range { base, .. } => type_refs_resolved(base, named),
        TypeExpr::Array(inner) | TypeExpr::Map(inner) => type_refs_resolved(inner, named),
        TypeExpr::Union(variants) => variants.iter().all(|v| type_refs_resolved(v, named)),
        TypeExpr::Struct { fields, additional } => {
            fields.iter().all(|f| type_refs_resolved(&f.ty, named))
                && additional
                    .as_ref()
                    .is_none_or(|a| type_refs_resolved(a, named))
        }
    }
}

fn is_primitive_type_name(name: &str) -> bool {
    matches!(
        name,
        "string"
            | "integer"
            | "int"
            | "number"
            | "float"
            | "boolean"
            | "bool"
            | "duration"
            | "null"
            | "any"
            | "json"
    )
}

pub(crate) fn lower_type(type_expr: &TypeExpr) -> Result<RuninatorType, WdlError> {
    lower_type_with(type_expr, &NamedTypes::new())
}

pub(crate) fn lower_type_with(
    type_expr: &TypeExpr,
    named: &NamedTypes,
) -> Result<RuninatorType, WdlError> {
    match type_expr {
        TypeExpr::Named(name) => Ok(named_type(name, named)),
        TypeExpr::Enum(values) => Ok(RuninatorType::Enum(values.clone())),
        TypeExpr::Range { base, min, max } => Ok(RuninatorType::Range {
            base: Box::new(lower_type_with(base, named)?),
            min: min.clone(),
            max: max.clone(),
        }),
        TypeExpr::Array(inner) => Ok(RuninatorType::Array(Box::new(lower_type_with(
            inner, named,
        )?))),
        TypeExpr::Map(inner) => Ok(RuninatorType::Map(Box::new(lower_type_with(inner, named)?))),
        TypeExpr::Struct { fields, additional } => {
            let mut mapped = BTreeMap::new();
            for field in fields {
                let ty = lower_type_with(&field.ty, named)?;
                let runinator_field = if field.optional {
                    RuninatorField::optional(ty)
                } else {
                    RuninatorField::required(ty)
                };
                mapped.insert(field.name.clone(), runinator_field);
            }
            let additional = additional
                .as_ref()
                .map(|ty| lower_type_with(ty, named))
                .transpose()?
                .map(Box::new);
            Ok(RuninatorType::Struct {
                fields: mapped,
                additional,
            })
        }
        TypeExpr::Union(variants) => {
            let variants = variants
                .iter()
                .map(|variant| lower_type_with(variant, named))
                .collect::<Result<Vec<_>, _>>()?;
            Ok(RuninatorType::Union(variants))
        }
    }
}

fn named_type(name: &str, named: &NamedTypes) -> RuninatorType {
    match name {
        "string" => RuninatorType::String,
        "integer" | "int" => RuninatorType::Integer,
        "number" | "float" => RuninatorType::Number,
        "duration" => RuninatorType::Duration,
        "boolean" | "bool" => RuninatorType::Boolean,
        "null" => RuninatorType::Null,
        "any" | "json" => RuninatorType::Any,
        // a declared `type <Name>` resolves to its definition; unknown names degrade to Any
        // rather than failing the whole compile.
        other => named.get(other).cloned().unwrap_or(RuninatorType::Any),
    }
}
