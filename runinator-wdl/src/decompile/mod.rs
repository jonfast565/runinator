// reconstructs wdl source from a WorkflowDefinition. it walks the graph from the start node,
// recovering structured blocks (for/while/if/match/parallel/race/try) where possible. each
// node is emitted exactly once; every other edge into it (fail/reject/timeout arrows, back
// edges, and fan-in convergence) is rendered as an explicit `-> label` arrow, and nodes
// reached only by such arrows are emitted as top-level labelled statements. this lets
// arbitrary graphs round-trip, since wdl labels are global.

mod expr;

use std::collections::{HashMap, HashSet, VecDeque};

use runinator_models::types::RuninatorType;
use runinator_models::value::{Map, Value};
use runinator_models::workflows::{
    WorkflowDefinition, WorkflowNode, WorkflowNodeKind, WorkflowRetry, WorkflowRetryClass,
    WorkflowTransitions, WorkflowWaitSeconds,
};

use crate::errors::WdlError;

/// options controlling how a definition is rendered back to wdl.
#[derive(Debug, Clone, Default)]
pub struct DecompileOptions {
    /// emit the canonical fully-explicit form: a `start ->` line, an id and happy-path arrow on
    /// every node, and every defaulted value (timeout/retry/limit/concurrency/approval type).
    pub explicit: bool,
}

pub(super) struct Decompiler<'a> {
    nodes: HashMap<String, &'a WorkflowNode>,
    end_ids: HashSet<String>,
    fail_ids: HashSet<String>,
    // surface every implicit construct (ids, edges, defaults) instead of the terse form.
    explicit: bool,
    loop_vars: Vec<(String, String)>,
    // declared `node <id>: <type>` annotations recovered from graph metadata, kept as rendered wdl
    // type text so declared type-name references survive the round trip.
    declared_types: HashMap<String, String>,
    // surface-form overrides for top-level workflow parameter fields that reference a declared
    // type name.
    input_types: HashMap<String, String>,
    // header alias declarations recovered from graph metadata, in declaration order.
    alias_decls: Vec<(String, Vec<Value>)>,
    // per-node `...alias` spread recipes (node id -> recipe segments) recovered from metadata.
    spreads: HashMap<String, Vec<Value>>,
    // control-block ids explicitly authored in WDL, recovered from metadata.
    control_ids: HashSet<String>,
    // node ids already emitted; each node is emitted exactly once and every other edge into
    // it becomes an explicit `-> label` arrow (labels are global, so this round-trips).
    visited: HashSet<String>,
    // nodes reached only by non-linear edges (fail/reject/timeout, or convergence) that must
    // still be emitted as top-level labelled statements; drained after the main walk.
    worklist: VecDeque<String>,
    queued: HashSet<String>,
    out: String,
    indent: usize,
}

pub fn decompile_definition(
    definition: &WorkflowDefinition,
    options: &DecompileOptions,
) -> Result<String, WdlError> {
    let graph = &definition.definition;
    let mut nodes = HashMap::new();
    let mut end_ids = HashSet::new();
    let mut fail_ids = HashSet::new();
    for node in &graph.nodes {
        nodes.insert(node.id.clone(), node);
        match node.kind {
            WorkflowNodeKind::End => {
                end_ids.insert(node.id.clone());
            }
            WorkflowNodeKind::Fail => {
                fail_ids.insert(node.id.clone());
            }
            _ => {}
        }
    }

    let declared_types = read_declared_types(&graph.metadata);
    let input_types = read_input_types(&graph.metadata);
    let alias_decls = read_alias_decls(&graph.metadata);
    let spreads = read_spreads(&graph.metadata);
    let control_ids = read_control_ids(&graph.metadata);

    let mut decompiler = Decompiler {
        nodes,
        end_ids,
        fail_ids,
        explicit: options.explicit,
        loop_vars: Vec::new(),
        declared_types,
        input_types,
        alias_decls,
        spreads,
        control_ids,
        visited: HashSet::new(),
        worklist: VecDeque::new(),
        queued: HashSet::new(),
        out: String::new(),
        indent: 0,
    };

    // top-level `fn` definitions render before the workflow block (document = func_def* ~ workflow).
    decompiler.emit_functions(&read_functions(&graph.metadata))?;

    if let Some(namespace) = &definition.namespace {
        decompiler.line(&format!("namespace {namespace} {{"));
        decompiler.indent += 1;
    }

    let returns = read_output_type(&graph.metadata)
        .map(|ty| format!(" returns {}", expr::render_type(&ty)))
        .unwrap_or_default();
    decompiler.line(&format!(
        "workflow {} v{}{} {{",
        quote(&definition.name),
        definition.version,
        returns
    ));
    decompiler.indent += 1;
    decompiler.emit_params(&definition.input_type)?;
    decompiler.emit_triggers(&read_triggers(&graph.metadata))?;
    decompiler.emit_watches(&read_watches(&graph.metadata))?;
    decompiler.emit_type_decls(&read_type_decls(&graph.metadata))?;
    decompiler.emit_alias_decls()?;

    let start = graph
        .start
        .as_deref()
        .ok_or_else(|| WdlError::Decompile("workflow has no start node".into()))?;
    let entry = decompiler
        .nodes
        .get(start)
        .and_then(|node| node.transitions.next.as_ref())
        .map(|target| target.as_str().to_string());
    if let Some(entry) = entry {
        // the explicit form names the otherwise-synthetic start edge.
        if decompiler.explicit {
            let label = decompiler.target_label(&entry);
            decompiler.line(&format!("start -> {label}"));
        }
        decompiler.emit_region(&entry, None)?;
    }

    // emit any nodes reached only by fail/reject/timeout arrows or convergence as top-level
    // labelled statements; references to them elsewhere were rendered as `-> label` arrows.
    while let Some(id) = decompiler.worklist.pop_front() {
        if decompiler.visited.contains(&id) || !decompiler.nodes.contains_key(id.as_str()) {
            continue;
        }
        decompiler.emit_region(&id, None)?;
    }

    decompiler.indent -= 1;
    decompiler.line("}");
    if definition.namespace.is_some() {
        decompiler.indent -= 1;
        decompiler.line("}");
    }
    Ok(decompiler.out)
}

/// recover declared `let` types from the graph metadata sidecar at `/wdl/types` as rendered wdl
/// type text. newer graphs store the surface string directly; older graphs stored a native
/// `RuninatorType` schema, which is rendered back for compatibility.
// render a `.retry(...)` modifier from the model, or `None` when every field is at its default and
// `explicit` rendering is off. mirrors the WDL named-arg surface so compile->decompile round-trips.
fn decompile_retry(retry: &WorkflowRetry, explicit: bool) -> Option<String> {
    let on = match retry.retry_on {
        WorkflowRetryClass::Any => None,
        WorkflowRetryClass::Failure => Some("failure"),
        WorkflowRetryClass::Timeout => Some("timeout"),
    };
    let custom = retry.backoff_base_seconds != 1
        || retry.backoff_max_seconds != 300
        || retry.jitter
        || on.is_some();
    if !explicit && retry.max_attempts <= 1 && !custom {
        return None;
    }
    let mut args = vec![retry.max_attempts.to_string()];
    if retry.backoff_base_seconds != 1 {
        args.push(format!("backoff: {}s", retry.backoff_base_seconds));
    }
    if retry.backoff_max_seconds != 300 {
        args.push(format!("max: {}s", retry.backoff_max_seconds));
    }
    if retry.jitter {
        args.push("jitter: true".to_string());
    }
    if let Some(on) = on {
        args.push(format!("on: {on}"));
    }
    Some(format!(".retry({})", args.join(", ")))
}

fn read_declared_types(metadata: &Value) -> HashMap<String, String> {
    let mut types = HashMap::new();
    let Some(entries) = metadata.pointer("/wdl/types").and_then(Value::as_object) else {
        return types;
    };
    for (id, value) in entries {
        if let Some(text) = value.as_str() {
            types.insert(id.clone(), text.to_string());
            continue;
        }
        let json = serde_json::Value::from(value.clone());
        if let Ok(ty) = serde_json::from_value::<RuninatorType>(json) {
            types.insert(id.clone(), expr::render_type(&ty));
        }
    }
    types
}

