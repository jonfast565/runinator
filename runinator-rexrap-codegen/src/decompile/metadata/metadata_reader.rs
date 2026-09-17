#[allow(unused_imports)]
use super::*;

pub(crate) struct MetadataReader<'a> {
    pub(super) metadata: &'a Value,
}

impl<'a> MetadataReader<'a> {
    pub(crate) fn new(metadata: &'a Value) -> Self {
        Self { metadata }
    }

    pub(crate) fn portable(&self) -> Map {
        const OWNED_KEYS: &[&str] = &[
            "workspace",
            "rexrap",
            "triggers",
            "notifications",
            "concurrency",
            "watches",
            "interrupts",
            "correlation",
            "ingress",
            "functions",
            "artifact_refs",
            "managed_by",
            "namespace",
            "function",
        ];
        self.metadata
            .as_object()
            .map(|metadata| {
                metadata
                    .iter()
                    .filter(|(key, _)| !OWNED_KEYS.contains(&key.as_str()))
                    .map(|(key, value)| (key.clone(), value.clone()))
                    .collect()
            })
            .unwrap_or_default()
    }

    pub(crate) fn declared_types(&self) -> HashMap<String, String> {
        let mut types = HashMap::new();
        let Some(entries) = self.object("/rexrap/types") else {
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

    pub(crate) fn type_declarations(&self) -> Vec<(String, String)> {
        let Some(entries) = self.object("/rexrap/type_decls") else {
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

    pub(crate) fn output_type(&self) -> Option<RuninatorType> {
        self.value("/rexrap/output_type")?.decode().ok()
    }

    pub(crate) fn input_types(&self) -> HashMap<String, String> {
        let mut overrides = HashMap::new();
        let Some(entries) = self.object("/rexrap/input_types") else {
            return overrides;
        };
        for (name, value) in entries {
            if let Some(text) = value.as_str() {
                overrides.insert(name.clone(), text.to_string());
            }
        }
        overrides
    }

    pub(crate) fn triggers(&self) -> &[Value] {
        self.array("/triggers")
    }

    pub(crate) fn notifications(&self) -> &[Value] {
        self.array("/notifications")
    }

    pub(crate) fn interrupts(&self) -> &[Value] {
        self.array("/interrupts")
    }

    pub(crate) fn watches(&self) -> &[Value] {
        self.array("/watches")
    }

    pub(crate) fn alias_declarations(&self) -> Vec<(String, Vec<Value>)> {
        self.array("/rexrap/aliases")
            .iter()
            .filter_map(|entry| {
                Some((
                    entry.get("name").and_then(Value::as_str)?.to_string(),
                    entry.get("segs").and_then(Value::as_array)?.clone(),
                ))
            })
            .collect()
    }

    pub(crate) fn resource_imports(&self) -> Vec<ResourceImport> {
        self.array("/rexrap/imports")
            .iter()
            .filter_map(|entry| {
                Some(ResourceImport {
                    kind: entry.get("kind")?.as_str()?.to_string(),
                    path: entry.get("path")?.as_str()?.to_string(),
                    alias: entry.get("alias")?.as_str()?.to_string(),
                    revision: entry.get("revision").and_then(Value::as_i64),
                })
            })
            .collect()
    }

    pub(crate) fn spreads(&self) -> HashMap<String, Vec<Value>> {
        let mut spreads = HashMap::new();
        let Some(entries) = self.object("/rexrap/spreads") else {
            return spreads;
        };
        for (id, segments) in entries {
            if let Some(segments) = segments.as_array() {
                spreads.insert(id.clone(), segments.clone());
            }
        }
        spreads
    }

    pub(crate) fn control_ids(&self) -> HashSet<String> {
        self.array("/rexrap/control_ids")
            .iter()
            .filter_map(Value::as_str)
            .map(str::to_string)
            .collect()
    }

    pub(crate) fn control_vars(&self) -> HashMap<String, ControlVars> {
        self.object("/rexrap/control_vars")
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

    pub(crate) fn parallel_branches(&self) -> HashMap<String, ParallelSurface> {
        let mut result = HashMap::new();
        let Some(entries) = self.object("/rexrap/parallel_branches") else {
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

    pub(crate) fn functions(&self) -> Vec<FnEntry> {
        let signatures = self.object("/rexrap/functions");
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

    pub(crate) fn concurrency(&self) -> Option<&Value> {
        self.value("/concurrency")
    }

    pub(crate) fn correlation(&self) -> Option<&Value> {
        self.value("/correlation").filter(|value| !value.is_null())
    }

    pub(crate) fn ingress(&self) -> Option<&Value> {
        self.value("/ingress").filter(|value| !value.is_null())
    }

    pub(super) fn value(&self, pointer: &str) -> Option<&'a Value> {
        self.metadata.pointer(pointer)
    }

    pub(super) fn array(&self, pointer: &str) -> &'a [Value] {
        self.value(pointer)
            .and_then(Value::as_array)
            .map(Vec::as_slice)
            .unwrap_or_default()
    }

    pub(super) fn object(&self, pointer: &str) -> Option<&'a Map> {
        self.value(pointer).and_then(Value::as_object)
    }
}
