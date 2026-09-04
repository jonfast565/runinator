// reconstructs rexrap source from a WorkflowDefinition. it walks the graph from the start node,
// recovering structured blocks (for/while/if/match/parallel/race/try) where possible. each
// node is emitted exactly once; every other edge into it (fail/reject/timeout arrows, back
// edges, and fan-in convergence) is rendered as an explicit `-> label` arrow, and nodes
// reached only by such arrows are emitted as top-level labelled statements. this lets
// arbitrary graphs round-trip, since rexrap labels are global.

mod expr;
mod metadata;

use std::collections::{HashMap, HashSet, VecDeque};

use runinator_models::types::RuninatorType;
use runinator_models::value::{Map, Value};
use runinator_models::workflows::{
    WorkflowDefinition, WorkflowGraph, WorkflowNode, WorkflowNodeKind, WorkflowRetry,
    WorkflowRetryClass, WorkflowTransitions, WorkflowWaitSeconds,
};

use runinator_rexrap_syntax::errors::RexRapError;

use metadata::*;

type InterruptRegion = (String, Option<i64>, String, bool);

/// options controlling how a definition is rendered back to rexrap.
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
    // declared `node <id>: <type>` annotations recovered from graph metadata, kept as rendered rexrap
    // type text so declared type-name references survive the round trip.
    declared_types: HashMap<String, String>,
    // surface-form overrides for top-level workflow parameter fields that reference a declared
    // type name.
    input_types: HashMap<String, String>,
    // header alias declarations recovered from graph metadata, in declaration order.
    alias_decls: Vec<(String, Vec<Value>)>,
    // typed durable-resource imports retained in the authoring sidecar.
    resource_imports: Vec<ResourceImport>,
    // per-node `...alias` spread recipes (node id -> recipe segments) recovered from metadata.
    spreads: HashMap<String, Vec<Value>>,
    // control-block ids explicitly authored in REXRAP, recovered from metadata.
    control_ids: HashSet<String>,
    // authored loop/map binding names recovered from metadata.
    control_vars: HashMap<String, ControlVars>,
    // authored parallel labels and generated private branch terminals.
    parallel_branches: HashMap<String, metadata::ParallelSurface>,
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
) -> Result<String, RexRapError> {
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

    let metadata = MetadataReader::new(&graph.metadata);
    let declared_types = metadata.declared_types();
    let input_types = metadata.input_types();
    let alias_decls = metadata.alias_declarations();
    let resource_imports = metadata.resource_imports();
    let spreads = metadata.spreads();
    let control_ids = metadata.control_ids();
    let control_vars = metadata.control_vars();
    let parallel_branches = metadata.parallel_branches();

    let mut decompiler = Decompiler {
        nodes,
        end_ids,
        fail_ids,
        explicit: options.explicit,
        loop_vars: Vec::new(),
        declared_types,
        input_types,
        alias_decls,
        resource_imports,
        spreads,
        control_ids,
        control_vars,
        parallel_branches,
        visited: HashSet::new(),
        worklist: VecDeque::new(),
        queued: HashSet::new(),
        out: String::new(),
        indent: 0,
    };

    // top-level `fn` definitions render before the workflow block (document = func_def* ~ workflow).
    decompiler.emit_functions(&metadata.functions())?;

    if let Some(namespace) = &definition.namespace {
        decompiler.line(&format!("namespace {namespace} {{"));
        decompiler.indent += 1;
    }

    let returns = metadata
        .output_type()
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
    if let Some(key) = &definition.key {
        decompiler.line(&format!("key {key}"));
        decompiler.out.push('\n');
    }
    if let Some(workspace) = graph.metadata.get("workspace") {
        decompiler.line(&format!("workspace {}", decompiler.expr(workspace)?));
    }
    decompiler.emit_resource_imports();
    decompiler.emit_triggers(metadata.triggers())?;
    decompiler.emit_notifications(metadata.notifications())?;
    decompiler.emit_concurrency(metadata.concurrency())?;
    decompiler.emit_watches(metadata.watches())?;
    let interrupt_regions = decompiler.interrupt_regions(graph, metadata.interrupts())?;
    decompiler.emit_interrupts(&interrupt_regions)?;
    decompiler.emit_correlation(metadata.correlation())?;
    decompiler.emit_ingress(metadata.ingress())?;
    decompiler.emit_type_decls(&metadata.type_declarations())?;
    decompiler.emit_alias_decls()?;

    let start = graph
        .start
        .as_deref()
        .ok_or_else(|| RexRapError::Decompile("workflow has no start node".into()))?;
    let entry = decompiler
        .nodes
        .get(start)
        .and_then(|node| node.transitions.next.as_ref())
        .map(|target| target.as_str().to_string());
    if let Some(entry) = entry {
        // the explicit form names the otherwise-synthetic start edge. it is a header declaration,
        // so it renders above the runtime block rather than inside it.
        if decompiler.explicit {
            let label = decompiler.target_label(&entry);
            decompiler.line(&format!("start -> {label}"));
        }
        // every statement a run executes lives inside exactly one `do { … }` runtime block.
        decompiler.line("do {");
        decompiler.indent += 1;
        decompiler.emit_region(&entry, None)?;
    } else {
        decompiler.line("do {");
        decompiler.indent += 1;
    }

    // emit any nodes reached only by fail/reject/timeout arrows or convergence as top-level
    // labelled statements; references to them elsewhere were rendered as `-> label` arrows.
    while let Some(id) = decompiler.worklist.pop_front() {
        if decompiler.visited.contains(&id) || !decompiler.nodes.contains_key(id.as_str()) {
            continue;
        }
        decompiler.emit_region(&id, None)?;
    }

    // emit any remaining nodes with no incoming reference at all (true orphans). a node freshly
    // added in the editor is disconnected until the author wires it; without this pass it has no
    // path from `start` and would silently vanish from the decompiled rexrap. nodes that are unvisited
    // but still referenced somewhere (a join consumed by its parallel, a convergence target) are
    // left alone, since force-emitting them at top level would double-render. authored order keeps
    // output stable.
    let referenced = referenced_node_ids(graph);
    let orphan_ids: Vec<String> = graph
        .nodes
        .iter()
        .filter(|node| {
            !decompiler.visited.contains(&node.id)
                && !referenced.contains(&node.id)
                && Some(&node.id) != graph.start.as_ref()
                && !matches!(
                    node.kind,
                    WorkflowNodeKind::Start
                        | WorkflowNodeKind::End
                        | WorkflowNodeKind::Fail
                        // an entry point with no statement syntax: it is rendered by the
                        // `interrupt on` header line, and force-emitting it here would error.
                        | WorkflowNodeKind::Interrupt
                )
        })
        .map(|node| node.id.clone())
        .collect();
    for id in orphan_ids {
        if decompiler.visited.contains(&id) {
            continue;
        }
        decompiler.emit_region(&id, None)?;
    }

    // close the runtime block before the workflow's own closing brace.
    decompiler.indent -= 1;
    decompiler.line("}");

    decompiler.indent -= 1;
    decompiler.line("}");
    if definition.namespace.is_some() {
        decompiler.indent -= 1;
        decompiler.line("}");
    }
    Ok(decompiler.out)
}

/// Render one lowered pure expression using the same inverse mapping as workflow decompilation.
pub fn render_expression(value: &Value) -> Result<String, RexRapError> {
    Decompiler {
        nodes: HashMap::new(),
        end_ids: HashSet::new(),
        fail_ids: HashSet::new(),
        explicit: false,
        loop_vars: Vec::new(),
        declared_types: HashMap::new(),
        input_types: HashMap::new(),
        alias_decls: Vec::new(),
        resource_imports: Vec::new(),
        spreads: HashMap::new(),
        control_ids: HashSet::new(),
        control_vars: HashMap::new(),
        parallel_branches: HashMap::new(),
        visited: HashSet::new(),
        worklist: VecDeque::new(),
        queued: HashSet::new(),
        out: String::new(),
        indent: 0,
    }
    .expr(value)
}

