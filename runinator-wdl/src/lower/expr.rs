// lowers wdl expressions and conditions into the json forms the runtime understands:
// `$ref`/`$concat`/`$coalesce`/`$to_string`/`$to_json_string` for expressions and the
// `{value, <op>}` / `{all|any|not}` shape for conditions.

use runinator_models::value::{Map, Value};

use crate::ast::*;
use crate::errors::WdlError;

use super::{Lowerer, VarBinding};

impl Lowerer {
    pub(super) fn push_scope(&mut self, name: &str, node_id: &str, base: Vec<PathSeg>) {
        self.scope.push(VarBinding {
            name: name.to_string(),
            node_id: node_id.to_string(),
            base,
        });
    }

    pub(super) fn pop_scope(&mut self) {
        self.scope.pop();
    }

    pub(super) fn lower_expr(&self, expr: &Expr) -> Result<Value, WdlError> {
        match &expr.kind {
            ExprKind::Null => Ok(Value::Null),
            ExprKind::Bool(value) => Ok(Value::Bool(*value)),
            ExprKind::Int(value) => Ok(Value::from(*value)),
            ExprKind::Float(value) => Ok(Value::from(*value)),
            ExprKind::Str(parts) => self.lower_string(parts),
            ExprKind::Path(segs) => self.lower_path(segs),
            ExprKind::Array(items) => {
                let items = items
                    .iter()
                    .map(|item| self.lower_expr(item))
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(Value::Array(items))
            }
            ExprKind::Object(entries) => {
                let mut map = Map::new();
                for (key, value) in entries {
                    map.insert(key.clone(), self.lower_expr(value)?);
                }
                Ok(Value::Object(map))
            }
            ExprKind::Concat(parts) => self.wrap_array("$concat", parts),
            ExprKind::Coalesce(parts) => self.wrap_array("$coalesce", parts),
            ExprKind::ToString(inner) => self.wrap_unary("$to_string", inner),
            ExprKind::ToJson(inner) => self.wrap_unary("$to_json_string", inner),
        }
    }

    fn lower_string(&self, parts: &[StrPart]) -> Result<Value, WdlError> {
        // a single literal collapses to a plain string; interpolation becomes $concat.
        if parts.iter().all(|part| matches!(part, StrPart::Lit(_))) {
            let mut text = String::new();
            for part in parts {
                if let StrPart::Lit(lit) = part {
                    text.push_str(lit);
                }
            }
            return Ok(Value::String(text));
        }
        let mut items = Vec::new();
        for part in parts {
            match part {
                StrPart::Lit(lit) => {
                    if !lit.is_empty() {
                        items.push(Value::String(lit.clone()));
                    }
                }
                StrPart::Expr(expr) => items.push(self.lower_expr(expr)?),
            }
        }
        Ok(single_key("$concat", Value::Array(items)))
    }

    fn wrap_array(&self, key: &str, parts: &[Expr]) -> Result<Value, WdlError> {
        let items = parts
            .iter()
            .map(|part| self.lower_expr(part))
            .collect::<Result<Vec<_>, _>>()?;
        Ok(single_key(key, Value::Array(items)))
    }

    fn wrap_unary(&self, key: &str, inner: &Expr) -> Result<Value, WdlError> {
        Ok(single_key(key, self.lower_expr(inner)?))
    }

    fn lower_path(&self, segs: &[PathSeg]) -> Result<Value, WdlError> {
        let head = match segs.first() {
            Some(PathSeg::Key(name)) => name.clone(),
            _ => return Err(WdlError::lower("path must start with an identifier")),
        };
        let rest = &segs[1..];

        // loop/map variables remap to the controlling node's `item` output.
        if let Some(binding) = self.scope.iter().rev().find(|b| b.name == head) {
            let mut path = binding.base.clone();
            path.extend_from_slice(rest);
            return Ok(node_ref_expr(&binding.node_id, &path));
        }

        match head.as_str() {
            "input" => Ok(scoped_ref("input", rest)),
            "prev" => Ok(scoped_ref("prev", rest)),
            "run" => Ok(scoped_ref("workflow", rest)),
            // config resolves eagerly in the web service, so it is a plain ref.
            "config" => Ok(scoped_ref("config", rest)),
            // secrets resolve late at the worker; lower to the `secret://scope/name` form it
            // already understands. the whole value must be a single secret (no interpolation).
            "secret" => lower_secret(rest),
            // any other head is treated as a reference to that node's output.
            _ => Ok(node_ref_expr(&head, rest)),
        }
    }

