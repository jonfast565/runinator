// lowers a `compute { }` block into a `std.run`/`std.exec` action node. the block becomes a
// program array under `action.configuration.program`; the function is `run` when every called
// library function is pure and `exec` when any call is effectful.

use std::collections::HashSet;

use runinator_models::value::{Map, Value};

use crate::ast::*;
use crate::errors::WdlError;
use crate::purity::block_is_effectful;

use super::Lowerer;

impl Lowerer {
    pub(super) fn lower_compute_fragment(&self, body: &[ComputeLine]) -> Result<Value, WdlError> {
        // collect every local name so fragment lowering matches normal compute-node lowering.
        let previous_locals = self.compute_locals.replace(HashSet::new());
        collect_locals(body, &mut self.compute_locals.borrow_mut());
        let program = self.lower_program(body).map(Value::Array);
        self.compute_locals.replace(previous_locals);
        program
    }

    pub(super) fn lower_compute(
        &mut self,
        compute: &ComputeStmt,
        stmt: &Stmt,
        id: &str,
        next: &str,
    ) -> Result<(), WdlError> {
        self.record_declared_type(id, stmt)?;
        // collect every local name so bare local paths lower to `let` refs.
        let previous_locals = self.compute_locals.replace(HashSet::new());
        collect_locals(&compute.body, &mut self.compute_locals.borrow_mut());

        let program = self.lower_program(&compute.body)?;
        let function = if block_is_effectful(&compute.body, &self.registry) {
            "exec"
        } else {
            "run"
        };

        let mut config = Map::new();
        config.insert("program".into(), Value::Array(program));
        let mut action_obj = Map::new();
        action_obj.insert("provider".into(), Value::String("std".into()));
        action_obj.insert("function".into(), Value::String(function.into()));
        action_obj.insert(
            "timeout_seconds".into(),
            Value::from(compute.modifiers.timeout_seconds.unwrap_or(60)),
        );
        action_obj.insert("configuration".into(), Value::Object(config));

        let mut fields = vec![
            ("action", Value::Object(action_obj)),
            (
                "transitions",
                self.leaf_transitions(&stmt.transitions, "on_success", next),
            ),
        ];
        self.apply_modifier_fields(&mut fields, &compute.modifiers);
        self.apply_annotations(&mut fields, stmt);
        self.push(super::node(id, "action", fields));

        self.compute_locals.replace(previous_locals);
        Ok(())
    }

    fn lower_program(&self, body: &[ComputeLine]) -> Result<Vec<Value>, WdlError> {
        body.iter()
            .map(|line| self.lower_compute_line(line))
            .collect()
    }

    fn lower_compute_line(&self, line: &ComputeLine) -> Result<Value, WdlError> {
        match line {
            ComputeLine::Let { name, value, .. } => {
                let mut map = Map::new();
                map.insert("$let".into(), Value::String(name.clone()));
                map.insert("value".into(), self.lower_expr(value)?);
                Ok(Value::Object(map))
            }
            ComputeLine::Return(expr) => {
                let mut map = Map::new();
                map.insert("$return".into(), self.lower_expr(expr)?);
                Ok(Value::Object(map))
            }
            ComputeLine::Goto(target) => {
                let mut map = Map::new();
                map.insert("$goto".into(), Value::String(self.target_id(target)));
                Ok(Value::Object(map))
            }
            ComputeLine::If {
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
            ComputeLine::Expr(expr) => self.lower_expr(expr),
        }
    }
}

/// collect every `let` name and lambda parameter declared anywhere in the block (including nested
/// `if` branches), so bare references to them lower to `let` refs.
fn collect_locals(body: &[ComputeLine], out: &mut HashSet<String>) {
    for line in body {
        match line {
            ComputeLine::Let { name, value, .. } => {
                out.insert(name.clone());
                collect_locals_expr(value, out);
            }
            ComputeLine::Return(expr) | ComputeLine::Expr(expr) => collect_locals_expr(expr, out),
            ComputeLine::If {
                cond,
                then_branch,
                else_branch,
            } => {
                collect_locals_cond(cond, out);
                collect_locals(then_branch, out);
                collect_locals(else_branch, out);
            }
            ComputeLine::Goto(_) => {}
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
