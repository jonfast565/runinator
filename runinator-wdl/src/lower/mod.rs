// lowers the wdl ast into the existing runinator json workflow model. sequential
// statements imply forward edges; control blocks expand into the matching control nodes.
// the output is a WorkflowDefinition whose `definition` is `{ start, nodes: [...] }`.

mod blocks;
mod compute;
mod expr;
mod spreads;
pub(crate) mod types;

use std::collections::HashSet;
use std::path::PathBuf;

use runinator_models::value::{Map, Value};
use runinator_models::workflows::{WorkflowDefinition, WorkflowGraph};

use crate::CompileOptions;
use crate::ast::*;
use crate::desugar::AliasTable;
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
    // header alias declarations, used to expand `...alias` spreads while lowering.
    aliases: AliasTable,
    // per-node `...alias` spread recipes (node id -> recipe segments), kept for graph metadata so
    // decompile can resugar the spreads. empty for spread-free workflows.
    spreads: Map,
    // in-scope local names (compute-block `let`s and lambda params), so a bare local path lowers to
    // a `let` ref. interior-mutable because `lower_expr` (`&self`) scopes a lambda's params while
    // lowering its body, whether the lambda sits in a compute block or inline in any expression.
    compute_locals: std::cell::RefCell<HashSet<String>>,
    // resolved `type <Name>` declarations, consulted when lowering named type references.
    named_types: std::collections::BTreeMap<String, runinator_models::types::RuninatorType>,
    // base directory used for compile-time `file("...")` text includes.
    source_dir: Option<PathBuf>,
    // the callable registry (intrinsics + user functions), used to resolve keyword arguments.
    registry: crate::registry::FunctionRegistry,
}

pub fn lower_document(
    document: &Document,
    options: &CompileOptions,
) -> Result<WorkflowDefinition, WdlError> {
    let workflow = &document.workflow;
    let mut lowerer = Lowerer::new();
    lowerer.source_dir = options.source_dir.clone();
    // the callable registry resolves keyword args in both the workflow body and function bodies.
    lowerer.registry = crate::registry::FunctionRegistry::build(&document.functions);
    // collect the header aliases so spreads can be expanded (graph) and recorded (sidecar) while
    // lowering, where node ids are available to key the recipes.
    lowerer.aliases = crate::desugar::collect_aliases(&workflow.aliases)?;
    // resolve named `type <Name>` declarations so they can be referenced by parameter/let types.
    lowerer.resolve_type_decls(&workflow.type_decls)?;
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

    // the header alias declarations, encoded as recipe segments so decompile can re-emit them.
    let mut alias_meta = Vec::with_capacity(workflow.aliases.len());
    for alias in &workflow.aliases {
        let segs = lowerer.entry_segs(&alias.entries)?;
        let mut entry = Map::new();
        entry.insert("name".into(), Value::String(alias.name.clone()));
        entry.insert("segs".into(), Value::Array(segs));
        alias_meta.push(Value::Object(entry));
    }

    let mut definition = Map::new();
    definition.insert("start".into(), Value::String(lowerer.start_id.clone()));
    definition.insert("nodes".into(), Value::Array(nodes));
    // the `wdl` sidecar carries render-only hints (declared types, alias declarations, and
    // per-node spread recipes) that let decompile reproduce the original source; the runtime
    // ignores it.
    let mut wdl = Map::new();
    if !lowerer.declared_types.is_empty() {
        let mut types_map = Map::new();
        for (id, value) in &lowerer.declared_types {
            types_map.insert(id.clone(), value.clone());
        }
        wdl.insert("types".into(), Value::Object(types_map));
    }
    // named `type <Name>` declarations, recorded as name-preserving surface strings so a
    // declaration that references another declared type keeps that name on decompile.
    if !workflow.type_decls.is_empty() {
        let mut decls = Map::new();
        for decl in &workflow.type_decls {
            // validate the declaration resolves before recording its surface form.
            lowerer.lower_named_type(&decl.ty)?;
            decls.insert(
                decl.name.clone(),
                Value::String(crate::format::format_type(&decl.ty)),
            );
        }
        wdl.insert("type_decls".into(), Value::Object(decls));
    }
    // surface-form overrides for top-level workflow parameter fields whose type references a
    // declared name, so `params { cart: Cart }` decompiles back to the name instead of the
    // expanded struct shape.
    if let Some(TypeExpr::Struct { fields, .. }) = &workflow.input {
        let mut overrides = Map::new();
        for field in fields {
            if type_expr_uses_declared_name(&field.ty, &lowerer.named_types) {
                overrides.insert(
                    field.name.clone(),
                    Value::String(crate::format::format_type(&field.ty)),
                );
            }
        }
        if !overrides.is_empty() {
            wdl.insert("input_types".into(), Value::Object(overrides));
        }
    }
    if !alias_meta.is_empty() {
        wdl.insert("aliases".into(), Value::Array(alias_meta));
    }
    if !lowerer.spreads.is_empty() {
        wdl.insert("spreads".into(), Value::Object(lowerer.spreads.clone()));
    }
    // header `trigger cron` declarations, carried as runtime data the web service materializes on
    // import (unlike the render-only `wdl` sidecar).
    let triggers = lowerer.lower_triggers(&workflow.triggers)?;
    // user `fn` definitions, lowered to runtime-evaluable expression bodies the engine calls.
    let functions = lowerer.lower_functions(&document.functions)?;
    let mut metadata = Map::new();
    if !wdl.is_empty() {
        metadata.insert("wdl".into(), Value::Object(wdl));
    }
    if !triggers.is_empty() {
        metadata.insert("triggers".into(), Value::Array(triggers));
    }
    if !functions.is_empty() {
        metadata.insert("functions".into(), Value::Array(functions));
    }
    if !metadata.is_empty() {
        definition.insert("metadata".into(), Value::Object(metadata));
    }
    let graph = WorkflowGraph::from_value(Value::Object(definition)).map_err(WdlError::lower)?;

    let input_type = match &workflow.input {
        Some(type_expr) => lowerer.lower_input_type(type_expr)?,
        None => Default::default(),
    };

    Ok(WorkflowDefinition {
        id: None,
        name: workflow.name.clone(),
        namespace: workflow.namespace.clone(),
        version: workflow.version.unwrap_or(options.default_version),
        enabled: options.enabled,
        input_type,
        definition: graph,
        created_at: None,
        updated_at: None,
    })
}

