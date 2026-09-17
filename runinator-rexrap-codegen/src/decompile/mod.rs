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

pub fn decompile_definition(
    definition: &WorkflowDefinition,
    options: &DecompileOptions,
) -> Result<String, RexRapError> {
    let graph = &definition.definition;
    if !graph.defs.is_empty() {
        return Err(RexRapError::Decompile(
            "workflow $defs must be expanded before decompiling to REXRAP".into(),
        ));
    }
    if let Some(key) = graph.extra.keys().find(|key| key.as_str() != "ui") {
        return Err(RexRapError::Decompile(format!(
            "workflow definition field '{key}' is not representable in REXRAP"
        )));
    }
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

    let returns = (definition.output_type != runinator_models::types::RuninatorType::Any)
        .then(|| definition.output_type.clone())
        .or_else(|| {
            metadata
                .output_type()
                .filter(|ty| *ty == runinator_models::types::RuninatorType::Any)
        })
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
    let portable_metadata = metadata.portable();
    if !portable_metadata.is_empty() {
        decompiler.line(&format!(
            "metadata {}",
            decompiler.expr(&Value::Object(portable_metadata))?
        ));
    }
    if let Some(ui) = graph.extra.get("ui") {
        decompiler.line(&format!("ui {}", decompiler.expr(ui)?));
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
    empty_decompiler().expr(value)
}

/// Render one lowered compute program using the same inverse mapping as workflow decompilation.
///
/// Invocation nodes retain this statement tree beside their compiled module so authoring clients
/// can show the program people wrote instead of exposing VM bytecode as JSON.
pub fn render_compute_program(program: &[Value]) -> Result<String, RexRapError> {
    empty_decompiler().compute_text(program)
}

fn empty_decompiler() -> Decompiler<'static> {
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

mod decompile_options;
pub use decompile_options::DecompileOptions;

mod decompiler;
pub(super) use decompiler::Decompiler;
