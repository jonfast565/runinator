// reconstructs wdl source from a WorkflowDefinition. it walks the graph from the start
// node, emitting linear sequences and recovering for/if/match blocks. control shapes it
// cannot yet structure (parallel/join/try/race/map) surface as a clear error so callers
// know the definition needs manual porting.

mod expr;

use std::collections::{HashMap, HashSet, VecDeque};

use runinator_models::types::RuninatorType;
use runinator_models::value::Value;
use runinator_models::workflows::{
    WorkflowDefinition, WorkflowNode, WorkflowNodeKind, WorkflowTransitions, WorkflowWaitSeconds,
};

use crate::errors::WdlError;

pub(super) struct Decompiler<'a> {
    nodes: HashMap<String, &'a WorkflowNode>,
    end_ids: HashSet<String>,
    fail_ids: HashSet<String>,
    loop_vars: Vec<(String, String)>,
    // declared `let <id>: <type>` annotations recovered from graph metadata.
    declared_types: HashMap<String, RuninatorType>,
    out: String,
    indent: usize,
}

pub fn decompile_definition(definition: &WorkflowDefinition) -> Result<String, WdlError> {
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

    let mut decompiler = Decompiler {
        nodes,
        end_ids,
        fail_ids,
        loop_vars: Vec::new(),
        declared_types,
        out: String::new(),
        indent: 0,
    };

    decompiler.line(&format!(
        "workflow {} v{} {{",
        quote(&definition.name),
        definition.version
    ));
    decompiler.indent += 1;
    decompiler.emit_input(&definition.input_type);

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
        decompiler.emit_region(&entry, None)?;
    }

    decompiler.indent -= 1;
    decompiler.line("}");
    Ok(decompiler.out)
}

