// lowers the wdl ast into the existing runinator json workflow model. sequential
// statements imply forward edges; control blocks expand into the matching control nodes.
// the output is a WorkflowDefinition whose `definition` is `{ start, nodes: [...] }`.

mod blocks;
mod expr;
pub(crate) mod types;

use std::collections::HashSet;

use runinator_models::value::{Map, Value};
use runinator_models::workflows::{WorkflowDefinition, WorkflowGraph};

use crate::CompileOptions;
use crate::ast::*;
use crate::errors::WdlError;

/// a binding from a loop/map variable to the node output it reads from.
#[derive(Clone)]
struct VarBinding {
    name: String,
    node_id: String,
    base: Vec<PathSeg>,
}

struct Lowerer {
    nodes: Vec<Value>,
    used_ids: HashSet<String>,
    counter: u64,
    start_id: String,
    end_id: String,
    fail_id: String,
    scope: Vec<VarBinding>,
    // declared `let <id>: <type>` annotations, kept for graph metadata so decompile can
    // re-emit them. each value is the lossless native form of a RuninatorType.
    declared_types: Vec<(String, Value)>,
}

pub fn lower_document(
    document: &Document,
    options: &CompileOptions,
) -> Result<WorkflowDefinition, WdlError> {
    let workflow = &document.workflow;
    let mut lowerer = Lowerer::new();
    let end_id = lowerer.end_id.clone();
    let body_entry = lowerer.lower_block(&workflow.body, &end_id)?;

    // the entry is an explicit `start -> <target>` when present, else the first statement.
    let entry = match &workflow.start {
        Some(target) => lowerer.target_id(target),
        None => body_entry,
    };

    // build the start node pointing at the entry, then append the terminals.
    let start_node = node(
        &lowerer.start_id,
        "start",
        vec![("transitions", transitions_next(&entry))],
    );
    let mut nodes = Vec::with_capacity(lowerer.nodes.len() + 3);
    nodes.push(start_node);
    nodes.append(&mut lowerer.nodes);
    nodes.push(node(&lowerer.end_id, "end", vec![]));
    nodes.push(node(&lowerer.fail_id, "fail", vec![]));

    let mut definition = Map::new();
    definition.insert("start".into(), Value::String(lowerer.start_id.clone()));
    definition.insert("nodes".into(), Value::Array(nodes));
    if !lowerer.declared_types.is_empty() {
        let mut types_map = Map::new();
        for (id, value) in &lowerer.declared_types {
            types_map.insert(id.clone(), value.clone());
        }
        let mut wdl = Map::new();
        wdl.insert("types".into(), Value::Object(types_map));
        let mut metadata = Map::new();
        metadata.insert("wdl".into(), Value::Object(wdl));
        definition.insert("metadata".into(), Value::Object(metadata));
    }
    let graph = WorkflowGraph::from_value(Value::Object(definition)).map_err(WdlError::lower)?;

    let input_type = match &workflow.input {
        Some(type_expr) => types::lower_type(type_expr)?,
        None => Default::default(),
    };

    Ok(WorkflowDefinition {
        id: None,
        name: workflow.name.clone(),
        version: workflow.version.unwrap_or(options.default_version),
        enabled: options.enabled,
        input_type,
        definition: graph,
        created_at: None,
        updated_at: None,
    })
}

impl Lowerer {
    fn new() -> Self {
        let mut used_ids = HashSet::new();
        used_ids.insert("start".to_string());
        used_ids.insert("end".to_string());
        used_ids.insert("fail".to_string());
        Self {
            nodes: Vec::new(),
            used_ids,
            counter: 0,
            start_id: "start".to_string(),
            end_id: "end".to_string(),
            fail_id: "fail".to_string(),
            scope: Vec::new(),
            declared_types: Vec::new(),
        }
    }

    /// record a `let <id>: <type>` annotation for the graph metadata sidecar.
    fn record_declared_type(&mut self, id: &str, stmt: &Stmt) -> Result<(), WdlError> {
        let Some(type_expr) = &stmt.label_type else {
            return Ok(());
        };
        let ty = types::lower_type(type_expr)?;
        let value = serde_json::to_value(&ty)
            .map(Value::from)
            .map_err(|err| WdlError::lower(err.to_string()))?;
        self.declared_types.push((id.to_string(), value));
        Ok(())
    }

    /// lower a sequence of statements, wiring each forward edge to the next statement's
    /// entry (or `cont` after the last). returns the block's entry node id.
    fn lower_block(&mut self, block: &[Stmt], cont: &str) -> Result<String, WdlError> {
        if block.is_empty() {
            return Ok(cont.to_string());
        }
        // pass 1: claim entry ids so forward references resolve.
        let mut entries = Vec::with_capacity(block.len());
        for stmt in block {
            entries.push(self.entry_id_for(stmt)?);
        }
        // pass 2: lower each statement with its concrete continuation.
        for (index, stmt) in block.iter().enumerate() {
            let next = if index + 1 < block.len() {
                entries[index + 1].clone()
            } else {
                cont.to_string()
            };
            self.lower_stmt(stmt, &entries[index], &next)?;
        }
        Ok(entries[0].clone())
    }

