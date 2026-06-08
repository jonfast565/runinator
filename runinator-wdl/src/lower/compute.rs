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
    pub(super) fn lower_compute(
        &mut self,
        compute: &ComputeStmt,
        stmt: &Stmt,
        id: &str,
        next: &str,
    ) -> Result<(), WdlError> {
        self.record_declared_type(id, stmt)?;
        // collect every local name so bare local paths lower to `let` refs.
        let previous_locals = std::mem::take(&mut self.compute_locals);
        collect_locals(&compute.body, &mut self.compute_locals);

        let program = self.lower_program(&compute.body)?;
        let function = if block_is_effectful(&compute.body) {
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

        self.compute_locals = previous_locals;
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

/// collect every `let` name declared anywhere in the block (including nested `if` branches).
fn collect_locals(body: &[ComputeLine], out: &mut HashSet<String>) {
    for line in body {
        match line {
            ComputeLine::Let { name, .. } => {
                out.insert(name.clone());
            }
            ComputeLine::If {
                then_branch,
                else_branch,
                ..
            } => {
                collect_locals(then_branch, out);
                collect_locals(else_branch, out);
            }
            _ => {}
        }
    }
}
