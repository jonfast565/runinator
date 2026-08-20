// lowers a `do { }` block into a `std.run`/`std.exec` action node. the block becomes a
// program array under `action.configuration.program`; the function is `run` when every called
// library function is pure and `exec` when any call is effectful.

use std::collections::HashSet;

use runinator_models::value::{Map, Value};

use runinator_rexrap_syntax::ast::*;
use runinator_rexrap_syntax::errors::RexRapError;

use runinator_compute::{CallableCatalog, assemble_module, parse_program};
use runinator_models::invocation::EffectClass;
use runinator_models::invocation::InvocationModule;

use super::Lowerer;

impl Lowerer {
    pub(super) fn lower_do_fragment(&self, body: &[DoLine]) -> Result<Value, RexRapError> {
        // collect every local name so fragment lowering matches normal compute-node lowering.
        let previous_locals = self.compute_locals.replace(HashSet::new());
        collect_locals(body, &mut self.compute_locals.borrow_mut());
        let program = self.lower_program(body).map(Value::Array);
        self.compute_locals.replace(previous_locals);
        program
    }

    pub(super) fn lower_do(
        &mut self,
        compute: &DoStmt,
        stmt: &Stmt,
        id: &str,
        next: &str,
    ) -> Result<(), RexRapError> {
        self.record_declared_type(id, stmt)?;
        if let Some(foreign) = &compute.foreign {
            return self.lower_foreign_do(foreign, compute, stmt, id, next);
        }
        // collect every local name so bare local paths lower to `let` refs.
        let previous_locals = self.compute_locals.replace(HashSet::new());
        collect_locals(&compute.body, &mut self.compute_locals.borrow_mut());

        let program = self.lower_program(&compute.body)?;
        let result = self.push_invocation_node(&program, compute, stmt, id, next);
        self.compute_locals.replace(previous_locals);
        result
    }

    /// emit an `invocation` node: the assembled module the vm runs, plus the statement tree the
    /// decompiler renders back.
    ///
    /// both, not one. the module is bytecode, and recovering `let`/`if`/`return` from a flat
    /// instruction stream is control-flow reconstruction — a decompiler in the hard sense, which
    /// would have to be exactly right or the editor pane would silently rewrite the author's code.
    /// keeping the source beside the bytecode is the same arrangement `metadata.rexrap.functions`
    /// already uses for function signatures, and it costs a copy of a small json tree.
    fn push_invocation_node(
        &mut self,
        program: &[Value],
        compute: &DoStmt,
        stmt: &Stmt,
        id: &str,
        next: &str,
    ) -> Result<(), RexRapError> {
        let module = self.assemble_module(program)?;
        let mut parameters = Map::new();
        parameters.insert(
            "module".into(),
            serde_json::to_value(&module)
                .map_err(|err| RexRapError::lower(format!("failed to encode the module: {err}")))?
                .into(),
        );
        parameters.insert("source".into(), Value::Array(program.to_vec()));
        if let Some(timeout) = compute.modifiers.timeout_seconds {
            parameters.insert("timeout_seconds".into(), Value::from(timeout));
        }

        let mut fields = vec![
            ("parameters", Value::Object(parameters)),
            (
                "transitions",
                self.leaf_transitions(&stmt.transitions, "on_success", next)?,
            ),
        ];
        self.apply_modifier_fields(&mut fields, &compute.modifiers);
        self.apply_annotations(&mut fields, stmt);
        self.push(super::node(id, "invocation", fields));
        Ok(())
    }

    /// assemble a lowered statement tree, plus every user function it may call, into a module.
    ///
    /// the functions go in whole rather than by reachability analysis: a call can be reached through
    /// a closure or a `$if` branch the assembler never walks, and a module carrying an unused
    /// function costs bytes, while one missing a reachable function fails at run time.
    fn assemble_module(&self, program: &[Value]) -> Result<InvocationModule, RexRapError> {
        let entry = parse_program(&Value::Array(program.to_vec()))
            .map_err(|err| RexRapError::lower(format!("failed to read the program: {err}")))?;
        let mut functions = Vec::new();
        for entry in &self.lowered_functions {
            functions.push(assembled_function(entry)?);
        }
        assemble_module(&entry, &functions, &self.callable_catalog())
            .map_err(|err| RexRapError::lower(format!("failed to assemble the program: {err}")))
    }

