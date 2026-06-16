// lowers wdl expressions and conditions into the json forms the runtime understands:
// `$ref`/`$concat`/`$coalesce`/`$to_string`/`$to_json_string` for expressions and the
// `{value, <op>}` / `{all|any|not}` shape for conditions.

use std::path::{Component, Path};

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
            ExprKind::FileInclude { path } => self.lower_file_include(expr, path),
            ExprKind::DirInclude {
                path,
                recursive,
                max_depth,
            } => self.lower_dir_include(expr, path, *recursive, *max_depth),
            ExprKind::InlineCode { content, .. } => Ok(Value::String(content.clone())),
            ExprKind::Path(segs) => self.lower_path(segs),
            ExprKind::Array(items) => {
                let items = items
                    .iter()
                    .map(|item| self.lower_expr(item))
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(Value::Array(items))
            }
            ExprKind::Object(entries) => {
                // expand any `...alias` spreads nested inside this object literal before lowering.
                let flat = crate::desugar::flatten_entries(entries, &self.aliases)?;
                let mut map = Map::new();
                for (key, value) in &flat {
                    map.insert(key.clone(), self.lower_expr(value)?);
                }
                Ok(Value::Object(map))
            }
            ExprKind::Concat(parts) => self.wrap_array("$concat", parts),
            ExprKind::Coalesce(parts) => self.wrap_array("$coalesce", parts),
            ExprKind::ToString(inner) => self.wrap_unary("$to_string", inner),
            ExprKind::ToJson(inner) => self.wrap_unary("$to_json_string", inner),
            ExprKind::Add(parts) => self.wrap_array("$add", parts),
            ExprKind::Sub(parts) => self.wrap_array("$sub", parts),
            ExprKind::Mul(parts) => self.wrap_array("$mul", parts),
            ExprKind::Div(parts) => self.wrap_array("$div", parts),
            ExprKind::Mod(parts) => self.wrap_array("$mod", parts),
            ExprKind::Neg(inner) => self.wrap_unary("$neg", inner),
            ExprKind::Compare { op, left, right } => {
                let args = vec![self.lower_expr(left)?, self.lower_expr(right)?];
                let mut map = Map::new();
                map.insert("$call".into(), Value::String(op.intrinsic().to_string()));
                map.insert("args".into(), Value::Array(args));
                Ok(Value::Object(map))
            }
            ExprKind::Ternary { cond, then, els } => {
                let mut map = Map::new();
                map.insert("$if".into(), self.lower_expr(cond)?);
                map.insert("then".into(), self.lower_expr(then)?);
                map.insert("else".into(), self.lower_expr(els)?);
                Ok(Value::Object(map))
            }
            ExprKind::Call {
                name, args, named, ..
            } => {
                // resolve keyword args into positional order and fill user-function defaults.
                let positional = self
                    .registry
                    .resolve_args(name, args, named)
                    .map_err(|err| WdlError::semantic(expr.span, err))?;
                let args = positional
                    .iter()
                    .map(|arg| self.lower_expr(arg))
                    .collect::<Result<Vec<_>, _>>()?;
                let mut map = Map::new();
                map.insert("$call".into(), Value::String(name.clone()));
                map.insert("args".into(), Value::Array(args));
                Ok(Value::Object(map))
            }
            // a lambda lowers to `{ "$lambda": { "params": [...], "body": <body> } }`. its params are
            // registered as locals while lowering the body so body paths become `let` refs. params
            // a compute block already pre-collected stay (insert returns false); only freshly added
            // ones — the inline-lambda case — are removed afterward, keeping scope correct either way.
            ExprKind::Lambda { params, body } => {
                let added: Vec<String> = params
                    .iter()
                    .filter(|p| self.compute_locals.borrow_mut().insert((*p).clone()))
                    .cloned()
                    .collect();
                let body_value = self.lower_expr(body);
                for name in &added {
                    self.compute_locals.borrow_mut().remove(name);
                }
                let mut spec = Map::new();
                spec.insert(
                    "params".into(),
                    Value::Array(params.iter().map(|p| Value::String(p.clone())).collect()),
                );
                spec.insert("body".into(), body_value?);
                Ok(single_key("$lambda", Value::Object(spec)))
            }
            // spreads are expanded by desugaring before lowering; one reaching here is a bug.
            ExprKind::Spread(name) => {
                Err(WdlError::lower(format!("unexpanded spread '...{name}'")))
            }
        }
    }

    fn lower_file_include(&self, expr: &Expr, include_path: &str) -> Result<Value, WdlError> {
        let Some(source_dir) = &self.source_dir else {
            return Err(WdlError::semantic(
                expr.span,
                "file() requires a source directory; compile from a .wdl file or pack source",
            ));
        };
        let relative = Path::new(include_path);
        if relative.as_os_str().is_empty() {
            return Err(WdlError::semantic(expr.span, "file() path cannot be empty"));
        }
        if !is_safe_relative_path(relative) {
            return Err(WdlError::semantic(
                expr.span,
                "file() path must be relative and cannot contain '..'",
            ));
        }
        let path = source_dir.join(relative);
        let text = std::fs::read_to_string(&path).map_err(|err| {
            WdlError::semantic(
                expr.span,
                format!("failed to read included file {}: {err}", path.display()),
            )
        })?;
        Ok(Value::String(text))
    }

    fn lower_dir_include(
        &self,
        expr: &Expr,
        include_path: &str,
        recursive: bool,
        max_depth: Option<usize>,
    ) -> Result<Value, WdlError> {
        let Some(source_dir) = &self.source_dir else {
            return Err(WdlError::semantic(
                expr.span,
                "dir() requires a source directory; compile from a .wdl file or pack source",
            ));
        };
        let relative = Path::new(include_path);
        if relative.as_os_str().is_empty() {
            return Err(WdlError::semantic(expr.span, "dir() path cannot be empty"));
        }
        if !is_safe_relative_path(relative) {
            return Err(WdlError::semantic(
                expr.span,
                "dir() path must be relative and cannot contain '..'",
            ));
        }
        let base = source_dir.join(relative);
        let entries =
            crate::includes::dir_relative_files(&base, recursive, max_depth).map_err(|err| {
                WdlError::semantic(
                    expr.span,
                    format!("failed to list directory {}: {err}", base.display()),
                )
            })?;
        // emit forward-slash relative paths so listings are stable across platforms.
        let items = entries
            .iter()
            .map(|entry| Value::String(to_forward_slashes(entry)))
            .collect();
        Ok(Value::Array(items))
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

        // a compute-block local resolves to the `let` slot: the whole path (including the head
        // name) becomes the lookup key list.
        if self.compute_locals.borrow().contains(&head) {
            let mut inner = Map::new();
            inner.insert("let".into(), path_array(segs));
            return Ok(single_key("$ref", Value::Object(inner)));
        }

        // loop/map variables remap to the controlling node's `item` output.
        if let Some(binding) = self.scope.iter().rev().find(|b| b.name == head) {
            let mut path = binding.base.clone();
            path.extend_from_slice(rest);
            return Ok(node_ref_expr(&binding.node_id, &path));
        }

        match head.as_str() {
            "params" => Ok(scoped_ref("params", rest)),
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
            CondKind::Expr(expr) => {
                let mut map = Map::new();
                map.insert("value".into(), self.lower_expr(expr)?);
                Ok(Value::Object(map))
            }
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

fn is_safe_relative_path(path: &Path) -> bool {
    path.components()
        .all(|component| matches!(component, Component::Normal(_) | Component::CurDir))
}

// join a relative path's normal components with `/` so directory listings are platform-stable.
fn to_forward_slashes(path: &Path) -> String {
    path.components()
        .filter_map(|component| match component {
            Component::Normal(part) => Some(part.to_string_lossy()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("/")
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
