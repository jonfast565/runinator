// argument-dependent result typing for the first-order intrinsics.
//
// a flat `ActionMetadata` signature cannot express "returns the element type of arg 0", so the
// polymorphic intrinsics declare `any`/`array<any>` in the catalog. the expression-inference
// engines (sema, runtime typing, completion) call `intrinsic_result_type` with the already-inferred
// argument types to recover a concrete result type, mirroring how the higher-order intrinsics are
// specially typed. this is the single source shared across the front end and runtime checker so the
// three engines cannot drift.

use std::collections::BTreeMap;

use runinator_models::types::{RuninatorField, RuninatorType};

/// the argument-dependent result type of a first-order intrinsic, given its inferred argument types
/// and any statically-known string keys (the caller extracts these from a literal key/key-list
/// argument, e.g. `at(obj, "id")` or `pick(obj, ["a", "b"])`; `None` when not a literal).
///
/// returns `None` when the intrinsic is not argument-polymorphic (the caller should use the
/// catalog's declared result type) or when the arguments carry too little type information to do
/// better than the catalog. genuinely-dynamic intrinsics (`parse_json`, `from_entries`, `http_*`
/// bodies) intentionally return `None`: their result shape is not derivable from argument types
/// alone, so they keep the catalog's honest `any`.
pub fn intrinsic_result_type(
    name: &str,
    args: &[RuninatorType],
    literal_keys: Option<&[String]>,
) -> Option<RuninatorType> {
    let arg0 = args.first();
    match name {
        // element accessors: the element type of the collection argument.
        "first" | "last" => arg0?.element_type(),
        "at" => match arg0? {
            // an array indexes to its element; a map keys to its value type.
            RuninatorType::Map(value) => Some((**value).clone()),
            // a struct access resolves the field when the key is a known literal, else falls back.
            RuninatorType::Struct { fields, additional } => {
                let key = literal_keys?.first()?;
                fields
                    .get(key)
                    .map(|field| field.ty.clone())
                    .or_else(|| additional.as_ref().map(|extra| (**extra).clone()))
            }
            other => other.element_type(),
        },
        // whole-array transforms that preserve the element type.
        "sort" | "reverse" | "unique" | "slice" => {
            Some(RuninatorType::array(arg0?.element_type()?))
        }
        // appending widens the element type to also admit the appended item.
        "append" => {
            let element = arg0?.element_type()?;
            let item = args.get(1)?;
            let widened = element.common_type(item).unwrap_or(RuninatorType::Any);
            Some(RuninatorType::array(widened))
        }
        // flatten drops one level of nesting: array<array<T>> -> array<T>.
        "flatten" => {
            let outer = arg0?.element_type()?;
            let inner = outer.element_type().unwrap_or(outer);
            Some(RuninatorType::array(inner))
        }
        // the values of a map become an array of its value type.
        "values" => match arg0? {
            RuninatorType::Map(value) => Some(RuninatorType::array((**value).clone())),
            _ => None,
        },
        // the entries of a map become an array of {key, value} pairs.
        "entries" => match arg0? {
            RuninatorType::Map(value) => Some(RuninatorType::array(RuninatorType::structure([
                ("key", RuninatorType::String),
                ("value", (**value).clone()),
            ]))),
            _ => None,
        },
        // `default(a, b)` yields `a` when non-null, else `b`: the common type of the two, or a
        // union when they are disjoint.
        "default" => Some(arg0?.unify(args.get(1)?)),
        // a shallow merge of two structs (right wins); non-struct operands fall through.
        "merge" => merge_result_type(arg0?, args.get(1)?),
        // keep only / drop the named keys of a struct, when the key list is statically known.
        "pick" => narrow_struct(arg0?, literal_keys?, true),
        "omit" => narrow_struct(arg0?, literal_keys?, false),
        _ => None,
    }
}

/// narrow a struct to (pick) or without (omit) the named keys. picking closes the struct to exactly
/// the named-and-present keys; omitting drops them and preserves the struct's openness. `None` for a
/// non-struct operand so the caller keeps the catalog `any`.
fn narrow_struct(ty: &RuninatorType, keys: &[String], keep: bool) -> Option<RuninatorType> {
    let RuninatorType::Struct { fields, additional } = ty else {
        return None;
    };
    let named: std::collections::HashSet<&str> = keys.iter().map(String::as_str).collect();
    let narrowed: BTreeMap<String, RuninatorField> = fields
        .iter()
        .filter(|(key, _)| named.contains(key.as_str()) == keep)
        .map(|(key, field)| (key.clone(), field.clone()))
        .collect();
    let additional = if keep { None } else { additional.clone() };
    Some(RuninatorType::Struct {
        fields: narrowed,
        additional,
    })
}

/// the type of `merge(a, b)` when both operands are structs: the union of their fields with the
/// right operand overriding, staying open when either input is open.
fn merge_result_type(left: &RuninatorType, right: &RuninatorType) -> Option<RuninatorType> {
    let (
        RuninatorType::Struct {
            fields: left_fields,
            additional: left_additional,
        },
        RuninatorType::Struct {
            fields: right_fields,
            additional: right_additional,
        },
    ) = (left, right)
    else {
        return None;
    };
    let mut fields: BTreeMap<String, RuninatorField> = left_fields.clone();
    fields.extend(right_fields.clone());
    let additional = if left_additional.is_some() || right_additional.is_some() {
        Some(Box::new(RuninatorType::Any))
    } else {
        None
    };
    Some(RuninatorType::Struct { fields, additional })
}