pub(crate) fn lower_expression_fragment(
    expr: &Expr,
    options: &CompileOptions,
) -> Result<Value, WdlError> {
    let mut lowerer = Lowerer::new();
    lowerer.source_dir = options.source_dir.clone();
    lowerer.lower_expr(expr)
}

pub(crate) fn lower_condition_fragment(
    cond: &Cond,
    options: &CompileOptions,
) -> Result<Value, WdlError> {
    let mut lowerer = Lowerer::new();
    lowerer.source_dir = options.source_dir.clone();
    lowerer.lower_cond(cond)
}

pub(crate) fn lower_compute_fragment(
    body: &[ComputeLine],
    options: &CompileOptions,
) -> Result<Value, WdlError> {
    let mut lowerer = Lowerer::new();
    lowerer.source_dir = options.source_dir.clone();
    lowerer.lower_compute_fragment(body)
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
            aliases: AliasTable::new(),
            spreads: Map::new(),
            compute_locals: std::cell::RefCell::new(HashSet::new()),
            named_types: std::collections::BTreeMap::new(),
            source_dir: None,
            registry: crate::registry::FunctionRegistry::build(&[]),
        }
    }

    /// lower the top-level workflow `params { }` type, attaching each field's default expression.
    /// defaults only exist on top-level parameter fields; nested struct fields go through plain
    /// type lowering.
    fn lower_input_type(
        &self,
        type_expr: &TypeExpr,
    ) -> Result<runinator_models::types::RuninatorType, WdlError> {
        use runinator_models::types::{RuninatorField, RuninatorType};
        let TypeExpr::Struct { fields, additional } = type_expr else {
            return types::lower_type_with(type_expr, &self.named_types);
        };
        let mut mapped = std::collections::BTreeMap::new();
        for field in fields {
            let ty = types::lower_type_with(&field.ty, &self.named_types)?;
            let mut runinator_field = if field.optional {
                RuninatorField::optional(ty)
            } else {
                RuninatorField::required(ty)
            };
            if let Some(default) = &field.default {
                runinator_field = runinator_field.with_default(self.lower_expr(default)?);
            }
            mapped.insert(field.name.clone(), runinator_field);
        }
        let additional = additional
            .as_ref()
            .map(|ty| types::lower_type_with(ty, &self.named_types))
            .transpose()?
            .map(Box::new);
        Ok(RuninatorType::Struct {
            fields: mapped,
            additional,
        })
    }

    /// resolve `type <Name>` declarations into RuninatorType, rejecting cycles and duplicates.
    fn resolve_type_decls(&mut self, decls: &[TypeDecl]) -> Result<(), WdlError> {
        self.named_types = types::resolve_named_types(decls)?;
        Ok(())
    }

    /// lower a declared type body using the resolved name table.
    fn lower_named_type(
        &self,
        type_expr: &TypeExpr,
    ) -> Result<runinator_models::types::RuninatorType, WdlError> {
        types::lower_type_with(type_expr, &self.named_types)
    }

    /// lower header `trigger cron "..."` declarations into runtime trigger specs
    /// (`[{ cron, parameters, enabled }]`). the cron expression must be a string literal.
    fn lower_triggers(&self, triggers: &[TriggerDecl]) -> Result<Vec<Value>, WdlError> {
        let mut specs = Vec::with_capacity(triggers.len());
        for trigger in triggers {
            let Value::String(cron) = self.lower_expr(&trigger.schedule)? else {
                return Err(WdlError::lower(
                    "trigger cron expression must be a string literal",
                ));
            };
            let parameters = match &trigger.params {
                Some(params) => self.lower_expr(params)?,
                None => Value::Object(Map::new()),
            };
            let mut spec = Map::new();
            spec.insert("cron".into(), Value::String(cron));
            spec.insert("parameters".into(), parameters);
            spec.insert("enabled".into(), Value::Bool(trigger.enabled));
            if let Some(start) = &trigger.blackout_start {
                let Value::String(start) = self.lower_expr(start)? else {
                    return Err(WdlError::lower(
                        "trigger blackout start must be a string literal",
                    ));
                };
                spec.insert("blackout_start".into(), Value::String(start));
            }
            if let Some(end) = &trigger.blackout_end {
                let Value::String(end) = self.lower_expr(end)? else {
                    return Err(WdlError::lower(
                        "trigger blackout end must be a string literal",
                    ));
                };
                spec.insert("blackout_end".into(), Value::String(end));
            }
            specs.push(Value::Object(spec));
        }
        Ok(specs)
    }

    /// lower user `fn` definitions into the `metadata.functions` runtime form:
    /// `[{ name, params: [{name}], body: <expr>, recursive?: { max_depth } }]`. each body lowers
    /// with its parameters registered as locals, so param references become `let` refs the engine
    /// binds at call time.
    fn lower_functions(&self, functions: &[FunctionDef]) -> Result<Vec<Value>, WdlError> {
        let mut out = Vec::with_capacity(functions.len());
        for def in functions {
            let added: Vec<String> = def
                .params
                .iter()
                .map(|param| param.name.clone())
                .filter(|name| self.compute_locals.borrow_mut().insert(name.clone()))
                .collect();
            let body = self.lower_expr(&def.body);
            for name in &added {
                self.compute_locals.borrow_mut().remove(name);
            }
            let body = body?;
            let params = def
                .params
                .iter()
                .map(|param| {
                    Value::Object(Map::from_iter([(
                        "name".into(),
                        Value::String(param.name.clone()),
                    )]))
                })
                .collect();
            let mut entry = Map::new();
            entry.insert("name".into(), Value::String(def.name.clone()));
            entry.insert("params".into(), Value::Array(params));
            entry.insert("body".into(), body);
            if let Some(max_depth) = def.recursive {
                entry.insert(
                    "recursive".into(),
                    Value::Object(Map::from_iter([(
                        "max_depth".into(),
                        Value::from(max_depth as i64),
                    )])),
                );
            }
            out.push(Value::Object(entry));
        }
        Ok(out)
    }

    /// record a `let <id>: <type>` annotation for the graph metadata sidecar.
    fn record_declared_type(&mut self, id: &str, stmt: &Stmt) -> Result<(), WdlError> {
        let Some(type_expr) = &stmt.label_type else {
            return Ok(());
        };
        // validate the annotation resolves, but record its name-preserving surface form so a
        // declared `type` reference (e.g. `let x: Cart`) decompiles back to the name, not its shape.
        types::lower_type_with(type_expr, &self.named_types)?;
        let rendered = crate::format::format_type(type_expr);
        self.declared_types
            .push((id.to_string(), Value::String(rendered)));
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
            StmtKind::Compute(_) => "compute",
            StmtKind::Subflow(_) => "subflow",
            StmtKind::Wait(_) => "wait",
            StmtKind::Output(_) => "output",
            StmtKind::Deliverable(_) => "deliverable",
            StmtKind::Input(_) => "input",
            StmtKind::Approval(_) => "approval",
            StmtKind::Gate(_) => "gate",
            StmtKind::Signal(_) => "signal",
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
            StmtKind::Compute(compute) => self.lower_compute(compute, stmt, id, next),
            StmtKind::Subflow(subflow) => self.lower_subflow(subflow, stmt, id, next),
            StmtKind::Wait(wait) => self.lower_wait(wait, stmt, id, next),
            StmtKind::Output(output) => self.lower_output(output, stmt, id, next),
            StmtKind::Deliverable(deliverable) => {
                self.lower_deliverable(deliverable, stmt, id, next)
            }
            StmtKind::Input(input) => self.lower_input(input, stmt, id, next),
            StmtKind::Approval(approval) => self.lower_approval(approval, stmt, id, next),
            StmtKind::Gate(gate) => self.lower_gate(gate, stmt, id, next),
            StmtKind::Signal(signal) => self.lower_signal(signal, stmt, id, next),
            StmtKind::Config(config) => self.lower_config(config, stmt, id, next),
            StmtKind::Fail(message) => self.lower_fail(message.as_ref(), stmt, id),
            StmtKind::If(if_stmt) => {
                let cont = self.block_cont(&stmt.transitions, next);
                self.lower_if(if_stmt, stmt, id, &cont)
            }
            StmtKind::For(for_stmt) => {
                let cont = self.block_cont(&stmt.transitions, next);
                self.lower_for(for_stmt, stmt, id, &cont)
            }
            StmtKind::While(while_stmt) => {
                let cont = self.block_cont(&stmt.transitions, next);
                self.lower_while(while_stmt, stmt, id, &cont)
            }
            StmtKind::Match(match_stmt) => {
                let cont = self.block_cont(&stmt.transitions, next);
                self.lower_match(match_stmt, stmt, id, &cont)
            }
            StmtKind::Parallel(parallel) => {
                let cont = self.block_cont(&stmt.transitions, next);
                self.lower_parallel(parallel, stmt, id, &cont)
            }
            StmtKind::Try(try_stmt) => {
                let cont = self.block_cont(&stmt.transitions, next);
                self.lower_try(try_stmt, stmt, id, &cont)
            }
            StmtKind::Race(race) => {
                let cont = self.block_cont(&stmt.transitions, next);
                self.lower_race(race, stmt, id, &cont)
            }
            StmtKind::Map(map_stmt) => {
                let cont = self.block_cont(&stmt.transitions, next);
                self.lower_map(map_stmt, stmt, id, &cont)
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
        // expand `...alias` spreads for the graph, and record the authored form for resugaring.
        let flat = crate::desugar::flatten_entries(&action.args, &self.aliases)?;
        let mut config = Map::new();
        for (name, value) in &flat {
            config.insert(name.clone(), self.lower_expr(value)?);
        }
        self.record_spreads(id, &action.args)?;
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
                self.leaf_transitions(&stmt.transitions, "on_success", next)?,
            ),
        ];
        self.apply_modifier_fields(&mut fields, &action.modifiers);
        self.apply_annotations(&mut fields, stmt);
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

        let flat = crate::desugar::flatten_entries(&subflow.params, &self.aliases)?;
        let mut params = Map::new();
        for (name, value) in &flat {
            params.insert(name.clone(), self.lower_expr(value)?);
        }
        self.record_spreads(id, &subflow.params)?;

        let mut fields = vec![
            ("subflow", Value::Object(subflow_obj)),
            ("parameters", Value::Object(params)),
            (
                "transitions",
                self.leaf_transitions(&stmt.transitions, "on_success", next)?,
            ),
        ];
        self.apply_annotations(&mut fields, stmt);
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
                self.leaf_transitions(&stmt.transitions, "next", next)?,
            ),
        ];
        self.apply_annotations(&mut fields, stmt);
        self.push(node(id, "wait", fields));
        Ok(())
    }

    fn lower_output(
        &mut self,
        output: &OutputStmt,
        stmt: &Stmt,
        id: &str,
        next: &str,
    ) -> Result<(), WdlError> {
        let mut params = Map::new();
        if let Some(event_type) = &output.event_type {
            params.insert("event_type".into(), Value::String(event_type.clone()));
        }
        let data = match &output.data {
            Some(data) => self.lower_expr(data)?,
            None => Value::Null,
        };
        params.insert("data".into(), data);
        let mut fields = vec![
            ("parameters", Value::Object(params)),
            (
                "transitions",
                self.leaf_transitions(&stmt.transitions, "next", next)?,
            ),
        ];
        self.apply_annotations(&mut fields, stmt);
        self.push(node(id, "output", fields));
        Ok(())
    }

    fn lower_deliverable(
        &mut self,
        deliverable: &DeliverableStmt,
        stmt: &Stmt,
        id: &str,
        next: &str,
    ) -> Result<(), WdlError> {
        let mut items = Vec::with_capacity(deliverable.items.len());
        for (name, source) in &deliverable.items {
            let mut entry = Map::new();
            entry.insert("name".into(), Value::String(name.clone()));
            entry.insert("source".into(), self.lower_expr(source)?);
            items.push(Value::Object(entry));
        }
        let mut params = Map::new();
        params.insert("items".into(), Value::Array(items));
        let mut fields = vec![
            ("parameters", Value::Object(params)),
            (
                "transitions",
                self.leaf_transitions(&stmt.transitions, "next", next)?,
            ),
        ];
        self.apply_annotations(&mut fields, stmt);
        self.push(node(id, "deliverable", fields));
        Ok(())
    }

    fn lower_input(
        &mut self,
        input: &InputStmt,
        stmt: &Stmt,
        id: &str,
        next: &str,
    ) -> Result<(), WdlError> {
        let mut params = Map::new();
        if let Some(prompt) = &input.prompt {
            params.insert("prompt".into(), self.lower_expr(prompt)?);
        }
        let mut fields = vec![
            ("parameters", Value::Object(params)),
            (
                "transitions",
                self.leaf_transitions(&stmt.transitions, "next", next)?,
            ),
        ];
        self.apply_annotations(&mut fields, stmt);
        self.push(node(id, "input", fields));
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
        let flat = crate::desugar::flatten_entries(&approval.metadata, &self.aliases)?;
        for (name, value) in &flat {
            params.insert(name.clone(), self.lower_expr(value)?);
        }
        self.record_spreads(id, &approval.metadata)?;
        let mut fields = vec![
            ("parameters", Value::Object(params)),
            (
                "transitions",
                self.leaf_transitions(&stmt.transitions, "on_success", next)?,
            ),
        ];
        self.apply_annotations(&mut fields, stmt);
        self.push(node(id, "approval", fields));
        Ok(())
    }

    fn lower_gate(
        &mut self,
        gate: &GateStmt,
        stmt: &Stmt,
        id: &str,
        next: &str,
    ) -> Result<(), WdlError> {
        let mut params = Map::new();
        params.insert("kind".into(), Value::String(gate.kind.clone()));
        if let Some(when) = &gate.when {
            params.insert("when".into(), self.lower_cond(when)?);
        }
        if let Some(poll) = gate.poll_interval {
            params.insert("poll_interval".into(), Value::from(poll));
        }
        if let Some(timeout) = gate.timeout {
            params.insert("timeout".into(), Value::from(timeout));
        }
        let flat = crate::desugar::flatten_entries(&gate.metadata, &self.aliases)?;
        for (name, value) in &flat {
            params.insert(name.clone(), self.lower_expr(value)?);
        }
        self.record_spreads(id, &gate.metadata)?;
        let mut fields = vec![
            ("parameters", Value::Object(params)),
            (
                "transitions",
                self.leaf_transitions(&stmt.transitions, "on_success", next)?,
            ),
        ];
        self.apply_annotations(&mut fields, stmt);
        self.push(node(id, "gate", fields));
        Ok(())
    }

    fn lower_signal(
        &mut self,
        signal: &SignalStmt,
        stmt: &Stmt,
        id: &str,
        next: &str,
    ) -> Result<(), WdlError> {
        let mut params = Map::new();
        params.insert("name".into(), Value::String(signal.name.clone()));
        let flat = crate::desugar::flatten_entries(&signal.metadata, &self.aliases)?;
        for (name, value) in &flat {
            params.insert(name.clone(), self.lower_expr(value)?);
        }
        self.record_spreads(id, &signal.metadata)?;
        let mut fields = vec![
            ("parameters", Value::Object(params)),
            (
                "transitions",
                self.leaf_transitions(&stmt.transitions, "on_success", next)?,
            ),
        ];
        self.apply_annotations(&mut fields, stmt);
        self.push(node(id, "signal", fields));
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
                self.leaf_transitions(&stmt.transitions, "next", next)?,
            ),
        ];
        self.apply_annotations(&mut fields, stmt);
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
        self.apply_annotations(&mut fields, stmt);
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

    pub(super) fn apply_annotations(&self, fields: &mut Vec<(&'static str, Value)>, stmt: &Stmt) {
        if stmt.annotations.skip {
            fields.push(("skipped", Value::Bool(true)));
        }
        if stmt.annotations.locked {
            fields.push(("locked", Value::Bool(true)));
        }
        if let Some(timeout) = stmt.annotations.timeout_seconds {
            fields.push(("timeout_seconds", Value::from(timeout)));
        }
    }

    /// build a transitions object for a leaf step. the happy path uses `primary`
    /// (on_success for actions, next for control-ish leaves) and falls back to `cont`.
    fn leaf_transitions(
        &self,
        clause: &TransitionClause,
        primary: &str,
        cont: &str,
    ) -> Result<Value, WdlError> {
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
        if !clause.branches.is_empty() {
            let mut branches = Vec::with_capacity(clause.branches.len());
            for edge in &clause.branches {
                let mut branch = Map::new();
                branch.insert("when".into(), self.lower_cond(&edge.when)?);
                branch.insert("target".into(), node_ref(&self.target_id(&edge.target)));
                if let Some(priority) = edge.priority {
                    branch.insert("priority".into(), Value::from(priority));
                }
                branches.push(Value::Object(branch));
            }
            map.insert("branches".into(), Value::Array(branches));
        }
        Ok(Value::Object(map))
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

/// whether a type expression references any declared (`type <Name>`) type, anywhere in its shape.
fn type_expr_uses_declared_name(
    ty: &TypeExpr,
    named: &std::collections::BTreeMap<String, runinator_models::types::RuninatorType>,
) -> bool {
    match ty {
        TypeExpr::Named(name) => named.contains_key(name),
        TypeExpr::Array(inner) | TypeExpr::Map(inner) => type_expr_uses_declared_name(inner, named),
        TypeExpr::Union(variants) => variants
            .iter()
            .any(|variant| type_expr_uses_declared_name(variant, named)),
        TypeExpr::Struct { fields, additional } => {
            fields
                .iter()
                .any(|field| type_expr_uses_declared_name(&field.ty, named))
                || additional
                    .as_ref()
                    .is_some_and(|a| type_expr_uses_declared_name(a, named))
        }
    }
}

fn transitions_next(target: &str) -> Value {
    let mut map = Map::new();
    map.insert("next".into(), node_ref(target));
    Value::Object(map)
}
