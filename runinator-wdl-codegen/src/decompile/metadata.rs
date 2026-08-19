use std::collections::{HashMap, HashSet};

use runinator_models::{
    types::RuninatorType,
    value::{Map, Value},
};

use super::expr;

#[derive(Debug, Clone, Default)]
pub(super) struct ParallelSurface {
    pub labels: Vec<Option<String>>,
    pub selected: Option<Vec<String>>,
    pub stops: Vec<String>,
}

pub(super) struct MetadataReader<'a> {
    metadata: &'a Value,
}

impl<'a> MetadataReader<'a> {
    pub(super) fn new(metadata: &'a Value) -> Self {
        Self { metadata }
    }

    pub(super) fn declared_types(&self) -> HashMap<String, String> {
        let mut types = HashMap::new();
        let Some(entries) = self.object("/wdl/types") else {
            return types;
        };
        for (id, value) in entries {
            if let Some(text) = value.as_str() {
                types.insert(id.clone(), text.to_string());
            } else if let Ok(ty) = value.decode::<RuninatorType>() {
                types.insert(id.clone(), expr::render_type(&ty));
            }
        }
        types
    }

    pub(super) fn type_declarations(&self) -> Vec<(String, String)> {
        let Some(entries) = self.object("/wdl/type_decls") else {
            return Vec::new();
        };
        entries
            .iter()
            .filter_map(|(name, value)| {
                value
                    .as_str()
                    .map(|text| (name.clone(), text.to_string()))
                    .or_else(|| {
                        value
                            .decode::<RuninatorType>()
                            .ok()
                            .map(|ty| (name.clone(), expr::render_type(&ty)))
                    })
            })
            .collect()
    }

    pub(super) fn output_type(&self) -> Option<RuninatorType> {
        self.value("/wdl/output_type")?.decode().ok()
    }

    pub(super) fn input_types(&self) -> HashMap<String, String> {
        let mut overrides = HashMap::new();
        let Some(entries) = self.object("/wdl/input_types") else {
            return overrides;
        };
        for (name, value) in entries {
            if let Some(text) = value.as_str() {
                overrides.insert(name.clone(), text.to_string());
            }
        }
        overrides
    }

    pub(super) fn triggers(&self) -> &[Value] {
        self.array("/triggers")
    }

    pub(super) fn notifications(&self) -> &[Value] {
        self.array("/notifications")
    }

    pub(super) fn interrupts(&self) -> &[Value] {
        self.array("/interrupts")
    }

    pub(super) fn watches(&self) -> &[Value] {
        self.array("/watches")
    }

    pub(super) fn alias_declarations(&self) -> Vec<(String, Vec<Value>)> {
        self.array("/wdl/aliases")
            .iter()
            .filter_map(|entry| {
                Some((
                    entry.get("name").and_then(Value::as_str)?.to_string(),
                    entry.get("segs").and_then(Value::as_array)?.clone(),
                ))
            })
            .collect()
    }

    pub(super) fn spreads(&self) -> HashMap<String, Vec<Value>> {
        let mut spreads = HashMap::new();
        let Some(entries) = self.object("/wdl/spreads") else {
            return spreads;
        };
        for (id, segments) in entries {
            if let Some(segments) = segments.as_array() {
                spreads.insert(id.clone(), segments.clone());
            }
        }
        spreads
    }

    pub(super) fn control_ids(&self) -> HashSet<String> {
        self.array("/wdl/control_ids")
            .iter()
            .filter_map(Value::as_str)
            .map(str::to_string)
            .collect()
    }

    pub(super) fn control_vars(&self) -> HashMap<String, ControlVars> {
        self.object("/wdl/control_vars")
            .map(|entries| {
                entries
                    .iter()
                    .filter_map(|(id, value)| {
                        let vars = value.as_object()?;
                        let item = vars.get("item")?.as_str()?.to_string();
                        let index = vars
                            .get("index")
                            .and_then(Value::as_str)
                            .map(str::to_string);
                        let item_type = vars
                            .get("item_type")
                            .and_then(Value::as_str)
                            .map(str::to_string);
                        Some((
                            id.clone(),
                            ControlVars {
                                item,
                                item_type,
                                index,
                            },
                        ))
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    pub(super) fn parallel_branches(&self) -> HashMap<String, ParallelSurface> {
        let mut result = HashMap::new();
        let Some(entries) = self.object("/wdl/parallel_branches") else {
            return result;
        };
        for (id, value) in entries {
            let Some(surface) = value.as_object() else {
                continue;
            };
            let labels = surface
                .get("labels")
                .and_then(Value::as_array)
                .map(|labels| {
                    labels
                        .iter()
                        .map(|label| label.as_str().map(str::to_string))
                        .collect()
                })
                .unwrap_or_default();
            let selected = surface
                .get("selected")
                .and_then(Value::as_array)
                .map(|labels| {
                    labels
                        .iter()
                        .filter_map(Value::as_str)
                        .map(str::to_string)
                        .collect()
                });
            let stops = surface
                .get("stops")
                .and_then(Value::as_array)
                .map(|stops| {
                    stops
                        .iter()
                        .filter_map(Value::as_str)
                        .map(str::to_string)
                        .collect()
                })
                .unwrap_or_default();
            result.insert(
                id.clone(),
                ParallelSurface {
                    labels,
                    selected,
                    stops,
                },
            );
        }
        result
    }

    pub(super) fn functions(&self) -> Vec<FnEntry> {
        let signatures = self.object("/wdl/functions");
        self.array("/functions")
            .iter()
            .filter_map(|entry| {
                let object = entry.as_object()?;
                let name = object.get("name").and_then(Value::as_str)?.to_string();
                let recursive = object
                    .get("recursive")
                    .and_then(Value::as_object)
                    .and_then(|recursive| recursive.get("max_depth"))
                    .and_then(Value::as_i64);
                let body = match object.get("program").and_then(Value::as_array) {
                    Some(program) => FnBodyForm::Program(program.clone()),
                    None => FnBodyForm::Expr(object.get("body").cloned().unwrap_or(Value::Null)),
                };
                let signature = signatures
                    .and_then(|map| map.get(&name))
                    .and_then(Value::as_str)
                    .map(str::to_string)
                    .unwrap_or_else(|| fallback_signature(object));
                Some(FnEntry {
                    name,
                    signature,
                    recursive,
                    body,
                })
            })
            .collect()
    }

    pub(super) fn concurrency(&self) -> Option<&Value> {
        self.value("/concurrency")
    }

    pub(super) fn correlation(&self) -> Option<&Value> {
        self.value("/correlation").filter(|value| !value.is_null())
    }

    fn value(&self, pointer: &str) -> Option<&'a Value> {
        self.metadata.pointer(pointer)
    }

    fn array(&self, pointer: &str) -> &'a [Value] {
        self.value(pointer)
            .and_then(Value::as_array)
            .map(Vec::as_slice)
            .unwrap_or_default()
    }

    fn object(&self, pointer: &str) -> Option<&'a Map> {
        self.value(pointer).and_then(Value::as_object)
    }
}

pub(super) struct ControlVars {
    pub(super) item: String,
    pub(super) item_type: Option<String>,
    pub(super) index: Option<String>,
}

/// a `fn` definition recovered for decompilation.
pub(super) struct FnEntry {
    pub(super) name: String,
    pub(super) signature: String,
    pub(super) recursive: Option<i64>,
    pub(super) body: FnBodyForm,
}

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