    fn entry_id_for(&mut self, stmt: &Stmt) -> Result<String, WdlError> {
        if let Some(id) = &stmt.annotations.id {
            return self.claim(id);
        }
        if let Some(label) = &stmt.label {
            return self.claim(label);
        }
        let prefix = match &stmt.kind {
            StmtKind::Action(_) => "action",
            StmtKind::Subflow(_) => "subflow",
            StmtKind::Wait(_) => "wait",
            StmtKind::Emit(_) => "emit",
            StmtKind::Approval(_) => "approval",
            StmtKind::Config(_) => "config",
            StmtKind::Fail(_) => "fail_node",
            StmtKind::If(_) => "if",
            StmtKind::For(_) => "for_loop",
            StmtKind::While(_) => "while_loop",
            StmtKind::Match(_) => "switch",
            StmtKind::Parallel(_) => "parallel",
            StmtKind::Try(_) => "try",
            StmtKind::Race(_) => "race",
            StmtKind::Map(_) => "map",
        };
        Ok(self.fresh(prefix))
    }

    fn lower_stmt(&mut self, stmt: &Stmt, id: &str, next: &str) -> Result<(), WdlError> {
        match &stmt.kind {
            StmtKind::Action(action) => self.lower_action(action, stmt, id, next),
            StmtKind::Subflow(subflow) => self.lower_subflow(subflow, stmt, id, next),
            StmtKind::Wait(wait) => self.lower_wait(wait, stmt, id, next),
            StmtKind::Emit(emit) => self.lower_emit(emit, stmt, id, next),
            StmtKind::Approval(approval) => self.lower_approval(approval, stmt, id, next),
            StmtKind::Config(config) => self.lower_config(config, stmt, id, next),
            StmtKind::Fail(message) => self.lower_fail(message.as_ref(), stmt, id),
            StmtKind::If(if_stmt) => {
                let cont = self.block_cont(&stmt.transitions, next);
                self.lower_if(if_stmt, id, &cont)
            }
            StmtKind::For(for_stmt) => {
                let cont = self.block_cont(&stmt.transitions, next);
                self.lower_for(for_stmt, id, &cont)
            }
            StmtKind::While(while_stmt) => {
                let cont = self.block_cont(&stmt.transitions, next);
                self.lower_while(while_stmt, id, &cont)
            }
            StmtKind::Match(match_stmt) => {
                let cont = self.block_cont(&stmt.transitions, next);
                self.lower_match(match_stmt, id, &cont)
            }
            StmtKind::Parallel(parallel) => {
                let cont = self.block_cont(&stmt.transitions, next);
                self.lower_parallel(parallel, id, &cont)
            }
            StmtKind::Try(try_stmt) => {
                let cont = self.block_cont(&stmt.transitions, next);
                self.lower_try(try_stmt, id, &cont)
            }
            StmtKind::Race(race) => {
                let cont = self.block_cont(&stmt.transitions, next);
                self.lower_race(race, id, &cont)
            }
            StmtKind::Map(map_stmt) => {
                let cont = self.block_cont(&stmt.transitions, next);
                self.lower_map(map_stmt, id, &cont)
            }
        }
    }

    // leaf statements -------------------------------------------------------

    fn lower_action(
        &mut self,
        action: &ActionStmt,
        stmt: &Stmt,
        id: &str,
        next: &str,
    ) -> Result<(), WdlError> {
        self.record_declared_type(id, stmt)?;
        let mut config = Map::new();
        for (name, value) in &action.args {
            config.insert(name.clone(), self.lower_expr(value)?);
        }
        let mut action_obj = Map::new();
        action_obj.insert("provider".into(), Value::String(action.provider.clone()));
        action_obj.insert("function".into(), Value::String(action.function.clone()));
        action_obj.insert(
            "timeout_seconds".into(),
            Value::from(action.modifiers.timeout_seconds.unwrap_or(60)),
        );
        action_obj.insert("configuration".into(), Value::Object(config));
        if action.modifiers.mcp {
            action_obj.insert("mcp_enabled".into(), Value::Bool(true));
        }
        if !action.modifiers.tags.is_empty() {
            action_obj.insert(
                "tags".into(),
                Value::Array(
                    action
                        .modifiers
                        .tags
                        .iter()
                        .map(|tag| Value::String(tag.clone()))
                        .collect(),
                ),
            );
        }

        let mut fields = vec![
            ("action", Value::Object(action_obj)),
            (
                "transitions",
                self.leaf_transitions(&stmt.transitions, "on_success", next),
            ),
        ];
        self.apply_modifier_fields(&mut fields, &action.modifiers);
        self.apply_skip(&mut fields, stmt);
        self.push(node(id, "action", fields));
        Ok(())
    }