    pub(super) fn lower_cond(&self, cond: &Cond) -> Result<Value, WdlError> {
        match &cond.kind {
            CondKind::All(parts) => {
                let items = parts
                    .iter()
                    .map(|part| self.lower_cond(part))
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(single_key("all", Value::Array(items)))
            }
            CondKind::Any(parts) => {
                let items = parts
                    .iter()
                    .map(|part| self.lower_cond(part))
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(single_key("any", Value::Array(items)))
            }
            CondKind::Not(inner) => Ok(single_key("not", self.lower_cond(inner)?)),
            CondKind::Exists(expr) => {
                let mut map = Map::new();
                map.insert("value".into(), self.lower_expr(expr)?);
                map.insert("exists".into(), Value::Bool(true));
                Ok(Value::Object(map))
            }
            CondKind::Cmp { left, op, right } => {
                let mut map = Map::new();
                map.insert("value".into(), self.lower_expr(left)?);
                map.insert(cmp_key(*op).into(), self.lower_expr(right)?);
                Ok(Value::Object(map))
            }
        }
    }
}

/// lower `secret.<scope>.<name…>` to the `secret://<scope>/<name>` string the worker resolves.
/// requires at least a scope and a name; extra segments join into the name with `/`.
fn lower_secret(rest: &[PathSeg]) -> Result<Value, WdlError> {
    let parts = rest
        .iter()
        .map(|seg| match seg {
            PathSeg::Key(key) => key.clone(),
            PathSeg::Index(index) => index.to_string(),
        })
        .collect::<Vec<_>>();
    if parts.len() < 2 {
        return Err(WdlError::lower(
            "secret reference must be `secret.<scope>.<name>`",
        ));
    }
    let scope = &parts[0];
    let name = parts[1..].join("/");
    Ok(Value::String(format!("secret://{scope}/{name}")))
}

fn cmp_key(op: CmpOp) -> &'static str {
    match op {
        CmpOp::Eq => "equals",
        CmpOp::Ne => "not_equals",
        CmpOp::Gt => "greater_than",
        CmpOp::Ge => "greater_than_or_equal",
        CmpOp::Lt => "less_than",
        CmpOp::Le => "less_than_or_equal",
        CmpOp::Contains => "contains",
        CmpOp::In => "in",
        CmpOp::StartsWith => "starts_with",
        CmpOp::EndsWith => "ends_with",
    }
}

fn path_array(segs: &[PathSeg]) -> Value {
    Value::Array(
        segs.iter()
            .map(|seg| match seg {
                PathSeg::Key(key) => Value::String(key.clone()),
                PathSeg::Index(index) => Value::from(*index),
            })
            .collect(),
    )
}

/// build `{ "$ref": { "<source>": [path...] } }`.
fn scoped_ref(source: &str, segs: &[PathSeg]) -> Value {
    let mut inner = Map::new();
    inner.insert(source.to_string(), path_array(segs));
    single_key("$ref", Value::Object(inner))
}

/// build `{ "$ref": { "node": "<id>", "output": [path...] } }`.
fn node_ref_expr(node_id: &str, segs: &[PathSeg]) -> Value {
    let mut inner = Map::new();
    inner.insert("node".into(), Value::String(node_id.to_string()));
    inner.insert("output".into(), path_array(segs));
    single_key("$ref", Value::Object(inner))
}

fn single_key(key: &str, value: Value) -> Value {
    let mut map = Map::new();
    map.insert(key.to_string(), value);
    Value::Object(map)
}