/// recover named `type <Name>` declarations from the metadata sidecar at `/wdl/type_decls` as
/// rendered surface strings, preserving declaration order. older graphs stored a native schema,
/// which is rendered back for compatibility.
fn read_type_decls(metadata: &Value) -> Vec<(String, String)> {
    let Some(entries) = metadata
        .pointer("/wdl/type_decls")
        .and_then(Value::as_object)
    else {
        return Vec::new();
    };
    entries
        .iter()
        .filter_map(|(name, value)| {
            if let Some(text) = value.as_str() {
                return Some((name.clone(), text.to_string()));
            }
            let json = serde_json::Value::from(value.clone());
            serde_json::from_value::<RuninatorType>(json)
                .ok()
                .map(|ty| (name.clone(), expr::render_type(&ty)))
        })
        .collect()
}

fn read_output_type(metadata: &Value) -> Option<runinator_models::types::RuninatorType> {
    let value = metadata.pointer("/wdl/output_type")?.clone();
    serde_json::from_value(value.into()).ok()
}

/// recover surface-form overrides for top-level workflow parameter fields at `/wdl/input_types`.
fn read_input_types(metadata: &Value) -> HashMap<String, String> {
    let mut overrides = HashMap::new();
    let Some(entries) = metadata
        .pointer("/wdl/input_types")
        .and_then(Value::as_object)
    else {
        return overrides;
    };
    for (name, value) in entries {
        if let Some(text) = value.as_str() {
            overrides.insert(name.clone(), text.to_string());
        }
    }
    overrides
}

/// recover header `trigger` specs from runtime metadata at `/triggers`.
fn read_triggers(metadata: &Value) -> Vec<Value> {
    metadata
        .pointer("/triggers")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default()
}

/// recover header `watch` guards from runtime metadata at `/watches`.
fn read_watches(metadata: &Value) -> Vec<Value> {
    metadata
        .pointer("/watches")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default()
}

/// recover header alias declarations from the metadata sidecar at `/wdl/aliases`, preserving
/// declaration order. each alias is a `(name, recipe-segments)` pair.
fn read_alias_decls(metadata: &Value) -> Vec<(String, Vec<Value>)> {
    let Some(entries) = metadata.pointer("/wdl/aliases").and_then(Value::as_array) else {
        return Vec::new();
    };
    entries
        .iter()
        .filter_map(|entry| {
            let name = entry.get("name").and_then(Value::as_str)?.to_string();
            let segs = entry.get("segs").and_then(Value::as_array)?.clone();
            Some((name, segs))
        })
        .collect()
}

/// recover per-node `...alias` spread recipes from the metadata sidecar at `/wdl/spreads`.
fn read_spreads(metadata: &Value) -> HashMap<String, Vec<Value>> {
    let mut spreads = HashMap::new();
    let Some(entries) = metadata.pointer("/wdl/spreads").and_then(Value::as_object) else {
        return spreads;
    };
    for (id, segs) in entries {
        if let Some(segs) = segs.as_array() {
            spreads.insert(id.clone(), segs.clone());
        }
    }
    spreads
}