    /// the catalog the assembler classifies called names against.
    ///
    /// built from the same three sources the type checker sees — the intrinsic library, the
    /// document's own `fn`s, and the compile options' providers and packaged exports — so a name
    /// that type-checked as a provider action cannot assemble as a module function, or the reverse.
    fn callable_catalog(&self) -> CallableCatalog {
        let mut catalog = CallableCatalog::builtin();
        for entry in &self.lowered_functions {
            if let Some(name) = entry.get("name").and_then(Value::as_str) {
                // arity and effect are the type checker's business, and it has already run: the
                // assembler only needs to know that this name is a module function rather than a
                // provider dispatch.
                catalog.add_local(name, 0, EffectClass::Pure);
            }
        }
        for provider in &self.provider_metadata {
            catalog.add_provider(provider);
        }
        catalog
    }

    fn lower_foreign_do(
        &mut self,
        foreign: &ForeignDo,
        compute: &DoStmt,
        stmt: &Stmt,
        id: &str,
        next: &str,
    ) -> Result<(), RexRapError> {
        let mut config = Map::new();
        config.insert("language".into(), Value::String(foreign.language.clone()));
        config.insert("source".into(), Value::String(foreign.source.clone()));

        let mut action_obj = Map::new();
        action_obj.insert("provider".into(), Value::String("std".into()));
        action_obj.insert("function".into(), Value::String("code".into()));
        action_obj.insert(
            "timeout_seconds".into(),
            Value::from(compute.modifiers.timeout_seconds.unwrap_or(60)),
        );
        action_obj.insert("configuration".into(), Value::Object(config));

        let mut fields = vec![
            ("action", Value::Object(action_obj)),
            (
                "transitions",
                self.leaf_transitions(&stmt.transitions, "on_success", next)?,
            ),
        ];
        self.apply_modifier_fields(&mut fields, &compute.modifiers);
        self.apply_annotations(&mut fields, stmt);
        self.push(super::node(id, "action", fields));
        Ok(())
    }

    /// lower a function block body into the same `$let`/`$return`/`$if` program array a `do`
    /// block produces. the caller has already registered the function parameters as compute locals;
    /// this adds the block's own `let`/lambda locals so bare references lower to `let` refs.
    pub(super) fn lower_fn_block(&self, body: &[DoLine]) -> Result<Vec<Value>, RexRapError> {
        collect_locals(body, &mut self.compute_locals.borrow_mut());
        self.lower_program(body)
    }

    fn lower_program(&self, body: &[DoLine]) -> Result<Vec<Value>, RexRapError> {
        body.iter().map(|line| self.lower_do_line(line)).collect()
    }

    fn lower_do_line(&self, line: &DoLine) -> Result<Value, RexRapError> {
        match line {
            DoLine::Let { name, value, .. } => {
                let mut map = Map::new();
                map.insert("$let".into(), Value::String(name.clone()));
                map.insert("value".into(), self.lower_expr(value)?);
                Ok(Value::Object(map))
            }
            DoLine::Return(expr) => {
                let mut map = Map::new();
                map.insert("$return".into(), self.lower_expr(expr)?);
                Ok(Value::Object(map))
            }
            DoLine::Goto(target) => {
                let mut map = Map::new();
                map.insert("$goto".into(), Value::String(self.target_id(target)));
                Ok(Value::Object(map))
            }
            DoLine::If {
                cond,
                then_branch,
                else_branch,
            } => {
                let mut map = Map::new();
                map.insert("$if".into(), self.lower_cond(cond)?);
                map.insert(
                    "then".into(),
                    Value::Array(self.lower_program(then_branch)?),
                );
                map.insert(
                    "else".into(),
                    Value::Array(self.lower_program(else_branch)?),
                );
                Ok(Value::Object(map))
            }
            DoLine::Expr(expr) => self.lower_expr(expr),
        }
    }
}

/// read one lowered `metadata.functions` entry into the form the assembler takes.
///
/// a function body is lowered as either a single expression (`body`) or a statement list
/// (`program`). the assembler only takes a program, so an expression body becomes a one-statement
/// `return`, which is what it means.
fn assembled_function(
    entry: &Value,
) -> Result<
    (
        String,
        Vec<String>,
        runinator_models::workflow_ast::ComputeProgram,
        Option<u32>,
    ),
    RexRapError,