/// collect every node id referenced as a target anywhere in the graph: typed transitions, branch
/// targets, and any `{"$node": "..."}` ref nested in node parameters (control-flow targets, join
/// dependencies, switch cases, etc.). a node absent from this set has no incoming edge.
fn referenced_node_ids(graph: &runinator_models::workflows::WorkflowGraph) -> HashSet<String> {
    let mut referenced = HashSet::new();
    for node in &graph.nodes {
        for target in transition_targets(&node.transitions) {
            referenced.insert(target);
        }
        let mut param_refs = Vec::new();
        collect_node_refs(node.parameters.as_value(), &mut param_refs);
        referenced.extend(param_refs);
    }
    referenced
}

/// recover declared `let` types from the graph metadata sidecar at `/rexrap/types` as rendered rexrap
/// type text. newer graphs store the surface string directly; older graphs stored a native
/// `RuninatorType` schema, which is rendered back for compatibility.
// render a `.retry(...)` modifier from the model, or `None` when every field is at its default and
// `explicit` rendering is off. mirrors the REXRAP named-arg surface so compile->decompile round-trips.
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
    Some(format!("@retry({})", args.join(", ")))
}

impl<'a> Decompiler<'a> {
    fn emit_resource_imports(&mut self) {
        if self.resource_imports.is_empty() {
            return;
        }
        for import in self.resource_imports.clone() {
            let revision = import
                .revision
                .map(|revision| format!(" @revision({revision})"))
                .unwrap_or_default();
            self.line(&format!(
                "import {} {}{} as {}",
                import.kind, import.path, revision, import.alias
            ));
        }
        self.out.push('\n');
    }

    fn resource_alias(&self, kind: &str, path: &str) -> Option<&str> {
        self.resource_imports
            .iter()
            .find(|import| import.kind == kind && (import.path == path || import.alias == path))
            .map(|import| import.alias.as_str())
    }

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