    fn lower_subflow(
        &mut self,
        subflow: &SubflowStmt,
        stmt: &Stmt,
        id: &str,
        next: &str,
    ) -> Result<(), WdlError> {
        self.record_declared_type(id, stmt)?;
        let mut subflow_obj = Map::new();
        subflow_obj.insert(
            "workflow_name".into(),
            Value::String(subflow.workflow_name.clone()),
        );
        subflow_obj.insert(
            "type".into(),
            Value::String(if subflow.detached {
                "fire_and_forget".into()
            } else {
                "wait".into()
            }),
        );
        if subflow.reuse {
            subflow_obj.insert("reuse_open_run".into(), Value::Bool(true));
        }
        if let Some(run_name) = &subflow.run_name {
            subflow_obj.insert("run_name".into(), self.lower_expr(run_name)?);
        }

        let mut params = Map::new();
        for (name, value) in &subflow.params {
            params.insert(name.clone(), self.lower_expr(value)?);
        }

        let mut fields = vec![
            ("subflow", Value::Object(subflow_obj)),
            ("parameters", Value::Object(params)),
            (
                "transitions",
                self.leaf_transitions(&stmt.transitions, "on_success", next),
            ),
        ];
        self.apply_skip(&mut fields, stmt);
        self.push(node(id, "subflow", fields));
        Ok(())
    }

    fn lower_wait(
        &mut self,
        wait: &WaitStmt,
        stmt: &Stmt,
        id: &str,
        next: &str,
    ) -> Result<(), WdlError> {
        let mut wait_obj = Map::new();
        let seconds = match &wait.amount {
            WaitAmount::Seconds(seconds) => Value::from(*seconds),
            WaitAmount::Expr(expr) => self.lower_expr(expr)?,
        };
        wait_obj.insert("seconds".into(), seconds);
        if let Some(status) = &wait.until_status {
            wait_obj.insert("until_status".into(), Value::String(status.clone()));
        }
        if let Some(status) = &wait.initial_status {
            wait_obj.insert("initial_status".into(), Value::String(status.clone()));
        }
        let mut fields = vec![
            ("wait", Value::Object(wait_obj)),
            (
                "transitions",
                self.leaf_transitions(&stmt.transitions, "next", next),
            ),
        ];
        self.apply_skip(&mut fields, stmt);
        self.push(node(id, "wait", fields));
        Ok(())
    }

    fn lower_emit(
        &mut self,
        emit: &EmitStmt,
        stmt: &Stmt,
        id: &str,
        next: &str,
    ) -> Result<(), WdlError> {
        let mut params = Map::new();
        if let Some(event_type) = &emit.event_type {
            params.insert("event_type".into(), Value::String(event_type.clone()));
        }
        let data = match &emit.data {
            Some(data) => self.lower_expr(data)?,
            None => Value::Null,
        };
        params.insert("data".into(), data);
        let mut fields = vec![
            ("parameters", Value::Object(params)),
            (
                "transitions",
                self.leaf_transitions(&stmt.transitions, "next", next),
            ),
        ];
        self.apply_skip(&mut fields, stmt);
        self.push(node(id, "emit", fields));
        Ok(())
    }

    fn lower_approval(
        &mut self,
        approval: &ApprovalStmt,
        stmt: &Stmt,
        id: &str,
        next: &str,
    ) -> Result<(), WdlError> {
        let mut params = Map::new();
        params.insert(
            "approval_type".into(),
            Value::String(
                approval
                    .approval_type
                    .clone()
                    .unwrap_or_else(|| "generic".into()),
            ),
        );
        params.insert("prompt".into(), self.lower_expr(&approval.prompt)?);
        for (name, value) in &approval.metadata {
            params.insert(name.clone(), self.lower_expr(value)?);
        }
        let mut fields = vec![
            ("parameters", Value::Object(params)),
            (
                "transitions",
                self.leaf_transitions(&stmt.transitions, "on_success", next),
            ),
        ];
        self.apply_skip(&mut fields, stmt);
        self.push(node(id, "approval", fields));
        Ok(())
    }