> {
    let name = entry
        .get("name")
        .and_then(Value::as_str)
        .ok_or_else(|| RexRapError::lower("a lowered function has no name"))?
        .to_string();
    let params = entry
        .get("params")
        .and_then(Value::as_array)
        .map(|params| {
            params
                .iter()
                .filter_map(|param| param.get("name").and_then(Value::as_str))
                .map(str::to_string)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let program = match (entry.get("program"), entry.get("body")) {
        (Some(program), _) => program.clone(),
        (None, Some(body)) => Value::Array(vec![Value::Object(Map::from_iter([(
            "$return".into(),
            body.clone(),
        )]))]),
        (None, None) => {
            return Err(RexRapError::lower(format!("function '{name}' has no body")));
        }
    };
    let body = parse_program(&program)
        .map_err(|err| RexRapError::lower(format!("failed to read the body of '{name}': {err}")))?;
    let max_depth = entry
        .get("recursive")
        .and_then(|recursive| recursive.get("max_depth"))
        .and_then(Value::as_i64)
        .map(|depth| depth as u32);
    Ok((name, params, body, max_depth))
}

/// collect every `let` name and lambda parameter declared anywhere in the block (including nested
/// `if` branches), so bare references to them lower to `let` refs.
fn collect_locals(body: &[DoLine], out: &mut HashSet<String>) {
    for line in body {
        match line {
            DoLine::Let { name, value, .. } => {
                out.insert(name.clone());
                collect_locals_expr(value, out);
            }
            DoLine::Return(expr) | DoLine::Expr(expr) => collect_locals_expr(expr, out),
            DoLine::If {
                cond,
                then_branch,
                else_branch,
            } => {
                collect_locals_cond(cond, out);
                collect_locals(then_branch, out);
                collect_locals(else_branch, out);
            }
            DoLine::Goto(_) => {}
        }
    }
}

/// gather lambda parameter names from an expression tree.
fn collect_locals_expr(expr: &Expr, out: &mut HashSet<String>) {
    match &expr.kind {
        ExprKind::Lambda { params, body } => {
            for param in params {
                out.insert(param.clone());
            }
            collect_locals_expr(body, out);
        }
        ExprKind::Call { args, .. } => {
            for arg in args {
                collect_locals_expr(arg, out);
            }
        }
        ExprKind::Array(items) => {
            for item in items {
                collect_locals_expr(item, out);
            }
        }
        ExprKind::Object(entries) => {
            for (_, value) in entries {
                collect_locals_expr(value, out);
            }
        }
        ExprKind::Concat(parts)
        | ExprKind::Coalesce(parts)
        | ExprKind::Add(parts)
        | ExprKind::Sub(parts)
        | ExprKind::Mul(parts)
        | ExprKind::Div(parts)
        | ExprKind::Mod(parts) => {
            for part in parts {
                collect_locals_expr(part, out);
            }
        }
        ExprKind::Neg(inner) | ExprKind::ToString(inner) | ExprKind::ToJson(inner) => {
            collect_locals_expr(inner, out);
        }
        ExprKind::Str(parts) => {
            for part in parts {
                if let StrPart::Expr(inner) = part {
                    collect_locals_expr(inner, out);
                }
            }
        }
        ExprKind::Apply { callee, args } => {
            collect_locals_expr(callee, out);
            for arg in args {
                collect_locals_expr(arg, out);
            }
        }
        ExprKind::Cast { expr, .. } => collect_locals_expr(expr, out),
        _ => {}
    }
}

/// gather lambda parameter names from a compute-tier condition.
fn collect_locals_cond(cond: &Cond, out: &mut HashSet<String>) {
    match &cond.kind {
        CondKind::All(parts) | CondKind::Any(parts) => {
            for part in parts {
                collect_locals_cond(part, out);
            }
        }
        CondKind::Not(inner) => collect_locals_cond(inner, out),
        CondKind::Expr(expr) => collect_locals_expr(expr, out),
        CondKind::Exists(expr) => collect_locals_expr(expr, out),
        CondKind::Cmp { left, right, .. } => {
            collect_locals_expr(left, out);
            collect_locals_expr(right, out);
        }
    }
}
