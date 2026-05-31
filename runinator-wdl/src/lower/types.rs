// maps the wdl `input { }` type expression onto RuninatorType.

use std::collections::BTreeMap;

use runinator_models::types::{RuninatorField, RuninatorType};

use crate::ast::TypeExpr;
use crate::errors::WdlError;

pub(crate) fn lower_type(type_expr: &TypeExpr) -> Result<RuninatorType, WdlError> {
    match type_expr {
        TypeExpr::Named(name) => Ok(named_type(name)),
        TypeExpr::Array(inner) => Ok(RuninatorType::Array(Box::new(lower_type(inner)?))),
        TypeExpr::Map(inner) => Ok(RuninatorType::Map(Box::new(lower_type(inner)?))),
        TypeExpr::Struct(fields) => {
            let mut mapped = BTreeMap::new();
            for field in fields {
                let ty = lower_type(&field.ty)?;
                let runinator_field = if field.optional {
                    RuninatorField::optional(ty)
                } else {
                    RuninatorField::required(ty)
                };
                mapped.insert(field.name.clone(), runinator_field);
            }
            Ok(RuninatorType::Struct {
                fields: mapped,
                additional: None,
            })
        }
        TypeExpr::Union(variants) => {
            let variants = variants
                .iter()
                .map(lower_type)
                .collect::<Result<Vec<_>, _>>()?;
            Ok(RuninatorType::Union(variants))
        }
    }
}

fn named_type(name: &str) -> RuninatorType {
    match name {
        "string" => RuninatorType::String,
        "integer" | "int" => RuninatorType::Integer,
        "number" | "float" => RuninatorType::Number,
        "boolean" | "bool" => RuninatorType::Boolean,
        "null" => RuninatorType::Null,
        "any" | "json" => RuninatorType::Any,
        // unknown named types degrade to Any rather than failing the whole compile.
        _ => RuninatorType::Any,
    }
}