    fn lower_config(
        &mut self,
        config: &ConfigStmt,
        stmt: &Stmt,
        id: &str,
        next: &str,
    ) -> Result<(), WdlError> {
        let mut params = Map::new();
        if let Some(name) = &config.name {
            params.insert("name".into(), self.lower_expr(name)?);
        }
        if let Some(metadata) = &config.metadata {
            params.insert("metadata".into(), self.lower_expr(metadata)?);
        }
        let mut fields = vec![
            ("parameters", Value::Object(params)),
            (
                "transitions",
                self.leaf_transitions(&stmt.transitions, "next", next),
            ),
        ];
        self.apply_skip(&mut fields, stmt);
        self.push(node(id, "config", fields));
        Ok(())
    }

    fn lower_fail(
        &mut self,
        message: Option<&Expr>,
        stmt: &Stmt,
        id: &str,
    ) -> Result<(), WdlError> {
        let mut fields = Vec::new();
        if let Some(message) = message {
            let mut params = Map::new();
            params.insert("message".into(), self.lower_expr(message)?);
            fields.push(("parameters", Value::Object(params)));
        }
        self.apply_skip(&mut fields, stmt);
        self.push(node(id, "fail", fields));
        Ok(())
    }

    // shared helpers --------------------------------------------------------

    fn apply_modifier_fields(
        &self,
        fields: &mut Vec<(&'static str, Value)>,
        modifiers: &Modifiers,
    ) {
        if let Some(retry) = modifiers.retry {
            let mut obj = Map::new();
            obj.insert("max_attempts".into(), Value::from(retry));
            fields.push(("retry", Value::Object(obj)));
        }
        if let Some(reentry) = &modifiers.reentry {
            let mut obj = Map::new();
            obj.insert("enabled".into(), Value::Bool(true));
            obj.insert("max_visits".into(), Value::from(reentry.max_visits));
            if let Some(target) = &reentry.on_exhausted {
                obj.insert("on_exhausted".into(), node_ref(&self.target_id(target)));
            }
            fields.push(("reentry", Value::Object(obj)));
        }
    }

    fn apply_skip(&self, fields: &mut Vec<(&'static str, Value)>, stmt: &Stmt) {
        if stmt.annotations.skip {
            fields.push(("skipped", Value::Bool(true)));
        }
    }

    /// build a transitions object for a leaf step. the happy path uses `primary`
    /// (on_success for actions, next for control-ish leaves) and falls back to `cont`.
    fn leaf_transitions(&self, clause: &TransitionClause, primary: &str, cont: &str) -> Value {
        let mut map = Map::new();
        let success = clause.next.as_ref().or(clause.on_success.as_ref());
        let success_id = match success {
            Some(target) => self.target_id(target),
            None => cont.to_string(),
        };
        map.insert(primary.to_string(), node_ref(&success_id));
        if let Some(target) = &clause.on_failure {
            map.insert("on_failure".into(), node_ref(&self.target_id(target)));
        }
        if let Some(target) = &clause.on_timeout {
            map.insert("on_timeout".into(), node_ref(&self.target_id(target)));
        }
        if let Some(target) = &clause.on_reject {
            map.insert("on_reject".into(), node_ref(&self.target_id(target)));
        }
        Value::Object(map)
    }

    /// the continuation a control block flows into: an explicit forward arrow overrides
    /// the sequential successor.
    fn block_cont(&self, clause: &TransitionClause, cont: &str) -> String {
        match clause.next.as_ref().or(clause.on_success.as_ref()) {
            Some(target) => self.target_id(target),
            None => cont.to_string(),
        }
    }

    fn target_id(&self, target: &Target) -> String {
        match target {
            Target::Done => self.end_id.clone(),
            Target::Fail => self.fail_id.clone(),
            Target::Label(name) => name.clone(),
        }
    }

    fn push(&mut self, node: Value) {
        self.nodes.push(node);
    }

    fn claim(&mut self, id: &str) -> Result<String, WdlError> {
        if !self.used_ids.insert(id.to_string()) {
            return Err(WdlError::lower(format!("duplicate node id '{id}'")));
        }
        Ok(id.to_string())
    }

    fn fresh(&mut self, prefix: &str) -> String {
        loop {
            self.counter += 1;
            let candidate = format!("{prefix}_{}", self.counter);
            if self.used_ids.insert(candidate.clone()) {
                return candidate;
            }
        }
    }
}

// free helpers --------------------------------------------------------------

fn node(id: &str, kind: &str, fields: Vec<(&str, Value)>) -> Value {
    let mut map = Map::new();
    map.insert("id".into(), Value::String(id.to_string()));
    map.insert("kind".into(), Value::String(kind.to_string()));
    for (key, value) in fields {
        map.insert(key.to_string(), value);
    }
    Value::Object(map)
}

fn node_ref(id: &str) -> Value {
    let mut map = Map::new();
    map.insert("$node".into(), Value::String(id.to_string()));
    Value::Object(map)
}

fn transitions_next(target: &str) -> Value {
    let mut map = Map::new();
    map.insert("next".into(), node_ref(target));
    Value::Object(map)
}