    fn emit_params(&mut self, input_type: &RuninatorType) -> Result<(), RexRapError> {
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
    fn emit_functions(&mut self, functions: &[FnEntry]) -> Result<(), RexRapError> {
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

    fn emit_triggers(&mut self, triggers: &[Value]) -> Result<(), RexRapError> {
        if triggers.is_empty() {
            return Ok(());
        }
        for trigger in triggers {
            // `kind` selects the surface form; absent ⇒ cron for packs compiled before the field.
            let is_chained = trigger.get("kind").and_then(Value::as_str) == Some("chained");
            let mut text = if is_chained {
                let target = trigger
                    .get("target_workflow")
                    .and_then(Value::as_str)
                    .ok_or_else(|| {
                        RexRapError::Decompile("chained trigger missing target".into())
                    })?;
                let keyword = match trigger.get("on").and_then(Value::as_str) {
                    Some("failure") => "on_failure",
                    Some("complete") => "on_complete",
                    _ => "on_success",
                };
                format!("trigger {keyword} workflow {}", quote(target))
            } else if let Some(schedule) = trigger.get("schedule") {
                format!("trigger schedule {}", self.expr(schedule)?)
            } else {
                let cron = trigger.get("cron").and_then(Value::as_str).ok_or_else(|| {
                    RexRapError::Decompile("trigger missing cron expression".into())
                })?;
                format!("trigger cron {}", quote(cron))
            };
            let params = trigger.get("parameters");
            let has_params = params
                .and_then(Value::as_object)
                .is_some_and(|object| !object.is_empty());
            if has_params {
                let rendered = self.expr(params.unwrap_or(&Value::Null))?;
                text.push_str(&format!(" with {rendered}"));
            }
            if trigger.get("enabled").and_then(Value::as_bool) == Some(false) {
                text.push_str(" disabled");
            }
            if !is_chained {
                if let Some(exclusions) = trigger.get("exclusions").and_then(Value::as_array) {
                    for exclusion in exclusions {
                        text.push_str(&format!(" blackout schedule {}", self.expr(exclusion)?));
                    }
                }
                if let (Some(start), Some(end)) = (
                    trigger.get("blackout_start").and_then(Value::as_str),
                    trigger.get("blackout_end").and_then(Value::as_str),
                ) {
                    text.push_str(&format!(" blackout {} to {}", quote(start), quote(end)));
                }
                if let Some(catchup) = trigger.get("catchup") {
                    // `fire_once` is the runtime default, so a spec carrying it explicitly still
                    // re-emits as `catchup fire_once` rather than vanishing.
                    let policy = catchup
                        .get("policy")
                        .and_then(Value::as_str)
                        .unwrap_or("fire_once");
                    text.push_str(&format!(" catchup {policy}"));
                    if let Some(grace) = catchup.get("grace_seconds").and_then(Value::as_i64) {
                        text.push_str(&format!(
                            " grace {}",
                            runinator_rexrap_syntax::format::format_duration(grace)
                        ));
                    }
                    if let Some(max_slots) = catchup.get("max_slots").and_then(Value::as_i64) {
                        text.push_str(&format!(" max {max_slots}"));
                    }
                }
            }
            self.line(&text);
        }
        self.out.push('\n');
        Ok(())
    }

    /// emit header `notify on ...` policies recovered from runtime metadata.
    fn emit_notifications(&mut self, policies: &[Value]) -> Result<(), RexRapError> {
        if policies.is_empty() {
            return Ok(());
        }
        for policy in policies {
            let event = match policy.get("event").and_then(Value::as_str) {
                Some("node_retry_exhausted") => "retry_exhausted",
                Some("run_sla_breached") => "sla",
                Some("run_parked") => "parked",
                _ => "failure",
            };
            let channel = match policy.get("channel").and_then(Value::as_str) {
                Some("email") => "email",
                Some("in_app") => "app",
                _ => "slack",
            };
            let target = policy
                .get("target")
                .and_then(Value::as_str)
                .ok_or_else(|| RexRapError::Decompile("notify policy missing target".into()))?;
            let mut text = format!("notify on {event} -> {channel} {}", quote(target));
            if let Some(threshold) = policy.get("threshold_seconds").and_then(Value::as_i64) {
                text.push_str(&format!(
                    " after {}",
                    runinator_rexrap_syntax::format::format_duration(threshold)
                ));
            }
            // `warning` is the surface default, so re-emitting it would not round-trip.
            if let Some(severity) = policy
                .get("severity")
                .and_then(Value::as_str)
                .filter(|severity| *severity != "warning")
            {
                text.push_str(&format!(" severity {severity}"));
            }
            let configuration = policy.get("configuration");
            let has_configuration = configuration
                .and_then(Value::as_object)
                .is_some_and(|object| !object.is_empty());
            if has_configuration {
                let rendered = self.expr(configuration.unwrap_or(&Value::Null))?;
                text.push_str(&format!(" with {rendered}"));
            }
            if policy.get("enabled").and_then(Value::as_bool) == Some(false) {
                text.push_str(" disabled");
            }
            self.line(&text);
        }
        self.out.push('\n');
        Ok(())
    }

    /// emit the header `concurrency <n> on_conflict <policy>` cap recovered from runtime metadata.
    fn emit_concurrency(&mut self, concurrency: Option<&Value>) -> Result<(), RexRapError> {
        let Some(concurrency) = concurrency else {
            return Ok(());
        };
        let Some(max_concurrent_runs) = concurrency
            .get("max_concurrent_runs")
            .and_then(Value::as_i64)
            .filter(|max| *max > 0)
        else {
            // an unlimited cap is what no header at all means, so emitting one would be noise.
            return Ok(());
        };
        let policy = concurrency
            .get("on_conflict")
            .and_then(Value::as_str)
            .unwrap_or("skip");
        self.line(&format!(
            "concurrency {max_concurrent_runs} on_conflict {policy}"
        ));
        self.out.push('\n');
        Ok(())
    }

    /// emit header `watch <cond> -> <target>` guards recovered from runtime metadata.
    fn emit_watches(&mut self, watches: &[Value]) -> Result<(), RexRapError> {
        if watches.is_empty() {
            return Ok(());
        }
        for watch in watches {
            let condition = watch
                .get("condition")
                .ok_or_else(|| RexRapError::Decompile("watch missing condition".into()))?;
            let handler = watch
                .get("handler")
                .and_then(Value::as_str)
                .ok_or_else(|| RexRapError::Decompile("watch missing handler".into()))?;
            let target = match handler {
                "end" => "end".to_string(),
                "fail" => "fail".to_string(),
                other => other.to_string(),
            };
            self.line(&format!("watch {} -> {target}", self.cond(condition)?));
        }
        self.out.push('\n');
        Ok(())
    }

    /// the handler regions to emit, as `(source, interval_seconds, first node of the body, enabled)`.
    ///
    /// metadata owns the source-to-entry link and enabled state. when its handler is an `interrupt`
    /// node, that structural entry is marked visited and the emitted body begins at its `next`.
    /// metadata pointing directly at a body remains supported for definitions from before entries.
    fn interrupt_regions(
        &mut self,
        graph: &WorkflowGraph,
        metadata: &[Value],
    ) -> Result<Vec<InterruptRegion>, RexRapError> {
        let regions = metadata
            .iter()
            .map(|interrupt| {
                let source = interrupt
                    .get("on")
                    .and_then(Value::as_str)
                    .ok_or_else(|| RexRapError::Decompile("interrupt missing source".into()))?;
                let handler = interrupt
                    .get("handler")
                    .and_then(Value::as_str)
                    .ok_or_else(|| RexRapError::Decompile("interrupt missing handler".into()))?;
                let enabled = interrupt
                    .get("enabled")
                    .and_then(Value::as_bool)
                    .unwrap_or(true);
                let interval_seconds = interrupt.get("interval_seconds").and_then(Value::as_i64);
                let body = match graph.nodes.iter().find(|node| node.id == handler) {
                    Some(node) if node.kind == WorkflowNodeKind::Interrupt => {
                        self.visited.insert(node.id.clone());
                        node.transitions
                            .next
                            .as_ref()
                            .map(|target| target.as_str().to_string())
                            .ok_or_else(|| {
                                RexRapError::Decompile(format!(
                                    "interrupt entry '{handler}' has no body"
                                ))
                            })?
                    }
                    _ => handler.to_string(),
                };
                Ok((source.to_string(), interval_seconds, body, enabled))
            })
            .collect::<Result<Vec<_>, RexRapError>>()?;
        let linked: HashSet<&str> = metadata
            .iter()
            .filter_map(|interrupt| interrupt.get("handler").and_then(Value::as_str))
            .collect();
        if let Some(node) = graph.nodes.iter().find(|node| {
            node.kind == WorkflowNodeKind::Interrupt && !linked.contains(node.id.as_str())
        }) {
            return Err(RexRapError::Decompile(format!(
                "interrupt entry '{}' is not linked by metadata",
                node.id
            )));
        }
        Ok(regions)
    }

    /// emit header `interrupt on <source> { ... }` regions.
    ///
    /// walking each region here — before the main flow — is also what keeps its nodes out of the
    /// orphan pass at the end. a region is unreachable from `start` by design, so without this it
    /// would be re-emitted as a pile of loose top-level statements.
    fn emit_interrupts(
        &mut self,
        regions: &[(String, Option<i64>, String, bool)],
    ) -> Result<(), RexRapError> {
        if regions.is_empty() {
            return Ok(());
        }
        for (source, interval_seconds, body, enabled) in regions {
            let disabled = if *enabled { "" } else { " disabled" };
            let every = interval_seconds
                .map(runinator_rexrap_syntax::format::format_duration)
                .map(|duration| format!(" every {duration}"))
                .unwrap_or_default();
            self.line(&format!("interrupt on {source}{every}{disabled} {{"));
            self.indent += 1;
            self.emit_region(body, None)?;
            self.indent -= 1;
            self.line("}");
        }
        self.out.push('\n');
        Ok(())
    }

    /// emit the header `correlate key <expr>` declaration recovered from `metadata.correlation`.
    fn emit_correlation(&mut self, expression: Option<&Value>) -> Result<(), RexRapError> {
        let Some(expression) = expression else {
            return Ok(());
        };
        self.line(&format!("correlate key {}", self.expr(expression)?));
        self.out.push('\n');
        Ok(())
    }

    fn emit_ingress(&mut self, value: Option<&Value>) -> Result<(), RexRapError> {
        let Some(value) = value else {
            return Ok(());
        };
        let policy: runinator_models::orchestration::IngressPolicy =
            serde_json::from_value(value.clone().into()).map_err(|error| {
                RexRapError::Decompile(format!("invalid ingress metadata: {error}"))
            })?;
        self.line(&format!(
            "ingress scope {} {{",
            serde_json::to_string(&policy.scope).unwrap_or_default()
        ));
        self.indent += 1;
        for route in policy.routes {
            let lifecycle = match route.lifecycle {
                runinator_models::orchestration::IngressLifecycle::Unbound => "unbound",
                runinator_models::orchestration::IngressLifecycle::Active => "active",
                runinator_models::orchestration::IngressLifecycle::Terminal => "terminal",
            };
            let action = match route.action {
                runinator_models::orchestration::IngressAction::Start => "start",
                runinator_models::orchestration::IngressAction::Interrupt => "interrupt",
                runinator_models::orchestration::IngressAction::Queue => "queue",
                runinator_models::orchestration::IngressAction::Record => "record",
                runinator_models::orchestration::IngressAction::Requeue => "requeue",
                runinator_models::orchestration::IngressAction::Dispatch => "dispatch",
            };
            self.line(&format!(
                "on {} when {lifecycle}",
                serde_json::to_string(&route.event_type).unwrap_or_default()
            ));
            self.indent += 1;
            for predicate in route.predicates {
                let operator = match predicate.operator {
                    runinator_models::orchestration::IngressPredicateOperator::Equal => "==",
                    runinator_models::orchestration::IngressPredicateOperator::NotEqual => "!=",
                    runinator_models::orchestration::IngressPredicateOperator::In => "in",
                    runinator_models::orchestration::IngressPredicateOperator::Contains => {
                        "contains"
                    }
                    runinator_models::orchestration::IngressPredicateOperator::Exists => "exists",
                };
                let value = predicate
                    .value
                    .as_ref()
                    .map(|value| render_expression(value).unwrap_or_else(|_| "null".into()))
                    .unwrap_or_default();
                self.line(
                    format!(
                        "if {} {operator} {value}",
                        serde_json::to_string(&predicate.pointer).unwrap_or_default()
                    )
                    .trim_end(),
                );
            }
            if route.action == runinator_models::orchestration::IngressAction::Dispatch {
                self.line(&format!(
                    "-> dispatch {}",
                    serde_json::to_string(route.intent.as_deref().unwrap_or_default())
                        .unwrap_or_default()
                ));
            } else {
                self.line(&format!("-> {action}"));
            }
            self.indent -= 1;
        }
        self.indent -= 1;
        self.line("}");
        self.out.push('\n');
        Ok(())
    }

    /// emit recovered `type <Name> ...` declarations from rendered surface strings. a struct (which
    /// renders starting with `{`) uses the brace shorthand; anything else uses the `= <type>` form.
    fn emit_type_decls(&mut self, decls: &[(String, String)]) -> Result<(), RexRapError> {
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
    fn emit_alias_decls(&mut self) -> Result<(), RexRapError> {
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
    fn emit_region(&mut self, cur: &str, stop: Option<&str>) -> Result<(), RexRapError> {
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
                return Err(RexRapError::Decompile(format!(
                    "workflow reaches node '{cur}' by more than one path (an unstructured loop or convergence) that cannot be decompiled to rexrap; author this workflow in rexrap directly"
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
                WorkflowNodeKind::Toggle => (self.emit_toggle(node, stop)?, false),
                WorkflowNodeKind::Percentage => (self.emit_split(node, stop)?, false),
                WorkflowNodeKind::Fail => {
                    let text = self.fail_text(node)?;
                    self.line(&text);
                    (None, true)
                }
                // terminal like `fail`: it ends its thread by handing control back, so the walk
                // stops here rather than looking for a successor.
                WorkflowNodeKind::Resume => {
                    let text = self.resume_text(node);
                    self.line(&text);
                    (None, true)
                }
                WorkflowNodeKind::Action
                | WorkflowNodeKind::Subflow
                | WorkflowNodeKind::Wait
                | WorkflowNodeKind::Output
                | WorkflowNodeKind::Input
                | WorkflowNodeKind::Approval
                | WorkflowNodeKind::Gate
                | WorkflowNodeKind::Signal
                | WorkflowNodeKind::Config
                | WorkflowNodeKind::Assert
                | WorkflowNodeKind::Transform
                | WorkflowNodeKind::Audit
                | WorkflowNodeKind::Checkpoint
                | WorkflowNodeKind::Throttle
                | WorkflowNodeKind::Cooldown
                | WorkflowNodeKind::AwaitRun
                | WorkflowNodeKind::Debounce
                | WorkflowNodeKind::Collect
                | WorkflowNodeKind::Barrier
                | WorkflowNodeKind::CircuitBreaker
                | WorkflowNodeKind::Invocation
                | WorkflowNodeKind::EventSource => {
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
                WorkflowNodeKind::Mutex => (self.emit_mutex(node, stop)?, false),
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
                // entry points and `end` have no statement syntax. an `interrupt` is rendered by
                // its `interrupt on` header line, and the walk into a region starts past it, so
                // reaching one here means a malformed graph — stop rather than emit.
                WorkflowNodeKind::Start | WorkflowNodeKind::End | WorkflowNodeKind::Interrupt => {
                    (None, true)
                }
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

    /// whether a node is a synthetic join, which has no standalone rexrap statement form.
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
            parts.push(format!("@deadline({timeout}s)"));
        }
        if parts.is_empty() {
            String::new()
        } else {
            format!("{} ", parts.join(" "))
        }
    }

    fn target_label(&self, id: &str) -> String {
        if self.end_ids.contains(id) {
            "end".to_string()
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
        // the explicit form always names the block's continuation edge with an attached
        // `routes { on next { … } }` section, still walking inline into a fresh successor so it is
        // emitted once.
        if self.explicit {
            let Some(c) = cont else {
                self.line(closing);
                return None;
            };
            let label = self.target_label(&c);
            self.close_block_route(closing, "on next", &label);
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
                self.close_block_route(closing, "on next", "fail");
                None
            }
            Some(c) if self.visited.contains(&c) => {
                self.close_block_route(closing, "on next", &c);
                None
            }
            Some(c) => {
                self.line(closing);
                Some(c)
            }
        }
    }

    /// close a control block and attach its continuation as a `routes { … }` section, matching the
    /// shape `emit_leaf` uses so a block and a leaf render their edges identically.
    fn close_block_route(&mut self, closing: &str, head: &str, label: &str) {
        self.line(closing);
        self.line("routes {");
        self.indent += 1;
        self.line(&format!("{head} {{"));
        self.indent += 1;
        self.line(&format!("continue {label}"));
        self.indent -= 1;
        self.line("}");
        self.indent -= 1;
        self.line("}");
    }

    // leaf statements -------------------------------------------------------

    /// emit a single leaf statement with its outcome arrows. returns the success target.
    fn emit_leaf(
        &mut self,
        node: &WorkflowNode,
        stop: Option<&str>,
    ) -> Result<Option<String>, RexRapError> {
        let (text, lets_binding) = self.statement_text(node)?;
        let prefix = if lets_binding {
            match self.declared_types.get(&node.id) {
                Some(rendered) => format!(
                    "{}let {}: {} = ",
                    self.annotation_prefix(node, false),
                    node.id,
                    rendered
                ),
                None => format!("{}let {} = ", self.annotation_prefix(node, false), node.id),
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
            (Some(target), _) => ("success", Some(target.as_str().to_string())),
            (None, Some(target)) => ("next", Some(target.as_str().to_string())),
            (None, None) => ("success", None),
        };

        // collect failure-style arrows and queue their targets for top-level emission, since
        // the linear walk never descends into them.
        let mut arrows: Vec<(String, String)> = Vec::new();
        for (outcome, target) in [
            ("failure", &transitions.on_failure),
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
            // a join has no rexrap statement form, so its branch edges stay structural even in the
            // explicit form (the enclosing `parallel { branch }` already expresses them).
            Some(id) if self.explicit && !self.is_join(id) => Some(self.target_label(id)),
            _ => None,
        };

        // gather every rendered outgoing edge into one attached `routes { … }` section. the pure
        // linear successor stays implicit (success_arrow is None), so most nodes emit no section;
        // only explicit jumps and failure routes surface one.
        let mut routes: Vec<(String, String)> = Vec::new();
        if let Some(label) = &success_arrow {
            let kw = if self.explicit { succ_kw } else { "success" };
            routes.push((format!("on {kw}"), label.clone()));
        }
        for (outcome, label) in &arrows {
            routes.push((format!("on {outcome}"), label.clone()));
        }
        // user-defined predicate routes, preserved in declaration order; an explicit `priority`
        // token is rendered whenever the branch carries one, keeping the round-trip stable.
        for branch in &transitions.branches {
            let cond = self.cond(&branch.when.to_value())?;
            let label = self.target_label(branch.target.as_str());
            self.defer(branch.target.as_str());
            let head = match branch.priority {
                Some(priority) => format!("when {cond} priority {priority}"),
                None => format!("when {cond}"),
            };
            routes.push((head, label));
        }

        for attribute in self.node_attributes(node)? {
            self.line(&attribute);
        }
        self.line(&format!("{prefix}{text}"));
        if !routes.is_empty() {
            self.line("routes {");
            self.indent += 1;
            for (head, label) in &routes {
                self.line(&format!("{head} {{"));
                self.indent += 1;
                self.line(&format!("continue {label}"));
                self.indent -= 1;
                self.line("}");
            }
            self.indent -= 1;
            self.line("}");
        }

        Ok(success)
    }

    /// returns the statement text and whether it should be prefixed with `node <id> <-`.
    fn statement_text(&self, node: &WorkflowNode) -> Result<(String, bool), RexRapError> {
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
            // an invocation renders from the authored statement list it retains, never from its
            // compiled module. reconstructing `let`/`if`/`return` out of a flat instruction stream
            // is control-flow recovery — a decompiler in the hard sense — and it would have to be
            // exactly right or the editor pane would silently rewrite the user's code. carrying the
            // source beside the bytecode is the same arrangement `metadata.rexrap.functions` already
            // uses for function signatures.
            WorkflowNodeKind::Invocation => {
                let program = invocation_source(node).ok_or_else(|| {
                    RexRapError::lower(format!(
                        "node '{}' is an invocation with no retained source to render",
                        node.id
                    ))
                })?;
                Ok((self.compute_text(node, &program)?, true))
            }
            WorkflowNodeKind::Subflow => Ok((self.subflow_text(node)?, true)),
            WorkflowNodeKind::Resume => Ok((self.resume_text(node), false)),
            WorkflowNodeKind::Wait => Ok((self.wait_text(node)?, false)),
            WorkflowNodeKind::Output => Ok((self.output_text(node)?, false)),
            WorkflowNodeKind::Input => Ok((self.input_text(node)?, false)),
            WorkflowNodeKind::Approval => Ok((self.approval_text(node)?, false)),
            WorkflowNodeKind::Gate => Ok((self.gate_text(node)?, false)),
            WorkflowNodeKind::Signal => Ok((self.signal_text(node)?, false)),
            WorkflowNodeKind::Assert => Ok((self.assert_text(node)?, false)),
            WorkflowNodeKind::Transform => Ok((self.transform_text(node)?, false)),
            WorkflowNodeKind::Audit => Ok((self.audit_text(node)?, false)),
            WorkflowNodeKind::Checkpoint => Ok((self.checkpoint_text(node)?, false)),
            WorkflowNodeKind::Mutex => Ok((self.mutex_text(node)?, false)),
            WorkflowNodeKind::Throttle => Ok((self.throttle_text(node)?, false)),
            WorkflowNodeKind::Cooldown => Ok((self.cooldown_text(node)?, false)),
            WorkflowNodeKind::AwaitRun => Ok((self.await_text(node)?, false)),
            WorkflowNodeKind::Debounce => Ok((self.debounce_text(node)?, false)),
            WorkflowNodeKind::Collect => Ok((self.collect_text(node)?, false)),
            WorkflowNodeKind::Barrier => Ok((self.barrier_text(node)?, false)),
            WorkflowNodeKind::CircuitBreaker => Ok((self.circuit_breaker_text(node)?, false)),
            WorkflowNodeKind::EventSource => Ok((self.event_source_text(node)?, false)),
            WorkflowNodeKind::Config => Ok((self.config_text(node)?, false)),
            other => Err(RexRapError::Decompile(format!("unexpected leaf {other:?}"))),
        }
    }

    // render a compute block. inner lines carry their absolute indentation so the caller's
    // `self.line` (which only indents the first line) yields correctly nested output, and the
    // trailing success arrow appends cleanly after the closing brace.
    fn compute_text(&self, _node: &WorkflowNode, program: &[Value]) -> Result<String, RexRapError> {
        let base = self.indent;
        let mut out = String::from("compute {\n");
        self.render_compute_lines(&mut out, program, base + 1)?;
        out.push_str(&"    ".repeat(base));
        out.push('}');
        Ok(out)
    }

    fn render_compute_lines(
        &self,
        out: &mut String,
        program: &[Value],
        indent: usize,
    ) -> Result<(), RexRapError> {
        let pad = "    ".repeat(indent);
        for statement in program {
            let object = statement.as_object().ok_or_else(|| {
                RexRapError::Decompile("compute statement must be an object".into())
            })?;
            if let Some(name) = object.get("$let").and_then(Value::as_str) {
                let value = object
                    .get("value")
                    .ok_or_else(|| RexRapError::Decompile("compute let missing value".into()))?;
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

    fn foreign_compute_text(&self, node: &WorkflowNode) -> Result<String, RexRapError> {
        let action = node
            .action
            .as_ref()
            .ok_or_else(|| RexRapError::Decompile("foreign compute node missing action".into()))?;
        let config = action.configuration.as_value();
        let language = config
            .get("language")
            .and_then(Value::as_str)
            .ok_or_else(|| RexRapError::Decompile("foreign compute missing language".into()))?;
        let source = config
            .get("source")
            .and_then(Value::as_str)
            .ok_or_else(|| RexRapError::Decompile("foreign compute missing source".into()))?;
        if source.contains("```") {
            return Err(RexRapError::Decompile(
                "foreign compute source contains a code fence delimiter".into(),
            ));
        }

        let mut out = format!("compute {}", quote(language));
        out.push_str(" ```\n");
        out.push_str(source);
        if !source.ends_with('\n') {
            out.push('\n');
        }
        out.push_str("```");

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

    /// a step's `@`-attributes, rendered inline ahead of the call they belong to.
    ///
    /// they must *precede* the call: written after it they would be indistinguishable from the
    /// attributes prefixing the next statement, and a decompile/recompile round trip would move
    /// them onto it.
    fn modifier_prefix(&self, modifiers: &[String]) -> String {
        if modifiers.is_empty() {
            return String::new();
        }
        format!("{} ", modifiers.join(" "))
    }

    fn action_text(&self, node: &WorkflowNode) -> Result<String, RexRapError> {
        let action = node
            .action
            .as_ref()
            .ok_or_else(|| RexRapError::Decompile("action node missing action".into()))?;
        // a packaged-function call is rendered from its binding, which carries the authored names
        // as well as the ids. reaching into a catalog here would make the same definition decompile
        // differently depending on what the catalog currently holds — including not at all, once
        // the package is deleted. the binding is part of the definition, so it always answers.
        let (call_provider, call_function) = match &action.function_binding {
            Some(binding) => {
                let provider = binding.provider_name();
                let path = provider.strip_prefix("functions.").unwrap_or(&provider);
                let provider = self
                    .resource_alias("functions", path)
                    .unwrap_or(&provider)
                    .to_string();
                (provider, binding.export_name.clone())
            }
            None => (action.provider.clone(), action.function.clone()),
        };
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
            // a bound call nests its authored arguments under `input`; everything else in the
            // configuration is worker staging the author never wrote and must not see back.
            let configuration = match (&action.function_binding, action.configuration.as_value()) {
                (Some(_), Value::Object(config)) => config
                    .get("input")
                    .cloned()
                    .unwrap_or(Value::Object(Map::new())),
                (_, other) => other.clone(),
            };
            if let Value::Object(config) = &configuration {
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
        let mut text = format!(
            "{call_provider}.{call_function}{}",
            self.call_args(&arg_parts, base)
        );
        if let Some(compensation) = &node.compensation {
            text.push_str(&format!(
                " compensate {}",
                self.action_call_text(compensation, base)?
            ));
        }
        Ok(text)
    }

    /// every `@`-attribute a node's step carries, in the order the formatter emits them.
    fn node_attributes(&self, node: &WorkflowNode) -> Result<Vec<String>, RexRapError> {
        let Some(action) = &node.action else {
            return Ok(Vec::new());
        };
        let mut out = Vec::new();
        out.extend(self.call_timeout_modifier(action));
        if let Some(retry) = decompile_retry(&node.retry, self.explicit) {
            out.push(retry);
        }
        out.extend(self.call_modifiers(action)?);
        if node.reentry.enabled {
            out.push(format!("@reentry(max_visits: {})", node.reentry.max_visits));
        }
        Ok(out)
    }

    /// the call's `@timeout(...)`, kept apart from [`Self::call_modifiers`] because a node's
    /// `@retry(...)` renders between the two and the formatter's golden output pins that order.
    fn call_timeout_modifier(
        &self,
        action: &runinator_models::workflows::WorkflowAction,
    ) -> Option<String> {
        (self.explicit || action.timeout_seconds != 60)
            .then(|| format!("@timeout({}s)", action.timeout_seconds))
    }

    /// the remaining modifiers that belong to the call itself rather than to the node around it.
    /// shared with `compensate`, whose clause is the same `action_stmt` and therefore carries the
    /// same modifiers — lowering has always preserved them, so dropping them here silently reverted
    /// a compensation's timeout, tags, and runner on every editor round trip.
    fn call_modifiers(
        &self,
        action: &runinator_models::workflows::WorkflowAction,
    ) -> Result<Vec<String>, RexRapError> {
        let mut modifiers = Vec::new();
        if !action.tags.is_empty() {
            let tags = action
                .tags
                .iter()
                .map(|tag| quote(tag))
                .collect::<Vec<_>>()
                .join(", ");
            modifiers.push(format!("@tags({tags})"));
        }
        if action.mcp_enabled {
            modifiers.push("@mcp".to_string());
        }
        if let Some(runner) = action.required_labels.get("runner") {
            // lowering adds the functions runner label itself when the author wrote no `.runner`,
            // so re-emitting it here would grow a modifier on every round trip.
            let implicit = action.function_binding.is_some()
                && runner == runinator_models::functions::FUNCTIONS_RUNNER_LABEL;
            if !implicit {
                modifiers.push(format!("@runner({})", quote(runner)));
            }
        }
        if let Some(affinity) = &action.workspace_affinity {
            modifiers.push(format!("@workspace({})", self.expr(affinity)?));
        }
        if let Some(profile) = &action.execution_profile {
            modifiers.push(format!("@profile({})", quote(profile.name())));
        }
        if let Some(key) = &action.idempotency_key {
            modifiers.push(format!("@idempotent(key: {})", self.expr(key)?));
        }
        Ok(modifiers)
    }

    /// render a `provider.function(args)` call from a `WorkflowAction` (used for `compensate`).
    fn action_call_text(
        &self,
        action: &runinator_models::workflows::WorkflowAction,
        base: usize,
    ) -> Result<String, RexRapError> {
        let (provider, function, configuration) = match &action.function_binding {
            Some(binding) => {
                let bound_provider = binding.provider_name();
                let path = bound_provider
                    .strip_prefix("functions.")
                    .unwrap_or(&bound_provider);
                let provider = self
                    .resource_alias("functions", path)
                    .unwrap_or(&bound_provider)
                    .to_string();
                let configuration = action
                    .configuration
                    .get("input")
                    .cloned()
                    .unwrap_or(Value::Object(Map::new()));
                (provider, binding.export_name.clone(), configuration)
            }
            None => (
                action.provider.clone(),
                action.function.clone(),
                action.configuration.as_value().clone(),
            ),
        };
        let mut args = Vec::new();
        if let Value::Object(config) = &configuration {
            for (name, value) in config {
                args.push(format!("{name}: {}", self.expr_multiline(value, base + 1)?));
            }
        }
        let multiline = !args.is_empty();
        let mut modifiers = Vec::new();
        modifiers.extend(self.call_timeout_modifier(action));
        modifiers.extend(self.call_modifiers(action)?);
        let _ = multiline;
        Ok(format!(
            "{}{}.{}{}",
            self.modifier_prefix(&modifiers),
            provider,
            function,
            self.call_args(&args, base),
        ))
    }

    fn subflow_text(&self, node: &WorkflowNode) -> Result<String, RexRapError> {
        let subflow = &node.subflow;
        let name = subflow.workflow_name.clone().unwrap_or_default();
        let imported = self.resource_alias("workflow", &name);
        let mut args = vec![imported.map(str::to_string).unwrap_or_else(|| quote(&name))];
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
        } else if let Value::Object(params) = node.parameters.as_value()
            && !params.is_empty()
        {
            let mut parts = Vec::new();
            for (name, value) in params {
                parts.push(format!("{name}: {}", self.expr_multiline(value, base + 1)?));
            }
            params_arg = Some(format!("params: {}", self.parts_object(&parts, base)));
        }
        if let Some(params_arg) = params_arg {
            args.insert(1, params_arg);
        }
        Ok(format!("subflow({})", args.join(", ")))
    }

    fn wait_text(&self, node: &WorkflowNode) -> Result<String, RexRapError> {
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

    fn output_text(&self, node: &WorkflowNode) -> Result<String, RexRapError> {
        let items = node
            .parameters
            .get("items")
            .and_then(Value::as_array)
            .filter(|v| !v.is_empty());
        // block form when artifact items are present.
        if let Some(items) = items {
            let base = self.indent;
            let mut out = String::from("output {\n");
            let event = node.parameters.get("event_type").and_then(Value::as_str);
            let data = node.parameters.get("data");
            let has_event_data =
                event.is_some() || matches!(data, Some(v) if !matches!(v, Value::Null));
            if has_event_data {
                out.push_str(&"    ".repeat(base + 1));
                out.push_str("emit");
                if let Some(event_type) = event {
                    out.push_str(&format!(" {}", quote(event_type)));
                }
                match data {
                    None | Some(Value::Null) => out.push_str(" {}"),
                    Some(d @ Value::Object(_)) => {
                        out.push_str(&format!(" {}", self.expr_multiline(d, base + 1)?))
                    }
                    Some(other) => {
                        let rendered = self.expr(other)?;
                        if event.is_some() {
                            out.push_str(&format!(" {rendered}"));
                        } else {
                            out.push_str(&format!(" ({rendered})"));
                        }
                    }
                }
                out.push('\n');
            }
            for item in items {
                let name = item.get("name").and_then(Value::as_str).unwrap_or_default();
                let source = item.get("source").cloned().unwrap_or(Value::Null);
                out.push_str(&"    ".repeat(base + 1));
                out.push_str(&format!("{name} = {}\n", self.expr(&source)?));
            }
            out.push_str(&"    ".repeat(base));
            out.push('}');
            return Ok(out);
        }
        // shorthand `emit` form for event-only nodes (idempotent for legacy emit nodes).
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

    /// `fail` with its optional message. the message is an ordinary expression, so it must be
    /// rendered back or `fail "reason"` silently loses its reason on every editor round trip.
    fn fail_text(&self, node: &WorkflowNode) -> Result<String, RexRapError> {
        let mut text = "fail".to_string();
        if let Some(message) = node.parameters.get("message")
            && !matches!(message, Value::Null)
        {
            text.push(' ');
            text.push_str(&self.expr(message)?);
        }
        Ok(text)
    }

    /// `resume`, `resume next`, `resume restart`, `resume fail`. the compiled `continue` mode is
    /// spelled `next` in source, and the default mode renders bare.
    fn resume_text(&self, node: &WorkflowNode) -> String {
        match node.parameters.get("mode").and_then(Value::as_str) {
            Some("continue") => "resume next".to_string(),
            Some("restart") => "resume restart".to_string(),
            Some("fail") => "resume fail".to_string(),
            _ => "resume".to_string(),
        }
    }

    fn input_text(&self, node: &WorkflowNode) -> Result<String, RexRapError> {
        let mut text = "input".to_string();
        if let Some(prompt) = node.parameters.get("prompt")
            && !matches!(prompt, Value::Null)
        {
            text.push(' ');
            text.push_str(&self.expr(prompt)?);
        }
        Ok(text)
    }

    fn approval_text(&self, node: &WorkflowNode) -> Result<String, RexRapError> {
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

    fn gate_text(&self, node: &WorkflowNode) -> Result<String, RexRapError> {
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
        if let Some(policy) = node
            .parameters
            .get("timeout_policy")
            .and_then(Value::as_str)
        {
            text.push_str(&format!(" on_timeout {policy}"));
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
                    !matches!(
                        name.as_str(),
                        "kind" | "when" | "poll_interval" | "timeout" | "timeout_policy"
                    )
                })
                .map(|(name, value)| (name.as_str(), value))
                .collect();
            if !entries.is_empty() {
                text.push_str(&format!(" {}", self.entries_object(&entries, base)?));
            }
        }
        Ok(text)
    }

    fn signal_text(&self, node: &WorkflowNode) -> Result<String, RexRapError> {
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

    fn assert_text(&self, node: &WorkflowNode) -> Result<String, RexRapError> {
        let assertions = node
            .parameters
            .get("assertions")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        if assertions.is_empty() {
            return Ok("assert {}".to_string());
        }
        let base = self.indent;
        let pad = "    ".repeat(base + 1);
        let mut out = String::from("assert {\n");
        for assertion in &assertions {
            let name = assertion
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or_default();
            let cond = assertion.get("condition").cloned().unwrap_or(Value::Null);
            out.push_str(&format!("{pad}{}: {}\n", quote(name), self.cond(&cond)?));
        }
        out.push_str(&"    ".repeat(base));
        out.push('}');
        Ok(out)
    }

    fn transform_text(&self, node: &WorkflowNode) -> Result<String, RexRapError> {
        let bindings = node
            .parameters
            .get("bindings")
            .cloned()
            .unwrap_or(Value::Null);
        let entries = match bindings.as_object() {
            Some(map) if !map.is_empty() => map,
            _ => return Ok("transform {}".to_string()),
        };
        let base = self.indent;
        let pad = "    ".repeat(base + 1);
        let mut out = String::from("transform {\n");
        for (name, value) in entries.iter() {
            out.push_str(&format!("{pad}{name} = {}\n", self.expr(value)?));
        }
        out.push_str(&"    ".repeat(base));
        out.push('}');
        Ok(out)
    }

    fn audit_text(&self, node: &WorkflowNode) -> Result<String, RexRapError> {
        let action = node
            .parameters
            .get("action")
            .cloned()
            .unwrap_or(Value::Null);
        let mut text = format!("audit action {}", self.expr(&action)?);
        for field in ["actor", "target", "reason"] {
            if let Some(value) = node.parameters.get(field) {
                text.push_str(&format!(" {field} {}", self.expr(value)?));
            }
        }
        Ok(text)
    }

    fn checkpoint_text(&self, node: &WorkflowNode) -> Result<String, RexRapError> {
        let name = node
            .parameters
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or_default();
        Ok(format!("checkpoint {}", quote(name)))
    }

    fn mutex_text(&self, node: &WorkflowNode) -> Result<String, RexRapError> {
        let name = node
            .parameters
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or_default();
        // a bare release leaf carries only the lock name.
        if mutex_is_release(node) {
            return Ok(format!("mutex release {}", quote(name)));
        }
        let mut text = format!("mutex {}", quote(name));
        if let Some(poll) = node
            .parameters
            .get("poll_interval_seconds")
            .and_then(Value::as_i64)
        {
            text.push_str(&format!(" every {poll}s"));
        }
        if let Some(hold) = node
            .parameters
            .get("hold_timeout_seconds")
            .and_then(Value::as_i64)
        {
            text.push_str(&format!(" hold {hold}s"));
        }
        // the node timeout round-trips through the `@timeout(...)` annotation prefix, so it is not
        // rendered inline here (doing so would double-emit it).
        Ok(text)
    }

    /// emit a mutex node. an acquire that brackets a body (paired with a `<id>_release` node) renders
    /// the `mutex "..." { ... }` critical-section block; a plain acquire or a bare release renders as
    /// a leaf. mirrors how `emit_parallel` consumes its join.
    fn emit_mutex(
        &mut self,
        node: &WorkflowNode,
        outer_stop: Option<&str>,
    ) -> Result<Option<String>, RexRapError> {
        if !mutex_is_release(node)
            && let Some((release_id, cont)) = self.find_mutex_release(node)
        {
            let body_entry = node
                .transitions
                .next
                .as_ref()
                .map(|target| target.as_str().to_string());
            self.line(&format!(
                "{}{} {{",
                self.block_id_prefix(node),
                self.mutex_text(node)?
            ));
            self.indent += 1;
            if let Some(body) = &body_entry {
                self.emit_region(body, Some(release_id.as_str()))?;
            }
            self.indent -= 1;
            return Ok(self.close_block_line("}", cont, outer_stop));
        }
        // a plain acquire or a bare release leaf: emit it and advance like any other leaf.
        let success = self.emit_leaf(node, outer_stop)?;
        Ok(match success {
            Some(next)
                if !self.is_terminal(&next)
                    && outer_stop != Some(next.as_str())
                    && !self.visited.contains(&next) =>
            {
                Some(next)
            }
            _ => None,
        })
    }

    /// the release node closing an acquire's critical section, if one exists. uses the lowerer's
    /// stable `<acquire_id>_release` id and matches the lock name. returns the release id and the
    /// section's continuation (the release node's `next`).
    fn find_mutex_release(&self, acquire: &WorkflowNode) -> Option<(String, Option<String>)> {
        let name = acquire.parameters.get("name").and_then(Value::as_str);
        let release = self.nodes.get(format!("{}_release", acquire.id).as_str())?;
        if !matches!(release.kind, WorkflowNodeKind::Mutex) || !mutex_is_release(release) {
            return None;
        }
        if release.parameters.get("name").and_then(Value::as_str) != name {
            return None;
        }
        let cont = release
            .transitions
            .next
            .as_ref()
            .map(|target| target.as_str().to_string());
        Some((release.id.clone(), cont))
    }

    fn throttle_text(&self, node: &WorkflowNode) -> Result<String, RexRapError> {
        let name = node
            .parameters
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let rate = node
            .parameters
            .get("max_per_window")
            .and_then(Value::as_i64)
            .unwrap_or(0);
        let window = node
            .parameters
            .get("window_seconds")
            .and_then(Value::as_i64)
            .unwrap_or(0);
        let mut text = format!("throttle {} rate {rate} per {window}s", quote(name));
        if let Some(poll) = node
            .parameters
            .get("poll_interval_seconds")
            .and_then(Value::as_i64)
        {
            text.push_str(&format!(" every {poll}s"));
        }
        Ok(text)
    }

    fn cooldown_text(&self, node: &WorkflowNode) -> Result<String, RexRapError> {
        let name = node
            .parameters
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let window = node
            .parameters
            .get("window_seconds")
            .and_then(Value::as_i64)
            .unwrap_or(0);
        Ok(format!("cooldown {} every {window}s", quote(name)))
    }

    fn await_text(&self, node: &WorkflowNode) -> Result<String, RexRapError> {
        if let Some(reference) = node.parameters.get("run_id").and_then(Value::as_object)
            && let Some(task) =
                reference
                    .get("$ref")
                    .and_then(Value::as_object)
                    .and_then(|reference| {
                        (reference
                            .get("output")
                            .and_then(Value::as_array)
                            .and_then(|output| output.first())
                            .and_then(Value::as_str)
                            == Some("subflow_run_id"))
                        .then(|| reference.get("node").and_then(Value::as_str))
                        .flatten()
                    })
        {
            return Ok(format!("await {task}"));
        }
        let workflow = node
            .parameters
            .get("workflow")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let mut text = format!("await workflow {}", quote(workflow));
        if let Some(key) = node.parameters.get("key").filter(|value| !value.is_null()) {
            text.push_str(&format!(" key {}", self.expr(key)?));
        }
        if let Some(mode) = node.parameters.get("mode").and_then(Value::as_str) {
            text.push_str(&format!(" mode {}", quote(mode)));
        }
        Ok(text)
    }

    fn debounce_text(&self, node: &WorkflowNode) -> Result<String, RexRapError> {
        let name = node
            .parameters
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let delay = node
            .parameters
            .get("delay_seconds")
            .and_then(Value::as_i64)
            .unwrap_or(0);
        let mut text = format!("debounce {} delay {delay}s", quote(name));
        if let Some(key) = node.parameters.get("trigger_key") {
            text.push_str(&format!(" key {}", self.expr(key)?));
        }
        Ok(text)
    }

    fn collect_text(&self, node: &WorkflowNode) -> Result<String, RexRapError> {
        let name = node
            .parameters
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let max = node
            .parameters
            .get("max")
            .and_then(Value::as_i64)
            .unwrap_or(0);
        let text = format!("collect {} max {max}", quote(name));
        Ok(text)
    }

    fn barrier_text(&self, node: &WorkflowNode) -> Result<String, RexRapError> {
        let name = node
            .parameters
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let count = node
            .parameters
            .get("count")
            .and_then(Value::as_i64)
            .unwrap_or(0);
        let mut text = format!("barrier {} count {count}", quote(name));
        if let Some(poll) = node
            .parameters
            .get("poll_interval_seconds")
            .and_then(Value::as_i64)
        {
            text.push_str(&format!(" every {poll}s"));
        }
        Ok(text)
    }

    fn circuit_breaker_text(&self, node: &WorkflowNode) -> Result<String, RexRapError> {
        let name = node
            .parameters
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let threshold = node
            .parameters
            .get("threshold")
            .and_then(Value::as_i64)
            .unwrap_or(0);
        let window = node
            .parameters
            .get("window_seconds")
            .and_then(Value::as_i64)
            .unwrap_or(0);
        let cooldown = node
            .parameters
            .get("cooldown_seconds")
            .and_then(Value::as_i64)
            .unwrap_or(0);
        Ok(format!(
            "circuit_breaker {} threshold {threshold} window {window}s cooldown {cooldown}s",
            quote(name)
        ))
    }

    fn event_source_text(&self, node: &WorkflowNode) -> Result<String, RexRapError> {
        let event_type = node
            .parameters
            .get("event_type")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let mut text = format!("event_source type {}", quote(event_type));
        if let Some(filter) = node.parameters.get("filter") {
            text.push_str(&format!(" filter {}", self.cond(filter)?));
        }
        if let Some(max) = node.parameters.get("max").and_then(Value::as_i64) {
            text.push_str(&format!(" max {max}"));
        }
        Ok(text)
    }

    fn config_text(&self, node: &WorkflowNode) -> Result<String, RexRapError> {
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
    ) -> Result<Option<String>, RexRapError> {
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
        let saved = self.control_vars.get(&node.id);
        let var = saved.map_or_else(|| self.fresh_var(), |vars| vars.item.clone());
        let index_var = saved.and_then(|vars| vars.index.clone());

        let mut binding = var.clone();
        if let Some(item_type) = saved.and_then(|vars| vars.item_type.as_deref()) {
            binding.push_str(&format!(": {item_type}"));
        }
        if let Some(index) = &index_var {
            binding.push_str(&format!(", {index}"));
        }
        let mut header = format!(
            "{}for {binding} in {items_text}",
            self.block_id_prefix(node)
        );
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
    ) -> Result<Option<String>, RexRapError> {
        let branch = node
            .transitions
            .branches
            .first()
            .ok_or_else(|| RexRapError::Decompile("while node has no branch".into()))?;
        let body_entry = branch.target.as_str().to_string();
        let after = node
            .transitions
            .next
            .as_ref()
            .map(|target| target.as_str().to_string());

        let mut header = format!(
            "{}while {}",
            self.block_id_prefix(node),
            self.cond(&branch.when.to_value())?
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
    ) -> Result<Option<String>, RexRapError> {
        let branches = &node.transitions.branches;
        if branches.is_empty() {
            return Err(RexRapError::Decompile(
                "condition node has no branches".into(),
            ));
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
            self.line(&format!(
                "{keyword} {} {{",
                self.cond(&branch.when.to_value())?
            ));
            self.indent += 1;
            self.emit_region(branch.target.as_str(), merge_ref)?;
            self.indent -= 1;
        }

        if let Some(else_target) = &else_target
            && merge_ref != Some(else_target.as_str())
            && !self.end_ids.contains(else_target)
            && !self.visited.contains(else_target)
        {
            self.line("} else {");
            self.indent += 1;
            self.emit_region(else_target, merge_ref)?;
            self.indent -= 1;
        }

        Ok(self.close_block_line("}", merge, stop))
    }

    fn emit_match(
        &mut self,
        node: &WorkflowNode,
        stop: Option<&str>,
    ) -> Result<Option<String>, RexRapError> {
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
                .ok_or_else(|| RexRapError::Decompile("switch case missing target".into()))?;
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
                    return Err(RexRapError::Decompile(
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
        if let Some(default) = &default
            && merge_ref != Some(default.as_str())
            && !self.visited.contains(default)
        {
            self.line("else -> {");
            self.indent += 1;
            self.emit_region(default, merge_ref)?;
            self.indent -= 1;
            self.line("}");
        }
        self.indent -= 1;

        Ok(self.close_block_line("}", merge, stop))
    }

    fn emit_toggle(
        &mut self,
        node: &WorkflowNode,
        stop: Option<&str>,
    ) -> Result<Option<String>, RexRapError> {
        let value = node.parameters.get("value").cloned().unwrap_or(Value::Null);
        let arm_target = |name: &str| {
            node.parameters
                .get(name)
                .and_then(|value| value.get("$node"))
                .and_then(Value::as_str)
                .map(str::to_string)
                .ok_or_else(|| RexRapError::Decompile(format!("toggle missing `{name}` target")))
        };
        let on = arm_target("on")?;
        let off = arm_target("off")?;

        let merge = self
            .find_merge(&[on.clone(), off.clone()])
            .or_else(|| stop.map(str::to_string));
        let merge_ref = merge.as_deref();

        self.line(&format!(
            "{}toggle {} {{",
            self.block_id_prefix(node),
            self.expr(&value)?
        ));
        self.indent += 1;
        for (label, target) in [("on", &on), ("off", &off)] {
            self.line(&format!("{label} -> {{"));
            self.indent += 1;
            self.emit_region(target, merge_ref)?;
            self.indent -= 1;
            self.line("}");
        }
        self.indent -= 1;

        Ok(self.close_block_line("}", merge, stop))
    }

    fn emit_split(
        &mut self,
        node: &WorkflowNode,
        stop: Option<&str>,
    ) -> Result<Option<String>, RexRapError> {
        let key = node.parameters.get("key").cloned().unwrap_or(Value::Null);
        let buckets = node
            .parameters
            .get("buckets")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        let default = node
            .parameters
            .get("default")
            .and_then(|value| value.get("$node"))
            .and_then(Value::as_str)
            .map(str::to_string);

        let mut merge_inputs: Vec<String> = buckets
            .iter()
            .filter_map(|bucket| bucket.pointer("/target/$node").and_then(Value::as_str))
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
            "{}split on {} {{",
            self.block_id_prefix(node),
            self.expr(&key)?
        ));
        self.indent += 1;
        for bucket in &buckets {
            let weight = bucket
                .get("weight")
                .and_then(Value::as_i64)
                .ok_or_else(|| RexRapError::Decompile("percentage bucket missing weight".into()))?;
            let target = bucket
                .pointer("/target/$node")
                .and_then(Value::as_str)
                .ok_or_else(|| RexRapError::Decompile("percentage bucket missing target".into()))?;
            self.line(&format!("{weight}% -> {{"));
            self.indent += 1;
            self.emit_region(target, merge_ref)?;
            self.indent -= 1;
            self.line("}");
        }
        if let Some(default) = &default
            && merge_ref != Some(default.as_str())
            && !self.visited.contains(default)
        {
            self.line("else -> {");
            self.indent += 1;
            self.emit_region(default, merge_ref)?;
            self.indent -= 1;
            self.line("}");
        }
        self.indent -= 1;

        Ok(self.close_block_line("}", merge, stop))
    }

    fn emit_map(
        &mut self,
        node: &WorkflowNode,
        stop: Option<&str>,
    ) -> Result<Option<String>, RexRapError> {
        let body_entry = single_node_id(node.parameters.get("target"));
        let after = node
            .transitions
            .next
            .as_ref()
            .map(|target| target.as_str().to_string());

        let items = node.parameters.get("items").cloned().unwrap_or(Value::Null);
        let items_text = self.expr(&items)?;
        let var = self
            .control_vars
            .get(&node.id)
            .map_or_else(|| self.fresh_var(), |vars| vars.item.clone());

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
    ) -> Result<Option<String>, RexRapError> {
        let branches = node_ref_ids(node.parameters.get("branches"));
        let surface = self
            .parallel_branches
            .get(&node.id)
            .cloned()
            .unwrap_or_default();
        let selected_ids = if let Some(selected) = &surface.selected {
            let selected = selected.iter().map(String::as_str).collect::<HashSet<_>>();
            branches
                .iter()
                .zip(surface.labels.iter())
                .filter(|&(_branch, label)| {
                    label
                        .as_deref()
                        .is_some_and(|label| selected.contains(label))
                })
                .map(|(branch, _label)| branch.clone())
                .collect::<Vec<_>>()
        } else {
            branches.clone()
        };
        let join = self.find_join(&selected_ids).ok_or_else(|| {
            RexRapError::Decompile(format!("parallel '{}' has no matching join", node.id))
        })?;
        let (join_id, mode, cont) = join;

        self.line(&format!("{}parallel {{", self.block_id_prefix(node)));
        self.indent += 1;
        for (index, branch) in branches.iter().enumerate() {
            let label = surface.labels.get(index).and_then(Option::as_deref);
            self.line(&match label {
                Some(label) => format!("branch {} {{", quote(label)),
                None => "branch {".to_string(),
            });
            self.indent += 1;
            let branch_stop = surface
                .stops
                .get(index)
                .map(String::as_str)
                .unwrap_or(join_id.as_str());
            self.emit_region(branch, Some(branch_stop))?;
            self.indent -= 1;
            self.line("}");
        }
        self.indent -= 1;

        let selector = surface
            .selected
            .map(|labels| {
                format!(
                    " [{}]",
                    labels
                        .iter()
                        .map(|label| quote(label))
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            })
            .unwrap_or_default();
        Ok(self.close_block_line(&format!("}} join{selector} {mode}"), cont, stop))
    }

    fn emit_race(
        &mut self,
        node: &WorkflowNode,
        outer_stop: Option<&str>,
    ) -> Result<Option<String>, RexRapError> {
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
    ) -> Result<Option<String>, RexRapError> {
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

    /// find the join node that synchronizes the given parallel branch endpoints.
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
        // a u64 counter cannot be exhausted before memory is, so this always returns without panicking.
        let mut index: u64 = 2;
        loop {
            let candidate = format!("item{index}");
            if !active.contains(&candidate) {
                return candidate;
            }
            index += 1;
        }
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
        for node in distance_maps[0].keys() {
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
            if map.len() == 1
                && let Some(id) = map.get("$node").and_then(Value::as_str)
            {
                out.push(id.to_string());
                return;
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
/// the authored statement list an `invocation` node retains for rendering.
///
/// separate from `parameters.module`, which is the compiled bytecode the vm runs. the two are
/// written together by lowering and must describe the same program; only this one is ever read
/// back into text.
fn invocation_source(node: &WorkflowNode) -> Option<Vec<Value>> {
    node.parameters
        .as_object()?
        .get("source")
        .and_then(Value::as_array)
        .cloned()
}

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

/// true when a mutex node releases its lock (an end-of-section release node) rather than acquiring.
fn mutex_is_release(node: &WorkflowNode) -> bool {
    node.parameters
        .get("release")
        .and_then(Value::as_bool)
        .unwrap_or(false)
}

fn needs_id_annotation(kind: &WorkflowNodeKind) -> bool {
    matches!(
        kind,
        WorkflowNodeKind::Wait
            | WorkflowNodeKind::Output
            | WorkflowNodeKind::Input
            | WorkflowNodeKind::Approval
            | WorkflowNodeKind::Gate
            | WorkflowNodeKind::Signal
            | WorkflowNodeKind::Config
            | WorkflowNodeKind::Assert
            | WorkflowNodeKind::Transform
            | WorkflowNodeKind::Audit
            | WorkflowNodeKind::Checkpoint
            | WorkflowNodeKind::Mutex
            | WorkflowNodeKind::Throttle
            | WorkflowNodeKind::Cooldown
            | WorkflowNodeKind::AwaitRun
            | WorkflowNodeKind::Debounce
            | WorkflowNodeKind::Collect
            | WorkflowNodeKind::Barrier
            | WorkflowNodeKind::CircuitBreaker
            | WorkflowNodeKind::EventSource
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
        WorkflowNodeKind::Toggle => &["toggle"],
        WorkflowNodeKind::Percentage => &["percentage"],
        WorkflowNodeKind::Try => &["try"],
        WorkflowNodeKind::Mutex => &["mutex", "mutex_release"],
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