/// recover control-block ids that were explicitly authored with `@id(...)`.
fn read_control_ids(metadata: &Value) -> HashSet<String> {
    metadata
        .pointer("/wdl/control_ids")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

/// a `fn` definition recovered for decompilation: its name, its surface signature (`(params) -> ret`,
/// from the `/wdl/functions` hint), an optional recursion cap, and the lowered body form.
struct FnEntry {
    name: String,
    signature: String,
    recursive: Option<i64>,
    body: FnBodyForm,
}

/// a recovered function body: a single lowered expression, or a `$let`/`$return`/`$if` program.
enum FnBodyForm {
    Expr(Value),
    Program(Vec<Value>),
}

/// recover user `fn` definitions from the runtime `/functions` array, pairing each with its surface
/// signature from the `/wdl/functions` hint (falling back to `any`-typed params for older graphs).
fn read_functions(metadata: &Value) -> Vec<FnEntry> {
    let Some(entries) = metadata.pointer("/functions").and_then(Value::as_array) else {
        return Vec::new();
    };
    let signatures = metadata
        .pointer("/wdl/functions")
        .and_then(Value::as_object);
    entries
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

/// build an `any`-typed signature `(p1: any, p2: any)` from a function's parameter names, used when
/// the `/wdl/functions` surface hint is absent (older graphs lowered before the hint existed).
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

impl<'a> Decompiler<'a> {
    fn loop_var(&self, node_id: &str) -> Option<String> {
        self.loop_vars
            .iter()
            .rev()
            .find(|(id, _)| id == node_id)
            .map(|(_, var)| var.clone())
    }

    fn line(&mut self, text: &str) {
        for _ in 0..self.indent {
            self.out.push_str("    ");
        }
        self.out.push_str(text);
        self.out.push('\n');
    }

    fn emit_params(&mut self, input_type: &RuninatorType) -> Result<(), WdlError> {
        let RuninatorType::Struct { fields, additional } = input_type else {
            return Ok(());
        };
        if fields.is_empty() && additional.is_none() {
            return Ok(());
        }
        self.line("params {");
        self.indent += 1;
        for (name, field) in fields {
            // prefer a recorded surface form (which preserves a declared type name) over the
            // expanded structural rendering.
            let rendered = self
                .input_types
                .get(name)
                .cloned()
                .unwrap_or_else(|| expr::render_type(&field.ty));
            // a default implies optionality, so it replaces the `?` marker rather than adding to it.
            if let Some(default) = &field.default {
                let default_text = self.expr(default)?;
                self.line(&format!("{name}: {rendered} = {default_text}"));
                continue;
            }
            let mark = if field.required { "" } else { "?" };
            self.line(&format!("{name}{mark}: {rendered}"));
        }
        if let Some(additional) = additional {
            self.line(&format!("...: {}", expr::render_type(additional)));
        }
        self.indent -= 1;
        self.line("}");
        self.out.push('\n');
        Ok(())
    }

    /// emit header `trigger cron "..."` declarations recovered from runtime metadata.
    /// emit recovered `fn` definitions ahead of the workflow block. an expression body renders
    /// `= <expr>`; a block body renders `= { <compute lines> }` reusing the compute-line renderer.
    fn emit_functions(&mut self, functions: &[FnEntry]) -> Result<(), WdlError> {
        for function in functions {
            if let Some(depth) = function.recursive {
                self.line(&format!("@recursive(max_depth: {depth})"));
            }
            match &function.body {
                FnBodyForm::Expr(value) => {
                    let rendered = self.expr(value)?;
                    self.line(&format!(
                        "fn {}{} = {rendered}",
                        function.name, function.signature
                    ));
                }
                FnBodyForm::Program(program) => {
                    let base = self.indent;
                    let mut out = format!("fn {}{} = {{\n", function.name, function.signature);
                    self.render_compute_lines(&mut out, program, base + 1)?;
                    out.push_str(&"    ".repeat(base));
                    out.push('}');
                    self.line(&out);
                }
            }
        }
        Ok(())
    }

    fn emit_triggers(&mut self, triggers: &[Value]) -> Result<(), WdlError> {
        if triggers.is_empty() {
            return Ok(());
        }
        for trigger in triggers {
            let cron = trigger
                .get("cron")
                .and_then(Value::as_str)
                .ok_or_else(|| WdlError::Decompile("trigger missing cron expression".into()))?;
            let params = trigger.get("parameters");
            let has_params = params
                .and_then(Value::as_object)
                .is_some_and(|object| !object.is_empty());
            let mut text = format!("trigger cron {}", quote(cron));
            if has_params {
                let rendered = self.expr(params.unwrap_or(&Value::Null))?;
                text.push_str(&format!(" with {rendered}"));
            }
            if trigger.get("enabled").and_then(Value::as_bool) == Some(false) {
                text.push_str(" disabled");
            }
            if let (Some(start), Some(end)) = (
                trigger.get("blackout_start").and_then(Value::as_str),
                trigger.get("blackout_end").and_then(Value::as_str),
            ) {
                text.push_str(&format!(" blackout {} to {}", quote(start), quote(end)));
            }
            self.line(&text);
        }
        self.out.push('\n');
        Ok(())
    }

    /// emit header `watch <cond> -> <target>` guards recovered from runtime metadata.
    fn emit_watches(&mut self, watches: &[Value]) -> Result<(), WdlError> {
        if watches.is_empty() {
            return Ok(());
        }
        for watch in watches {
            let condition = watch
                .get("condition")
                .ok_or_else(|| WdlError::Decompile("watch missing condition".into()))?;
            let handler = watch
                .get("handler")
                .and_then(Value::as_str)
                .ok_or_else(|| WdlError::Decompile("watch missing handler".into()))?;
            let target = match handler {
                "end" => "done".to_string(),
                "fail" => "fail".to_string(),
                other => other.to_string(),
            };
            self.line(&format!("watch {} -> {target}", self.cond(condition)?));
        }
        self.out.push('\n');
        Ok(())
    }

    /// emit recovered `type <Name> ...` declarations from rendered surface strings. a struct (which
    /// renders starting with `{`) uses the brace shorthand; anything else uses the `= <type>` form.
    fn emit_type_decls(&mut self, decls: &[(String, String)]) -> Result<(), WdlError> {
        if decls.is_empty() {
            return Ok(());
        }
        for (index, (name, rendered)) in decls.iter().enumerate() {
            if index > 0 {
                self.out.push('\n');
            }
            if rendered.starts_with('{') {
                self.line(&format!("type {name} {rendered}"));
            } else {
                self.line(&format!("type {name} = {rendered}"));
            }
        }
        self.out.push('\n');
        Ok(())
    }

    /// emit the recovered header `alias <name> = { ... }` declarations, if any, followed by a
    /// blank line separating them from the body.
    fn emit_alias_decls(&mut self) -> Result<(), WdlError> {
        if self.alias_decls.is_empty() {
            return Ok(());
        }
        let decls = self.alias_decls.clone();
        for (name, segs) in &decls {
            let body = self.render_segs(segs)?;
            self.line(&format!("alias {name} = {{ {body} }}"));
        }
        self.out.push('\n');
        Ok(())
    }

    /// emit statements from `cur` until reaching `stop`, a terminal, or a dead end.
    fn emit_region(&mut self, cur: &str, stop: Option<&str>) -> Result<(), WdlError> {
        let mut cur = cur.to_string();
        // tracks whether the previous sibling in this block spanned multiple lines; `None` until the
        // first statement is emitted so the block never opens with a blank line.
        let mut prev_multiline: Option<bool> = None;
        loop {
            if stop == Some(cur.as_str()) {
                break;
            }
            if self.end_ids.contains(&cur) {
                break;
            }
            // reaching a node twice means an unstructured back-edge (e.g. a poll loop) or a
            // fan-in convergence that this structured walk cannot render. fail cleanly rather
            // than recursing without bound or emitting duplicate node ids.
            if !self.visited.insert(cur.clone()) {
                return Err(WdlError::Decompile(format!(
                    "workflow reaches node '{cur}' by more than one path (an unstructured loop or convergence) that cannot be decompiled to wdl; author this workflow in wdl directly"
                )));
            }
            let node = match self.nodes.get(cur.as_str()) {
                Some(node) => *node,
                None => break,
            };
            // capture each statement's rendered span so multi-line statements can be blank-separated
            // from their siblings, matching the formatter's block layout.
            let start = self.out.len();
            let (advance, stop_after) = match &node.kind {
                WorkflowNodeKind::Loop => (self.emit_loop(node, stop)?, false),
                WorkflowNodeKind::Condition => {
                    // a reentry-enabled single-branch condition node is a while/until loop
                    // header (its body loops back); anything else is a plain if/else.
                    let is_while = node.reentry.enabled && node.transitions.branches.len() == 1;
                    let merge = if is_while {
                        self.emit_while(node, stop)?
                    } else {
                        self.emit_if(node, stop)?
                    };
                    (merge, false)
                }
                WorkflowNodeKind::Switch => (self.emit_match(node, stop)?, false),
                WorkflowNodeKind::Fail => {
                    self.line("fail");
                    (None, true)
                }
                WorkflowNodeKind::Action
                | WorkflowNodeKind::Subflow
                | WorkflowNodeKind::Wait
                | WorkflowNodeKind::Output
                | WorkflowNodeKind::Deliverable
                | WorkflowNodeKind::Input
                | WorkflowNodeKind::Approval
                | WorkflowNodeKind::Gate
                | WorkflowNodeKind::Signal
                | WorkflowNodeKind::Config => {
                    let success = self.emit_leaf(node, stop)?;
                    // keep walking only into a fresh linear successor; a jump to a terminal, the
                    // region stop, or an already-emitted node was rendered as an explicit arrow by
                    // emit_leaf, so stop here.
                    let advance = match success {
                        Some(next)
                            if !self.is_terminal(&next)
                                && stop != Some(next.as_str())
                                && !self.visited.contains(&next) =>
                        {
                            Some(next)
                        }
                        _ => None,
                    };
                    let stop_after = advance.is_none();
                    (advance, stop_after)
                }
                WorkflowNodeKind::Map => (self.emit_map(node, stop)?, false),
                WorkflowNodeKind::Parallel => (self.emit_parallel(node, stop)?, false),
                WorkflowNodeKind::Race => (self.emit_race(node, stop)?, false),
                WorkflowNodeKind::Try => (self.emit_try(node, stop)?, false),
                // a join is consumed by its parallel; if reached directly, pass through without
                // emitting a statement.
                WorkflowNodeKind::Join => (
                    node.transitions
                        .next
                        .as_ref()
                        .map(|target| target.as_str().to_string()),
                    false,
                ),
                WorkflowNodeKind::Start | WorkflowNodeKind::End => (None, true),
            };

            self.separate_block_statement(start, &mut prev_multiline);

            match advance {
                Some(next) if !stop_after => cur = next,
                _ => break,
            }
        }
        Ok(())
    }

    // insert a blank line before the statement just rendered into `self.out[start..]` when it or the
    // previous sibling spans multiple lines. statements that emitted nothing (a join passthrough)
    // are ignored and leave `prev_multiline` untouched.
    fn separate_block_statement(&mut self, start: usize, prev_multiline: &mut Option<bool>) {
        if self.out.len() == start {
            return;
        }
        let cur_multiline = self.out[start..].trim_end_matches('\n').contains('\n');
        if matches!(prev_multiline, Some(prev) if *prev || cur_multiline) {
            self.out.insert(start, '\n');
        }
        *prev_multiline = Some(cur_multiline);
    }

    fn is_terminal(&self, id: &str) -> bool {
        self.end_ids.contains(id) || self.fail_ids.contains(id)
    }

    /// whether a node is a synthetic join, which has no standalone wdl statement form.
    fn is_join(&self, id: &str) -> bool {
        self.nodes
            .get(id)
            .is_some_and(|node| matches!(node.kind, WorkflowNodeKind::Join))
    }

    /// an `@id("...") ` prefix for a control block in the explicit form, empty otherwise. leaf
    /// nodes already surface their id through `let`/`@id`, so this covers only control headers.
    fn block_id_prefix(&self, node: &WorkflowNode) -> String {
        self.annotation_prefix(node, self.should_emit_control_id(node))
    }

    fn should_emit_control_id(&self, node: &WorkflowNode) -> bool {
        self.explicit || self.control_ids.contains(&node.id) || !is_generated_control_id(node)
    }

    fn annotation_prefix(&self, node: &WorkflowNode, include_id: bool) -> String {
        let mut parts = Vec::new();
        if include_id {
            parts.push(format!("@id({})", quote(&node.id)));
        }
        if node.skipped {
            parts.push("@skip".to_string());
        }
        if node.locked {
            parts.push("@lock".to_string());
        }
        if let Some(timeout) = node.timeout_seconds {
            parts.push(format!("@timeout({timeout}s)"));
        }
        if parts.is_empty() {
            String::new()
        } else {
            format!("{} ", parts.join(" "))
        }
    }

    fn target_label(&self, id: &str) -> String {
        if self.end_ids.contains(id) {
            "done".to_string()
        } else if self.fail_ids.contains(id) {
            "fail".to_string()
        } else {
            id.to_string()
        }
    }

    /// queue a node to be emitted as a top-level labelled statement, unless it is terminal,
    /// already emitted, or already queued.
    fn defer(&mut self, id: &str) {
        if self.is_terminal(id) || self.visited.contains(id) || !self.queued.insert(id.to_string())
        {
            return;
        }
        self.worklist.push_back(id.to_string());
    }

    /// print a control block's closing line (`}` or e.g. `} join all`), appending an explicit
    /// `-> label` when the block's exit is not the next inline statement (a terminal, the region
    /// stop, or an already-emitted node). returns `Some(next)` when the caller should keep
    /// walking inline into a fresh successor.
    fn close_block_line(
        &mut self,
        closing: &str,
        cont: Option<String>,
        stop: Option<&str>,
    ) -> Option<String> {
        // the explicit form always names the block's continuation edge with a `next ->` arrow,
        // still walking inline into a fresh successor so it is emitted once.
        if self.explicit {
            let Some(c) = cont else {
                self.line(closing);
                return None;
            };
            let label = self.target_label(&c);
            self.line(&format!("{closing} next -> {label}"));
            let fresh =
                !self.is_terminal(&c) && Some(c.as_str()) != stop && !self.visited.contains(&c);
            return fresh.then_some(c);
        }
        match cont {
            None => {
                self.line(closing);
                None
            }
            Some(c) if Some(c.as_str()) == stop => {
                self.line(closing);
                None
            }
            Some(c) if self.end_ids.contains(&c) => {
                self.line(closing);
                None
            }
            Some(c) if self.fail_ids.contains(&c) => {
                self.line(&format!("{closing} -> fail"));
                None
            }
            Some(c) if self.visited.contains(&c) => {
                self.line(&format!("{closing} -> {c}"));
                None
            }
            Some(c) => {
                self.line(closing);
                Some(c)
            }
        }
    }

    // leaf statements -------------------------------------------------------

    /// emit a single leaf statement with its outcome arrows. returns the success target.
    fn emit_leaf(
        &mut self,
        node: &WorkflowNode,
        stop: Option<&str>,
    ) -> Result<Option<String>, WdlError> {
        let (text, lets_binding) = self.statement_text(node)?;
        let prefix = if lets_binding {
            match self.declared_types.get(&node.id) {
                Some(rendered) => format!(
                    "{}node {}: {} <- ",
                    self.annotation_prefix(node, false),
                    node.id,
                    rendered
                ),
                None => format!(
                    "{}node {} <- ",
                    self.annotation_prefix(node, false),
                    node.id
                ),
            }
        } else if needs_id_annotation(&node.kind) {
            self.annotation_prefix(node, true)
        } else {
            self.annotation_prefix(node, false)
        };

        let transitions = &node.transitions;
        // the happy path lives in `on_success` (action/subflow/approval) or `next` (wait/output/
        // config); the populated field also names the explicit arrow keyword.
        let (succ_kw, success) = match (transitions.on_success.as_ref(), transitions.next.as_ref())
        {
            (Some(target), _) => ("ok", Some(target.as_str().to_string())),
            (None, Some(target)) => ("next", Some(target.as_str().to_string())),
            (None, None) => ("ok", None),
        };

        // collect failure-style arrows and queue their targets for top-level emission, since
        // the linear walk never descends into them.
        let mut arrows: Vec<(String, String)> = Vec::new();
        for (outcome, target) in [
            ("fail", &transitions.on_failure),
            ("timeout", &transitions.on_timeout),
            ("reject", &transitions.on_reject),
        ] {
            if let Some(target) = target {
                arrows.push((outcome.into(), self.target_label(target.as_str())));
                self.defer(target.as_str());
            }
        }

        // the success edge is explicit when it jumps to a terminal or to a node already emitted
        // elsewhere; otherwise it is the linear successor we keep walking into. the explicit form
        // renders it always, even when it is that linear successor or the region boundary.
        let success_arrow = match success.as_deref() {
            Some(id) if Some(id) == stop && !self.explicit => None,
            Some(id) if self.is_terminal(id) => Some(self.target_label(id)),
            Some(id) if self.visited.contains(id) => Some(id.to_string()),
            // a join has no wdl statement form, so its branch edges stay structural even in the
            // explicit form (the enclosing `parallel { branch }` already expresses them).
            Some(id) if self.explicit && !self.is_join(id) => Some(self.target_label(id)),
            _ => None,
        };

        // gather every rendered outgoing edge into one `edges { … }` section under the statement.
        // the pure linear successor stays implicit (success_arrow is None), so most nodes emit no
        // block; only explicit jumps and failure arrows surface a section.
        let mut edges: Vec<String> = Vec::new();
        if let Some(label) = &success_arrow {
            let kw = if self.explicit { succ_kw } else { "ok" };
            edges.push(format!("{kw} -> {label}"));
        }
        for (outcome, label) in &arrows {
            edges.push(format!("{outcome} -> {label}"));
        }
        // user-defined predicate edges, preserved in declaration order; an explicit `priority`
        // token is rendered whenever the branch carries one, keeping the round-trip stable.
        for branch in &transitions.branches {
            let cond = self.cond(&branch.when)?;
            let label = self.target_label(branch.target.as_str());
            self.defer(branch.target.as_str());
            match branch.priority {
                Some(priority) => edges.push(format!("when {cond} priority {priority} -> {label}")),
                None => edges.push(format!("when {cond} -> {label}")),
            }
        }

        self.line(&format!("{prefix}{text}"));
        if !edges.is_empty() {
            self.line("edges {");
            self.indent += 1;
            for edge in &edges {
                self.line(edge);
            }
            self.indent -= 1;
            self.line("}");
        }

        Ok(success)
    }

    /// returns the statement text and whether it should be prefixed with `node <id> <-`.
    fn statement_text(&self, node: &WorkflowNode) -> Result<(String, bool), WdlError> {
        match &node.kind {
            WorkflowNodeKind::Action => {
                // a std provider node carrying a `program` is a compute block, not a plain call.
                if let Some(program) = compute_program(node) {
                    return Ok((self.compute_text(node, program)?, true));
                }
                if foreign_compute_config(node).is_some() {
                    return Ok((self.foreign_compute_text(node)?, true));
                }
                Ok((self.action_text(node)?, true))
            }
            WorkflowNodeKind::Subflow => Ok((self.subflow_text(node)?, true)),
            WorkflowNodeKind::Wait => Ok((self.wait_text(node)?, false)),
            WorkflowNodeKind::Output => Ok((self.output_text(node)?, false)),
            WorkflowNodeKind::Deliverable => Ok((self.deliverable_text(node)?, false)),
            WorkflowNodeKind::Input => Ok((self.input_text(node)?, false)),
            WorkflowNodeKind::Approval => Ok((self.approval_text(node)?, false)),
            WorkflowNodeKind::Gate => Ok((self.gate_text(node)?, false)),
            WorkflowNodeKind::Signal => Ok((self.signal_text(node)?, false)),
            WorkflowNodeKind::Config => Ok((self.config_text(node)?, false)),
            other => Err(WdlError::Decompile(format!("unexpected leaf {other:?}"))),
        }
    }

    // render a compute block. inner lines carry their absolute indentation so the caller's
    // `self.line` (which only indents the first line) yields correctly nested output, and the
    // trailing success arrow appends cleanly after the closing brace.
    fn compute_text(&self, node: &WorkflowNode, program: &[Value]) -> Result<String, WdlError> {
        let base = self.indent;
        let mut out = String::from("compute {\n");
        self.render_compute_lines(&mut out, program, base + 1)?;
        out.push_str(&"    ".repeat(base));
        out.push('}');
        if let Some(action) = &node.action {
            let mut modifiers = Vec::new();
            if self.explicit || action.timeout_seconds != 60 {
                modifiers.push(format!(".timeout({}s)", action.timeout_seconds));
            }
            if let Some(retry) = decompile_retry(&node.retry, self.explicit) {
                modifiers.push(retry);
            }
            // a compute block always closes its brace on its own line, so the chain hugs it.
            out.push_str(&self.modifier_suffix(base, &modifiers, true));
        }
        Ok(out)
    }

    fn render_compute_lines(
        &self,
        out: &mut String,
        program: &[Value],
        indent: usize,
    ) -> Result<(), WdlError> {
        let pad = "    ".repeat(indent);
        for statement in program {
            let object = statement
                .as_object()
                .ok_or_else(|| WdlError::Decompile("compute statement must be an object".into()))?;
            if let Some(name) = object.get("$let").and_then(Value::as_str) {
                let value = object
                    .get("value")
                    .ok_or_else(|| WdlError::Decompile("compute let missing value".into()))?;
                out.push_str(&format!("{pad}let {name} = {}\n", self.expr(value)?));
            } else if let Some(value) = object.get("$return") {
                out.push_str(&format!("{pad}return {}\n", self.expr(value)?));
            } else if let Some(target) = object.get("$goto").and_then(Value::as_str) {
                out.push_str(&format!("{pad}goto {}\n", self.target_label(target)));
            } else if let Some(condition) = object.get("$if") {
                out.push_str(&format!("{pad}if {} {{\n", self.cond(condition)?));
                let then_branch = object
                    .get("then")
                    .and_then(Value::as_array)
                    .cloned()
                    .unwrap_or_default();
                self.render_compute_lines(out, &then_branch, indent + 1)?;
                let else_branch = object
                    .get("else")
                    .and_then(Value::as_array)
                    .cloned()
                    .unwrap_or_default();
                if else_branch.is_empty() {
                    out.push_str(&format!("{pad}}}\n"));
                } else {
                    out.push_str(&format!("{pad}}} else {{\n"));
                    self.render_compute_lines(out, &else_branch, indent + 1)?;
                    out.push_str(&format!("{pad}}}\n"));
                }
            } else {
                // a bare expression statement (e.g. a side-effecting call).
                out.push_str(&format!("{pad}{}\n", self.expr(statement)?));
            }
        }
        Ok(())
    }

    fn foreign_compute_text(&self, node: &WorkflowNode) -> Result<String, WdlError> {
        let action = node
            .action
            .as_ref()
            .ok_or_else(|| WdlError::Decompile("foreign compute node missing action".into()))?;
        let config = action.configuration.as_value();
        let language = config
            .get("language")
            .and_then(Value::as_str)
            .ok_or_else(|| WdlError::Decompile("foreign compute missing language".into()))?;
        let source = config
            .get("source")
            .and_then(Value::as_str)
            .ok_or_else(|| WdlError::Decompile("foreign compute missing source".into()))?;
        if source.contains("```") {
            return Err(WdlError::Decompile(
                "foreign compute source contains a code fence delimiter".into(),
            ));
        }

        let base = self.indent;
        let mut out = format!("compute {language}");
        if let Some(image) = config.get("image").and_then(Value::as_str) {
            out.push_str(&format!(" using {}", quote(image)));
        }
        out.push_str(" ```\n");
        out.push_str(source);
        if !source.ends_with('\n') {
            out.push('\n');
        }
        out.push_str("```");

        let mut modifiers = Vec::new();
        if self.explicit || action.timeout_seconds != 60 {
            modifiers.push(format!(".timeout({}s)", action.timeout_seconds));
        }
        if let Some(retry) = decompile_retry(&node.retry, self.explicit) {
            modifiers.push(retry);
        }
        out.push_str(&self.modifier_suffix(base, &modifiers, true));
        Ok(out)
    }

    // lay out call arguments as a parenthesized list with one argument per line, indented under
    // `base`. an empty list renders inline as `()`.
    fn call_args(&self, parts: &[String], base: usize) -> String {
        if parts.is_empty() {
            return "()".to_string();
        }
        let inner = "    ".repeat(base + 1);
        let mut out = String::from("(\n");
        for (index, part) in parts.iter().enumerate() {
            out.push_str(&inner);
            out.push_str(part);
            if index + 1 < parts.len() {
                out.push(',');
            }
            out.push('\n');
        }
        out.push_str(&"    ".repeat(base));
        out.push(')');
        out
    }

    // lay out pre-rendered `key: value` / `...alias` parts as a brace object, one per line. used
    // for the trailing metadata objects (spreads and subflow/approval/gate/signal params).
    fn parts_object(&self, parts: &[String], base: usize) -> String {
        if parts.is_empty() {
            return "{}".to_string();
        }
        let inner = "    ".repeat(base + 1);
        let mut out = String::from("{\n");
        for (index, part) in parts.iter().enumerate() {
            out.push_str(&inner);
            out.push_str(part);
            if index + 1 < parts.len() {
                out.push(',');
            }
            out.push('\n');
        }
        out.push_str(&"    ".repeat(base));
        out.push('}');
        out
    }

    // append the fluent modifier chain (`.timeout(…)`, `.retry(…)`, …). the first call hugs the
    // closing paren/brace; any further calls align their leading dot one column past it. an inline
    // call (no multi-line args) keeps the whole chain on one line.
    fn modifier_suffix(&self, base: usize, modifiers: &[String], multiline: bool) -> String {
        let Some((first, rest)) = modifiers.split_first() else {
            return String::new();
        };
        let mut out = String::from(first.as_str());
        if !multiline {
            for modifier in rest {
                out.push_str(modifier);
            }
            return out;
        }
        let pad = format!("{} ", "    ".repeat(base));
        for modifier in rest {
            out.push('\n');
            out.push_str(&pad);
            out.push_str(modifier);
        }
        out
    }

    fn action_text(&self, node: &WorkflowNode) -> Result<String, WdlError> {
        let action = node
            .action
            .as_ref()
            .ok_or_else(|| WdlError::Decompile("action node missing action".into()))?;
        // action nodes carry args in `configuration`, but the reducer merges node-level
        // `parameters` over it (parameters win). fold both into the call args so a node that
        // only populated `parameters` is not dropped; recompiling routes them to configuration,
        // which is equivalent under the same merge.
        // a recorded spread recipe re-emits the authored `...alias` argument list; otherwise the
        // arguments come straight from the flat configuration/parameters.
        let base = self.indent;
        let arg_parts = if let Some(segs) = self.spreads.get(&node.id) {
            self.render_seg_parts(segs)?
        } else {
            let mut merged = Map::new();
            if let Value::Object(config) = action.configuration.as_value() {
                for (name, value) in config {
                    merged.insert(name.clone(), value.clone());
                }
            }
            if let Value::Object(params) = node.parameters.as_value() {
                for (name, value) in params {
                    merged.insert(name.clone(), value.clone());
                }
            }
            let mut args = Vec::new();
            for (name, value) in &merged {
                args.push(format!("{name}: {}", self.expr_multiline(value, base + 1)?));
            }
            args
        };
        let multiline = !arg_parts.is_empty();
        let mut text = format!(
            "{}.{}{}",
            action.provider,
            action.function,
            self.call_args(&arg_parts, base)
        );
        let mut modifiers = Vec::new();
        if self.explicit || action.timeout_seconds != 60 {
            modifiers.push(format!(".timeout({}s)", action.timeout_seconds));
        }
        if let Some(retry) = decompile_retry(&node.retry, self.explicit) {
            modifiers.push(retry);
        }
        if !action.tags.is_empty() {
            let tags = action
                .tags
                .iter()
                .map(|tag| quote(tag))
                .collect::<Vec<_>>()
                .join(", ");
            modifiers.push(format!(".tags({tags})"));
        }
        if action.mcp_enabled {
            modifiers.push(".mcp()".to_string());
        }
        if node.reentry.enabled {
            modifiers.push(format!(".reentry({})", node.reentry.max_visits));
        }
        text.push_str(&self.modifier_suffix(base, &modifiers, multiline));
        if let Some(compensation) = &node.compensation {
            text.push_str(&format!(
                " compensate {}",
                self.action_call_text(compensation, base)?
            ));
        }
        Ok(text)
    }

    /// render a bare `provider.function(args)` call from a `WorkflowAction` (used for `compensate`).
    fn action_call_text(
        &self,
        action: &runinator_models::workflows::WorkflowAction,
        base: usize,
    ) -> Result<String, WdlError> {
        let mut args = Vec::new();
        if let Value::Object(config) = action.configuration.as_value() {
            for (name, value) in config {
                args.push(format!("{name}: {}", self.expr_multiline(value, base + 1)?));
            }
        }
        Ok(format!(
            "{}.{}{}",
            action.provider,
            action.function,
            self.call_args(&args, base)
        ))
    }

    fn subflow_text(&self, node: &WorkflowNode) -> Result<String, WdlError> {
        let subflow = &node.subflow;
        let name = subflow.workflow_name.clone().unwrap_or_default();
        let mut args = vec![quote(&name)];
        let base = self.indent;
        if subflow.reuse_open_run {
            args.push("reuse: true".to_string());
        }
        if matches!(
            subflow.subflow_type,
            runinator_models::workflows::WorkflowSubflowType::FireAndForget
        ) {
            args.push("detached: true".to_string());
        }
        if let Some(run_name) = &subflow.run_name {
            args.push(format!("name: {}", self.expr(run_name)?));
        }
        let mut params_arg = None;
        if let Some(segs) = self.spreads.get(&node.id) {
            let parts = self.render_seg_parts(segs)?;
            params_arg = Some(format!("params: {}", self.parts_object(&parts, base)));
        } else if let Value::Object(params) = node.parameters.as_value() {
            if !params.is_empty() {
                let mut parts = Vec::new();
                for (name, value) in params {
                    parts.push(format!("{name}: {}", self.expr_multiline(value, base + 1)?));
                }
                params_arg = Some(format!("params: {}", self.parts_object(&parts, base)));
            }
        }
        if let Some(params_arg) = params_arg {
            args.insert(1, params_arg);
        }
        Ok(format!("subflow({})", args.join(", ")))
    }

    fn wait_text(&self, node: &WorkflowNode) -> Result<String, WdlError> {
        let amount = match &node.wait.seconds {
            Some(WorkflowWaitSeconds::Integer(seconds)) => format!("{seconds}s"),
            Some(WorkflowWaitSeconds::Expression(expr)) => self.expr(expr.as_value())?,
            None => "0s".to_string(),
        };
        let mut text = format!("wait {amount}");
        if let Some(status) = &node.wait.until_status {
            text.push_str(&format!(" until {}", quote(status)));
        }
        if let Some(status) = &node.wait.initial_status {
            text.push_str(&format!(" initial {}", quote(status)));
        }
        Ok(text)
    }

    fn output_text(&self, node: &WorkflowNode) -> Result<String, WdlError> {
        let mut text = "emit".to_string();
        let event = node.parameters.get("event_type").and_then(Value::as_str);
        if let Some(event_type) = event {
            text.push_str(&format!(" {}", quote(event_type)));
        }
        match node.parameters.get("data") {
            None | Some(Value::Null) => text.push_str(" {}"),
            Some(data @ Value::Object(_)) => {
                text.push_str(&format!(" {}", self.expr_multiline(data, self.indent)?))
            }
            Some(other) => {
                // scalar/array payloads render as expressions. without a preceding event type a
                // bare string or concat would be parsed as the event, so wrap it in parens.
                let rendered = self.expr(other)?;
                if event.is_some() {
                    text.push_str(&format!(" {rendered}"));
                } else {
                    text.push_str(&format!(" ({rendered})"));
                }
            }
        }
        Ok(text)
    }

    fn deliverable_text(&self, node: &WorkflowNode) -> Result<String, WdlError> {
        let base = self.indent;
        let mut out = String::from("deliverable {\n");
        if let Some(items) = node.parameters.get("items").and_then(Value::as_array) {
            for item in items {
                let name = item.get("name").and_then(Value::as_str).unwrap_or_default();
                let source = item.get("source").cloned().unwrap_or(Value::Null);
                out.push_str(&"    ".repeat(base + 1));
                out.push_str(&format!("{name} = {}\n", self.expr(&source)?));
            }
        }
        out.push_str(&"    ".repeat(base));
        out.push('}');
        Ok(out)
    }

    fn input_text(&self, node: &WorkflowNode) -> Result<String, WdlError> {
        let mut text = "input".to_string();
        if let Some(prompt) = node.parameters.get("prompt")
            && !matches!(prompt, Value::Null)
        {
            text.push(' ');
            text.push_str(&self.expr(prompt)?);
        }
        Ok(text)
    }

    fn approval_text(&self, node: &WorkflowNode) -> Result<String, WdlError> {
        let prompt = node
            .parameters
            .get("prompt")
            .cloned()
            .unwrap_or(Value::String("Approval required".into()));
        let mut text = format!("approve {}", self.expr(&prompt)?);
        let kind = node
            .parameters
            .get("approval_type")
            .and_then(Value::as_str)
            .unwrap_or("generic");
        if self.explicit || kind != "generic" {
            text.push_str(&format!(" type {}", quote(kind)));
        }
        let base = self.indent;
        if let Some(segs) = self.spreads.get(&node.id) {
            let parts = self.render_seg_parts(segs)?;
            text.push_str(&format!(" {}", self.parts_object(&parts, base)));
        } else if let Value::Object(params) = node.parameters.as_value() {
            let entries: Vec<(&str, &Value)> = params
                .iter()
                .filter(|(name, _)| name.as_str() != "prompt" && name.as_str() != "approval_type")
                .map(|(name, value)| (name.as_str(), value))
                .collect();
            if !entries.is_empty() {
                text.push_str(&format!(" {}", self.entries_object(&entries, base)?));
            }
        }
        Ok(text)
    }

    fn gate_text(&self, node: &WorkflowNode) -> Result<String, WdlError> {
        let kind = node
            .parameters
            .get("kind")
            .and_then(Value::as_str)
            .unwrap_or("manual");
        let mut text = format!("gate {kind}");
        if let Some(when) = node.parameters.get("when") {
            text.push_str(&format!(" when {}", self.cond(when)?));
        }
        if let Some(poll) = node.parameters.get("poll_interval").and_then(Value::as_i64) {
            text.push_str(&format!(" every {poll}s"));
        }
        if let Some(timeout) = node.parameters.get("timeout").and_then(Value::as_i64) {
            text.push_str(&format!(" timeout {timeout}s"));
        }
        // remaining params (label + extras) render as the trailing metadata object.
        let base = self.indent;
        if let Some(segs) = self.spreads.get(&node.id) {
            let parts = self.render_seg_parts(segs)?;
            text.push_str(&format!(" {}", self.parts_object(&parts, base)));
        } else if let Value::Object(params) = node.parameters.as_value() {
            let entries: Vec<(&str, &Value)> = params
                .iter()
                .filter(|(name, _)| {
                    !matches!(name.as_str(), "kind" | "when" | "poll_interval" | "timeout")
                })
                .map(|(name, value)| (name.as_str(), value))
                .collect();
            if !entries.is_empty() {
                text.push_str(&format!(" {}", self.entries_object(&entries, base)?));
            }
        }
        Ok(text)
    }

    fn signal_text(&self, node: &WorkflowNode) -> Result<String, WdlError> {
        let name = node
            .parameters
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let mut text = format!("signal {}", quote(name));
        // the optional correlation key renders as `key <expr>` before any metadata object.
        if let Some(key) = node.parameters.get("correlation_key") {
            text.push_str(&format!(" key {}", self.expr(key)?));
        }
        // remaining params render as the trailing metadata object.
        let base = self.indent;
        if let Some(segs) = self.spreads.get(&node.id) {
            let parts = self.render_seg_parts(segs)?;
            text.push_str(&format!(" {}", self.parts_object(&parts, base)));
        } else if let Value::Object(params) = node.parameters.as_value() {
            let entries: Vec<(&str, &Value)> = params
                .iter()
                .filter(|(key, _)| key.as_str() != "name" && key.as_str() != "correlation_key")
                .map(|(key, value)| (key.as_str(), value))
                .collect();
            if !entries.is_empty() {
                text.push_str(&format!(" {}", self.entries_object(&entries, base)?));
            }
        }
        Ok(text)
    }

    fn config_text(&self, node: &WorkflowNode) -> Result<String, WdlError> {
        if let Some(name) = node.parameters.get("name") {
            return Ok(format!("set name = {}", self.expr(name)?));
        }
        if let Some(metadata) = node.parameters.get("metadata") {
            return Ok(format!(
                "set meta {}",
                self.expr_multiline(metadata, self.indent)?
            ));
        }
        Ok("set meta {}".to_string())
    }

    // control blocks --------------------------------------------------------

    fn emit_loop(
        &mut self,
        node: &WorkflowNode,
        stop: Option<&str>,
    ) -> Result<Option<String>, WdlError> {
        let body_entry = node
            .transitions
            .next
            .as_ref()
            .map(|target| target.as_str().to_string());
        let after = node
            .transitions
            .on_success
            .as_ref()
            .map(|target| target.as_str().to_string());

        let items = node.parameters.get("items").cloned().unwrap_or(Value::Null);
        let items_text = self.expr(&items)?;
        let var = self.fresh_var();

        let mut header = format!("{}for {var} in {items_text}", self.block_id_prefix(node));
        match node.max_iterations {
            Some(limit) => header.push_str(&format!(" limit {limit}")),
            None => match node.parameters.get("max_iterations") {
                // an expression cap is carried in the loop parameters.
                Some(limit) => {
                    let limit_text = self.expr(limit)?;
                    header.push_str(&format!(" limit {limit_text}"));
                }
                None if self.explicit => header.push_str(" limit none"),
                None => {}
            },
        }
        header.push_str(" {");
        self.line(&header);

        self.indent += 1;
        self.loop_vars.push((node.id.clone(), var));
        if let Some(body_entry) = body_entry {
            self.emit_region(&body_entry, Some(node.id.as_str()))?;
        }
        self.loop_vars.pop();
        self.indent -= 1;

        Ok(self.close_block_line("}", after, stop))
    }

    fn emit_while(
        &mut self,
        node: &WorkflowNode,
        stop: Option<&str>,
    ) -> Result<Option<String>, WdlError> {
        let branch = node
            .transitions
            .branches
            .first()
            .ok_or_else(|| WdlError::Decompile("while node has no branch".into()))?;
        let body_entry = branch.target.as_str().to_string();
        let after = node
            .transitions
            .next
            .as_ref()
            .map(|target| target.as_str().to_string());

        let mut header = format!(
            "{}while {}",
            self.block_id_prefix(node),
            self.cond(&branch.when)?
        );
        if node.reentry.max_visits > 0 {
            header.push_str(&format!(" limit {}", node.reentry.max_visits));
        }
        header.push_str(" {");
        self.line(&header);

        self.indent += 1;
        // the body loops back to this header, so stop the region walk there.
        self.emit_region(&body_entry, Some(node.id.as_str()))?;
        self.indent -= 1;

        Ok(self.close_block_line("}", after, stop))
    }

    fn emit_if(
        &mut self,
        node: &WorkflowNode,
        stop: Option<&str>,
    ) -> Result<Option<String>, WdlError> {
        let branches = &node.transitions.branches;
        if branches.is_empty() {
            return Err(WdlError::Decompile("condition node has no branches".into()));
        }
        let else_target = node
            .transitions
            .next
            .as_ref()
            .map(|target| target.as_str().to_string());

        let mut merge_inputs: Vec<String> = branches
            .iter()
            .map(|b| b.target.as_str().to_string())
            .collect();
        if let Some(else_target) = &else_target {
            merge_inputs.push(else_target.clone());
        }
        let merge = self
            .find_merge(&merge_inputs)
            .or_else(|| stop.map(str::to_string));
        let merge_ref = merge.as_deref();

        for (index, branch) in branches.iter().enumerate() {
            let keyword = if index == 0 {
                format!("{}if", self.block_id_prefix(node))
            } else {
                "} else if".to_string()
            };
            self.line(&format!("{keyword} {} {{", self.cond(&branch.when)?));
            self.indent += 1;
            self.emit_region(branch.target.as_str(), merge_ref)?;
            self.indent -= 1;
        }

        if let Some(else_target) = &else_target {
            if merge_ref != Some(else_target.as_str())
                && !self.end_ids.contains(else_target)
                && !self.visited.contains(else_target)
            {
                self.line("} else {");
                self.indent += 1;
                self.emit_region(else_target, merge_ref)?;
                self.indent -= 1;
            }
        }

        Ok(self.close_block_line("}", merge, stop))
    }

    fn emit_match(
        &mut self,
        node: &WorkflowNode,
        stop: Option<&str>,
    ) -> Result<Option<String>, WdlError> {
        let value = node.parameters.get("value").cloned().unwrap_or(Value::Null);
        let cases = node
            .parameters
            .get("cases")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        let default = node
            .parameters
            .get("default")
            .and_then(|v| v.get("$node"))
            .and_then(Value::as_str)
            .map(str::to_string);

        let mut merge_inputs: Vec<String> = cases
            .iter()
            .filter_map(|case| case.pointer("/target/$node").and_then(Value::as_str))
            .map(str::to_string)
            .collect();
        if let Some(default) = &default {
            merge_inputs.push(default.clone());
        }
        let merge = self
            .find_merge(&merge_inputs)
            .or_else(|| stop.map(str::to_string));
        let merge_ref = merge.as_deref();

        self.line(&format!(
            "{}match {} {{",
            self.block_id_prefix(node),
            self.expr(&value)?
        ));
        self.indent += 1;
        for case in &cases {
            let target = case
                .pointer("/target/$node")
                .and_then(Value::as_str)
                .ok_or_else(|| WdlError::Decompile("switch case missing target".into()))?;
            let head = if let Some(when) = case.get("when") {
                format!("when {}", self.cond(when)?)
            } else if let Some(equals) = case.get("equals") {
                self.expr(equals)?
            } else {
                // not_equals / exists shorthand: rebuild the implied condition against the
                // switch subject (mirroring parse_switch_parameters) and render it as a guard.
                let mut condition = Map::new();
                condition.insert("value".into(), value.clone());
                for key in ["not_equals", "exists"] {
                    if let Some(expected) = case.get(key) {
                        condition.insert(key.into(), expected.clone());
                    }
                }
                if condition.len() == 1 {
                    return Err(WdlError::Decompile(
                        "switch case missing when/equals/not_equals/exists".into(),
                    ));
                }
                format!("when {}", self.cond(&Value::Object(condition))?)
            };
            self.line(&format!("{head} -> {{"));
            self.indent += 1;
            self.emit_region(target, merge_ref)?;
            self.indent -= 1;
            self.line("}");
        }
        if let Some(default) = &default {
            if merge_ref != Some(default.as_str()) && !self.visited.contains(default) {
                self.line("else -> {");
                self.indent += 1;
                self.emit_region(default, merge_ref)?;
                self.indent -= 1;
                self.line("}");
            }
        }
        self.indent -= 1;

        Ok(self.close_block_line("}", merge, stop))
    }

    fn emit_map(
        &mut self,
        node: &WorkflowNode,
        stop: Option<&str>,
    ) -> Result<Option<String>, WdlError> {
        let body_entry = single_node_id(node.parameters.get("target"));
        let after = node
            .transitions
            .next
            .as_ref()
            .map(|target| target.as_str().to_string());

        let items = node.parameters.get("items").cloned().unwrap_or(Value::Null);
        let items_text = self.expr(&items)?;
        let var = self.fresh_var();

        let mut header = format!("{}map {var} in {items_text}", self.block_id_prefix(node));
        match node.parameters.get("concurrency").and_then(Value::as_i64) {
            Some(concurrency) => header.push_str(&format!(" concurrency {concurrency}")),
            None if self.explicit => header.push_str(" concurrency none"),
            None => {}
        }
        header.push_str(" {");
        self.line(&header);

        self.indent += 1;
        self.loop_vars.push((node.id.clone(), var));
        if let Some(body_entry) = body_entry {
            self.emit_region(&body_entry, Some(node.id.as_str()))?;
        }
        self.loop_vars.pop();
        self.indent -= 1;

        Ok(self.close_block_line("}", after, stop))
    }

    fn emit_parallel(
        &mut self,
        node: &WorkflowNode,
        stop: Option<&str>,
    ) -> Result<Option<String>, WdlError> {
        let branches = node_ref_ids(node.parameters.get("branches"));
        let join = self.find_join(&branches).ok_or_else(|| {
            WdlError::Decompile(format!("parallel '{}' has no matching join", node.id))
        })?;
        let (join_id, mode, cont) = join;

        self.line(&format!("{}parallel {{", self.block_id_prefix(node)));
        self.indent += 1;
        for branch in &branches {
            self.line("branch {");
            self.indent += 1;
            self.emit_region(branch, Some(join_id.as_str()))?;
            self.indent -= 1;
            self.line("}");
        }
        self.indent -= 1;

        Ok(self.close_block_line(&format!("}} join {mode}"), cont, stop))
    }

    fn emit_race(
        &mut self,
        node: &WorkflowNode,
        outer_stop: Option<&str>,
    ) -> Result<Option<String>, WdlError> {
        let branches = node_ref_ids(node.parameters.get("branches"));
        let winner = node
            .parameters
            .get("winner")
            .and_then(Value::as_str)
            .unwrap_or("first_success")
            .to_string();
        let cont = node
            .transitions
            .next
            .as_ref()
            .map(|target| target.as_str().to_string());
        let branch_stop = cont.clone();

        self.line(&format!(
            "{}race winner {winner} {{",
            self.block_id_prefix(node)
        ));
        self.indent += 1;
        for branch in &branches {
            self.line("branch {");
            self.indent += 1;
            self.emit_region(branch, branch_stop.as_deref())?;
            self.indent -= 1;
            self.line("}");
        }
        self.indent -= 1;

        Ok(self.close_block_line("}", cont, outer_stop))
    }

    fn emit_try(
        &mut self,
        node: &WorkflowNode,
        outer_stop: Option<&str>,
    ) -> Result<Option<String>, WdlError> {
        let body = single_node_id(node.parameters.get("body"));
        let catch = single_node_id(node.parameters.get("catch"));
        let finally = single_node_id(node.parameters.get("finally"));
        let cont = node
            .transitions
            .next
            .as_ref()
            .map(|target| target.as_str().to_string());
        let branch_stop = cont.clone();

        self.line(&format!("{}try {{", self.block_id_prefix(node)));
        self.indent += 1;
        if let Some(body) = &body {
            self.emit_region(body, branch_stop.as_deref())?;
        }
        self.indent -= 1;
        if let Some(catch) = &catch {
            self.line("} catch {");
            self.indent += 1;
            self.emit_region(catch, branch_stop.as_deref())?;
            self.indent -= 1;
        }
        if let Some(finally) = &finally {
            self.line("} finally {");
            self.indent += 1;
            self.emit_region(finally, branch_stop.as_deref())?;
            self.indent -= 1;
        }

        Ok(self.close_block_line("}", cont, outer_stop))
    }

    /// find the join node that synchronizes the given parallel branches.
    fn find_join(&self, branches: &[String]) -> Option<(String, String, Option<String>)> {
        let target: HashSet<&str> = branches.iter().map(String::as_str).collect();
        for node in self.nodes.values() {
            if !matches!(node.kind, WorkflowNodeKind::Join) {
                continue;
            }
            let wait_for = node_ref_ids(node.parameters.get("wait_for"));
            let actual: HashSet<&str> = wait_for.iter().map(String::as_str).collect();
            if actual == target {
                let mode = node
                    .parameters
                    .get("mode")
                    .and_then(Value::as_str)
                    .unwrap_or("all")
                    .to_string();
                let cont = node
                    .transitions
                    .next
                    .as_ref()
                    .map(|target| target.as_str().to_string());
                return Some((node.id.clone(), mode, cont));
            }
        }
        None
    }

    // helpers ---------------------------------------------------------------

    fn fresh_var(&self) -> String {
        let active: HashSet<&String> = self.loop_vars.iter().map(|(_, var)| var).collect();
        if !active.contains(&"item".to_string()) {
            return "item".to_string();
        }
        for index in 2.. {
            let candidate = format!("item{index}");
            if !active.contains(&candidate) {
                return candidate;
            }
        }
        unreachable!()
    }

    /// find the nearest node reachable from every input (the structured merge point).
    fn find_merge(&self, starts: &[String]) -> Option<String> {
        if starts.is_empty() {
            return None;
        }
        let distance_maps: Vec<HashMap<String, usize>> =
            starts.iter().map(|start| self.reachable(start)).collect();
        let mut best: Option<String> = None;
        let mut best_score = usize::MAX;
        for (node, _) in &distance_maps[0] {
            if distance_maps.iter().all(|map| map.contains_key(node)) {
                let score: usize = distance_maps.iter().map(|map| map[node]).sum();
                if score < best_score {
                    best_score = score;
                    best = Some(node.clone());
                }
            }
        }
        best
    }

    fn reachable(&self, start: &str) -> HashMap<String, usize> {
        let mut distances = HashMap::new();
        let mut queue = VecDeque::new();
        distances.insert(start.to_string(), 0usize);
        queue.push_back(start.to_string());
        while let Some(current) = queue.pop_front() {
            let depth = distances[&current];
            let Some(node) = self.nodes.get(current.as_str()) else {
                continue;
            };
            for target in self.out_edges(node) {
                if !distances.contains_key(&target) {
                    distances.insert(target.clone(), depth + 1);
                    queue.push_back(target);
                }
            }
        }
        distances
    }

    fn out_edges(&self, node: &WorkflowNode) -> Vec<String> {
        let mut edges = transition_targets(&node.transitions);
        // include parameter-driven targets so switch arms participate in merge detection.
        collect_node_refs(&node.parameters, &mut edges);
        edges
    }
}

fn transition_targets(transitions: &WorkflowTransitions) -> Vec<String> {
    let mut targets = Vec::new();
    for target in [
        &transitions.next,
        &transitions.on_success,
        &transitions.on_failure,
        &transitions.on_timeout,
        &transitions.on_reject,
    ]
    .into_iter()
    .flatten()
    {
        targets.push(target.as_str().to_string());
    }
    for branch in &transitions.branches {
        targets.push(branch.target.as_str().to_string());
    }
    targets
}

fn collect_node_refs(value: &Value, out: &mut Vec<String>) {
    match value {
        Value::Object(map) => {
            if map.len() == 1 {
                if let Some(id) = map.get("$node").and_then(Value::as_str) {
                    out.push(id.to_string());
                    return;
                }
            }
            for nested in map.values() {
                collect_node_refs(nested, out);
            }
        }
        Value::Array(items) => {
            for item in items {
                collect_node_refs(item, out);
            }
        }
        _ => {}
    }
}

/// read an array of `{ "$node": id }` references into a list of node ids.
fn node_ref_ids(value: Option<&Value>) -> Vec<String> {
    value
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|item| item.pointer("/$node").and_then(Value::as_str))
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

/// the compute program of a `std` provider action node, if present.
fn compute_program(node: &WorkflowNode) -> Option<&[Value]> {
    let action = node.action.as_ref()?;
    if action.provider != "std" {
        return None;
    }
    action
        .configuration
        .as_value()
        .get("program")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
}

/// the foreign compute config of a `std.code` action node, if present.
fn foreign_compute_config(node: &WorkflowNode) -> Option<&Value> {
    let action = node.action.as_ref()?;
    if action.provider != "std" || action.function != "code" {
        return None;
    }
    let config = action.configuration.as_value();
    if config.get("language").is_some() && config.get("source").is_some() {
        return Some(config);
    }
    None
}

/// read a single `{ "$node": id }` reference into a node id.
fn single_node_id(value: Option<&Value>) -> Option<String> {
    value
        .and_then(|value| value.pointer("/$node"))
        .and_then(Value::as_str)
        .map(str::to_string)
}

fn needs_id_annotation(kind: &WorkflowNodeKind) -> bool {
    matches!(
        kind,
        WorkflowNodeKind::Wait
            | WorkflowNodeKind::Output
            | WorkflowNodeKind::Deliverable
            | WorkflowNodeKind::Input
            | WorkflowNodeKind::Approval
            | WorkflowNodeKind::Gate
            | WorkflowNodeKind::Signal
            | WorkflowNodeKind::Config
    )
}

fn is_generated_control_id(node: &WorkflowNode) -> bool {
    let prefixes: &[&str] = match node.kind {
        WorkflowNodeKind::Condition if node.reentry.enabled => &["while_loop"],
        WorkflowNodeKind::Condition => &["if"],
        WorkflowNodeKind::Loop => &["for_loop"],
        WorkflowNodeKind::Map => &["map"],
        WorkflowNodeKind::Parallel => &["parallel"],
        WorkflowNodeKind::Race => &["race"],
        WorkflowNodeKind::Switch => &["switch"],
        WorkflowNodeKind::Try => &["try"],
        _ => return true,
    };
    prefixes
        .iter()
        .any(|prefix| has_numbered_id(&node.id, prefix))
}

fn has_numbered_id(id: &str, prefix: &str) -> bool {
    let Some(rest) = id
        .strip_prefix(prefix)
        .and_then(|rest| rest.strip_prefix('_'))
    else {
        return false;
    };
    !rest.is_empty() && rest.chars().all(|ch| ch.is_ascii_digit())
}

fn quote(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 2);
    out.push('"');
    for ch in text.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '$' => out.push_str("\\$"),
            '\n' => out.push_str("\\n"),
            other => out.push(other),
        }
    }
    out.push('"');
    out
}