/// recover declared `let` types from the graph metadata sidecar at `/wdl/types`.
fn read_declared_types(metadata: &Value) -> HashMap<String, RuninatorType> {
    let mut types = HashMap::new();
    let Some(entries) = metadata.pointer("/wdl/types").and_then(Value::as_object) else {
        return types;
    };
    for (id, value) in entries {
        let json = serde_json::Value::from(value.clone());
        if let Ok(ty) = serde_json::from_value::<RuninatorType>(json) {
            types.insert(id.clone(), ty);
        }
    }
    types
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

    fn emit_input(&mut self, input_type: &RuninatorType) {
        let RuninatorType::Struct { fields, .. } = input_type else {
            return;
        };
        if fields.is_empty() {
            return;
        }
        self.line("input {");
        self.indent += 1;
        for (name, field) in fields {
            let mark = if field.required { "" } else { "?" };
            let rendered = expr::render_type(&field.ty);
            self.line(&format!("{name}{mark}: {rendered}"));
        }
        self.indent -= 1;
        self.line("}");
        self.out.push('\n');
    }

    /// emit statements from `cur` until reaching `stop`, a terminal, or a dead end.
    fn emit_region(&mut self, cur: &str, stop: Option<&str>) -> Result<(), WdlError> {
        let mut cur = cur.to_string();
        loop {
            if stop == Some(cur.as_str()) {
                break;
            }
            if self.end_ids.contains(&cur) {
                break;
            }
            let node = match self.nodes.get(cur.as_str()) {
                Some(node) => *node,
                None => break,
            };
            match &node.kind {
                WorkflowNodeKind::Loop => {
                    let after = self.emit_loop(node)?;
                    match after {
                        Some(next) => cur = next,
                        None => break,
                    }
                }
                WorkflowNodeKind::Condition => {
                    let merge = self.emit_if(node, stop)?;
                    match merge {
                        Some(next) => cur = next,
                        None => break,
                    }
                }
                WorkflowNodeKind::Switch => {
                    let merge = self.emit_match(node, stop)?;
                    match merge {
                        Some(next) => cur = next,
                        None => break,
                    }
                }
                WorkflowNodeKind::Fail => {
                    self.line("fail");
                    break;
                }
                WorkflowNodeKind::Action
                | WorkflowNodeKind::Subflow
                | WorkflowNodeKind::Wait
                | WorkflowNodeKind::Emit
                | WorkflowNodeKind::Approval
                | WorkflowNodeKind::Config => {
                    let success = self.emit_leaf(node)?;
                    match success {
                        Some(next) if !self.is_terminal(&next) && stop != Some(next.as_str()) => {
                            cur = next;
                        }
                        _ => break,
                    }
                }
                WorkflowNodeKind::Map => {
                    let after = self.emit_map(node)?;
                    match after {
                        Some(next) => cur = next,
                        None => break,
                    }
                }
                WorkflowNodeKind::Parallel => {
                    let after = self.emit_parallel(node)?;
                    match after {
                        Some(next) => cur = next,
                        None => break,
                    }
                }
                WorkflowNodeKind::Race => {
                    let after = self.emit_race(node)?;
                    match after {
                        Some(next) => cur = next,
                        None => break,
                    }
                }
                WorkflowNodeKind::Try => {
                    let after = self.emit_try(node)?;
                    match after {
                        Some(next) => cur = next,
                        None => break,
                    }
                }
                // a join is consumed by its parallel; if reached directly, pass through.
                WorkflowNodeKind::Join => match node.transitions.next.as_ref() {
                    Some(target) => cur = target.as_str().to_string(),
                    None => break,
                },
                WorkflowNodeKind::Start | WorkflowNodeKind::End => break,
            }
        }
        Ok(())
    }

    fn is_terminal(&self, id: &str) -> bool {
        self.end_ids.contains(id) || self.fail_ids.contains(id)
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

    // leaf statements -------------------------------------------------------

    /// emit a single leaf statement with its outcome arrows. returns the success target.
    fn emit_leaf(&mut self, node: &WorkflowNode) -> Result<Option<String>, WdlError> {
        let (text, lets_binding) = self.statement_text(node)?;
        let prefix = if lets_binding {
            match self.declared_types.get(&node.id) {
                Some(ty) => format!("let {}: {} = ", node.id, expr::render_type(ty)),
                None => format!("let {} = ", node.id),
            }
        } else if needs_id_annotation(&node.kind) {
            format!("@id({}) ", quote(&node.id))
        } else {
            String::new()
        };

        let transitions = &node.transitions;
        let success = transitions
            .on_success
            .as_ref()
            .or(transitions.next.as_ref())
            .map(|target| target.as_str().to_string());

        // collect failure-style arrows; the success arrow is only explicit when it jumps
        // to a terminal (otherwise it is the linear successor we keep emitting).
        let mut arrows: Vec<(String, String)> = Vec::new();
        if let Some(target) = &transitions.on_failure {
            arrows.push(("fail".into(), self.target_label(target.as_str())));
        }
        if let Some(target) = &transitions.on_timeout {
            arrows.push(("timeout".into(), self.target_label(target.as_str())));
        }
        if let Some(target) = &transitions.on_reject {
            arrows.push(("reject".into(), self.target_label(target.as_str())));
        }
        let success_terminal = success
            .as_deref()
            .map(|id| self.is_terminal(id))
            .unwrap_or(false);

        if success_terminal && arrows.is_empty() {
            let label = self.target_label(success.as_deref().unwrap());
            self.line(&format!("{prefix}{text} -> {label}"));
        } else if arrows.is_empty() {
            self.line(&format!("{prefix}{text}"));
        } else {
            self.line(&format!("{prefix}{text}"));
            self.indent += 1;
            if success_terminal {
                let label = self.target_label(success.as_deref().unwrap());
                self.line(&format!("ok -> {label}"));
            }
            for (outcome, label) in &arrows {
                self.line(&format!("{outcome} -> {label}"));
            }
            self.indent -= 1;
        }

        Ok(success)
    }

    /// returns the statement text and whether it should be prefixed with `let <id> =`.
    fn statement_text(&self, node: &WorkflowNode) -> Result<(String, bool), WdlError> {
        match &node.kind {
            WorkflowNodeKind::Action => Ok((self.action_text(node)?, true)),
            WorkflowNodeKind::Subflow => Ok((self.subflow_text(node)?, true)),
            WorkflowNodeKind::Wait => Ok((self.wait_text(node), false)),
            WorkflowNodeKind::Emit => Ok((self.emit_text(node)?, false)),
            WorkflowNodeKind::Approval => Ok((self.approval_text(node)?, false)),
            WorkflowNodeKind::Config => Ok((self.config_text(node)?, false)),
            other => Err(WdlError::Decompile(format!("unexpected leaf {other:?}"))),
        }
    }

    fn action_text(&self, node: &WorkflowNode) -> Result<String, WdlError> {
        let action = node
            .action
            .as_ref()
            .ok_or_else(|| WdlError::Decompile("action node missing action".into()))?;
        let mut args = Vec::new();
        if let Value::Object(config) = action.configuration.as_value() {
            for (name, value) in config {
                args.push(format!("{name}: {}", self.expr(value)?));
            }
        }
        let mut text = format!(
            "{}.{}({})",
            action.provider,
            action.function,
            args.join(", ")
        );
        if action.timeout_seconds != 60 {
            text.push_str(&format!(".timeout({}s)", action.timeout_seconds));
        }
        if node.retry.max_attempts > 1 {
            text.push_str(&format!(".retry({})", node.retry.max_attempts));
        }
        if !action.tags.is_empty() {
            let tags = action
                .tags
                .iter()
                .map(|tag| quote(tag))
                .collect::<Vec<_>>()
                .join(", ");
            text.push_str(&format!(".tags({tags})"));
        }
        if action.mcp_enabled {
            text.push_str(".mcp()");
        }
        if node.reentry.enabled {
            text.push_str(&format!(".reentry({})", node.reentry.max_visits));
        }
        Ok(text)
    }

    fn subflow_text(&self, node: &WorkflowNode) -> Result<String, WdlError> {
        let subflow = &node.subflow;
        let name = subflow.workflow_name.clone().unwrap_or_default();
        let verb = match subflow.subflow_type {
            runinator_models::workflows::WorkflowSubflowType::FireAndForget => "spawn",
            runinator_models::workflows::WorkflowSubflowType::Wait => "call",
        };
        let mut text = format!("{verb} {}", quote(&name));
        if subflow.reuse_open_run {
            text.push_str(" reuse");
        }
        if let Some(run_name) = &subflow.run_name {
            text.push_str(&format!(" as {}", self.expr(run_name)?));
        }
        if let Value::Object(params) = node.parameters.as_value() {
            if !params.is_empty() {
                let mut parts = Vec::new();
                for (name, value) in params {
                    parts.push(format!("{name}: {}", self.expr(value)?));
                }
                text.push_str(&format!(" with {{ {} }}", parts.join(", ")));
            }
        }
        Ok(text)
    }

    fn wait_text(&self, node: &WorkflowNode) -> String {
        let seconds = match &node.wait.seconds {
            Some(WorkflowWaitSeconds::Integer(seconds)) => *seconds,
            _ => 0,
        };
        let mut text = format!("wait {seconds}s");
        if let Some(status) = &node.wait.until_status {
            text.push_str(&format!(" until {}", quote(status)));
        }
        if let Some(status) = &node.wait.initial_status {
            text.push_str(&format!(" initial {}", quote(status)));
        }
        text
    }

    fn emit_text(&self, node: &WorkflowNode) -> Result<String, WdlError> {
        let mut text = "emit".to_string();
        if let Some(event_type) = node.parameters.get("event_type").and_then(Value::as_str) {
            text.push_str(&format!(" {}", quote(event_type)));
        }
        match node.parameters.get("data") {
            Some(Value::Object(_)) | None | Some(Value::Null) => {
                let data = node.parameters.get("data");
                if let Some(Value::Object(_)) = data {
                    text.push_str(&format!(" {}", self.expr(data.unwrap())?));
                } else {
                    text.push_str(" {}");
                }
            }
            Some(other) => {
                // non-object data cannot be written with the `emit { }` form.
                return Err(WdlError::Decompile(format!(
                    "emit data must be an object to decompile, found {other}"
                )));
            }
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
        if let Some(kind) = node.parameters.get("approval_type").and_then(Value::as_str) {
            if kind != "generic" {
                text.push_str(&format!(" type {}", quote(kind)));
            }
        }
        if let Value::Object(params) = node.parameters.as_value() {
            let mut parts = Vec::new();
            for (name, value) in params {
                if name == "prompt" || name == "approval_type" {
                    continue;
                }
                parts.push(format!("{name}: {}", self.expr(value)?));
            }
            if !parts.is_empty() {
                text.push_str(&format!(" {{ {} }}", parts.join(", ")));
            }
        }
        Ok(text)
    }

    fn config_text(&self, node: &WorkflowNode) -> Result<String, WdlError> {
        if let Some(name) = node.parameters.get("name") {
            return Ok(format!("set name = {}", self.expr(name)?));
        }
        if let Some(metadata) = node.parameters.get("metadata") {
            return Ok(format!("set meta {}", self.expr(metadata)?));
        }
        Ok("set meta {}".to_string())
    }

    // control blocks --------------------------------------------------------

    fn emit_loop(&mut self, node: &WorkflowNode) -> Result<Option<String>, WdlError> {
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

        let mut header = format!("for {var} in {items_text}");
        if let Some(limit) = node.max_iterations {
            header.push_str(&format!(" limit {limit}"));
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
        self.line("}");

        Ok(after)
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
            let keyword = if index == 0 { "if" } else { "} else if" };
            self.line(&format!("{keyword} {} {{", self.cond(&branch.when)?));
            self.indent += 1;
            self.emit_region(branch.target.as_str(), merge_ref)?;
            self.indent -= 1;
        }

        if let Some(else_target) = &else_target {
            if merge_ref != Some(else_target.as_str()) && !self.end_ids.contains(else_target) {
                self.line("} else {");
                self.indent += 1;
                self.emit_region(else_target, merge_ref)?;
                self.indent -= 1;
            }
        }
        self.line("}");

        Ok(merge)
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

        self.line(&format!("match {} {{", self.expr(&value)?));
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
                return Err(WdlError::Decompile(
                    "switch case missing equals/when".into(),
                ));
            };
            self.line(&format!("{head} -> {{"));
            self.indent += 1;
            self.emit_region(target, merge_ref)?;
            self.indent -= 1;
            self.line("}");
        }
        if let Some(default) = &default {
            if merge_ref != Some(default.as_str()) {
                self.line("else -> {");
                self.indent += 1;
                self.emit_region(default, merge_ref)?;
                self.indent -= 1;
                self.line("}");
            }
        }
        self.indent -= 1;
        self.line("}");

        Ok(merge)
    }

    fn emit_map(&mut self, node: &WorkflowNode) -> Result<Option<String>, WdlError> {
        let body_entry = single_node_id(node.parameters.get("target"));
        let after = node
            .transitions
            .next
            .as_ref()
            .map(|target| target.as_str().to_string());

        let items = node.parameters.get("items").cloned().unwrap_or(Value::Null);
        let items_text = self.expr(&items)?;
        let var = self.fresh_var();

        let mut header = format!("map {var} in {items_text}");
        if let Some(concurrency) = node.parameters.get("concurrency").and_then(Value::as_i64) {
            header.push_str(&format!(" concurrency {concurrency}"));
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
        self.line("}");

        Ok(after)
    }

    fn emit_parallel(&mut self, node: &WorkflowNode) -> Result<Option<String>, WdlError> {
        let branches = node_ref_ids(node.parameters.get("branches"));
        let join = self.find_join(&branches).ok_or_else(|| {
            WdlError::Decompile(format!("parallel '{}' has no matching join", node.id))
        })?;
        let (join_id, mode, cont) = join;

        self.line("parallel {");
        self.indent += 1;
        for branch in &branches {
            self.line("branch {");
            self.indent += 1;
            self.emit_region(branch, Some(join_id.as_str()))?;
            self.indent -= 1;
            self.line("}");
        }
        self.indent -= 1;
        self.line(&format!("}} join {mode}"));

        Ok(cont)
    }

    fn emit_race(&mut self, node: &WorkflowNode) -> Result<Option<String>, WdlError> {
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
        let stop = cont.as_deref();

        self.line(&format!("race winner {winner} {{"));
        self.indent += 1;
        for branch in &branches {
            self.line("branch {");
            self.indent += 1;
            self.emit_region(branch, stop)?;
            self.indent -= 1;
            self.line("}");
        }
        self.indent -= 1;
        self.line("}");

        Ok(cont)
    }

    fn emit_try(&mut self, node: &WorkflowNode) -> Result<Option<String>, WdlError> {
        let body = single_node_id(node.parameters.get("body"));
        let catch = single_node_id(node.parameters.get("catch"));
        let finally = single_node_id(node.parameters.get("finally"));
        let cont = node
            .transitions
            .next
            .as_ref()
            .map(|target| target.as_str().to_string());
        let stop = cont.as_deref();

        self.line("try {");
        self.indent += 1;
        if let Some(body) = &body {
            self.emit_region(body, stop)?;
        }
        self.indent -= 1;
        if let Some(catch) = &catch {
            self.line("} catch {");
            self.indent += 1;
            self.emit_region(catch, stop)?;
            self.indent -= 1;
        }
        if let Some(finally) = &finally {
            self.line("} finally {");
            self.indent += 1;
            self.emit_region(finally, stop)?;
            self.indent -= 1;
        }
        self.line("}");

        Ok(cont)
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
            | WorkflowNodeKind::Emit
            | WorkflowNodeKind::Approval
            | WorkflowNodeKind::Config
    )
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
