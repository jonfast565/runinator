// inverts the json expression/condition forms back into wdl surface text. this is the
// mirror of lower/expr.rs and relies on the runtime json being the canonical spec.

use runinator_models::types::RuninatorType;
use runinator_models::value::Value;

use crate::errors::WdlError;

use super::Decompiler;

impl Decompiler<'_> {
    pub(super) fn expr(&self, value: &Value) -> Result<String, WdlError> {
        match value {
            Value::Null => Ok("null".to_string()),
            Value::Bool(b) => Ok(b.to_string()),
            Value::Number(_) => Ok(value.to_string()),
            Value::String(text) => Ok(secret_path(text).unwrap_or_else(|| quote(text))),
            Value::Array(items) => {
                let parts = items
                    .iter()
                    .map(|item| self.expr(item))
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(format!("[{}]", parts.join(", ")))
            }
            Value::Object(map) => {
                if map.len() == 1 {
                    if let Some(reference) = map.get("$ref") {
                        return self.reference(reference);
                    }
                    if let Some(items) = map.get("$concat").and_then(Value::as_array) {
                        return self.join_binary(items, " ++ ");
                    }
                    if let Some(items) = map.get("$coalesce").and_then(Value::as_array) {
                        return self.join_binary(items, " ?? ");
                    }
                    if let Some(inner) = map.get("$to_string") {
                        return Ok(format!("string({})", self.expr(inner)?));
                    }
                    if let Some(inner) = map.get("$to_json_string") {
                        return Ok(format!("json({})", self.expr(inner)?));
                    }
                }
                // plain object literal.
                let mut parts = Vec::new();
                for (key, value) in map {
                    parts.push(format!("{}: {}", key, self.expr(value)?));
                }
                Ok(format!("{{ {} }}", parts.join(", ")))
            }
        }
    }

    fn join_binary(&self, items: &[Value], sep: &str) -> Result<String, WdlError> {
        let parts = items
            .iter()
            .map(|item| self.expr(item))
            .collect::<Result<Vec<_>, _>>()?;
        Ok(parts.join(sep))
    }

    fn reference(&self, reference: &Value) -> Result<String, WdlError> {
        let object = reference
            .as_object()
            .ok_or_else(|| WdlError::Decompile("invalid $ref".into()))?;
        if let Some(path) = object.get("input") {
            return Ok(self.dotted("input", path));
        }
        if let Some(path) = object.get("prev") {
            return Ok(self.dotted("prev", path));
        }
        if let Some(path) = object.get("workflow") {
            return Ok(self.dotted("run", path));
        }
        if let Some(path) = object.get("config") {
            return Ok(self.dotted("config", path));
        }
        if let (Some(node), Some(output)) = (object.get("node"), object.get("output")) {
            let node_id = node
                .as_str()
                .ok_or_else(|| WdlError::Decompile("invalid $ref node".into()))?;
            // a loop/map variable reads `<var>` for ["item"] and `<var>.x` for ["item","x"].
            if let Some(var) = self.loop_var(node_id) {
                let segs = output.as_array().cloned().unwrap_or_default();
                if segs.first().and_then(Value::as_str) == Some("item") {
                    return Ok(self.append_path(&var, &segs[1..]));
                }
            }
            return Ok(self.dotted(node_id, output));
        }
        Err(WdlError::Decompile("unrecognized $ref".into()))
    }

    fn dotted(&self, head: &str, path: &Value) -> String {
        let segs = path.as_array().cloned().unwrap_or_default();
        self.append_path(head, &segs)
    }

    fn append_path(&self, head: &str, segs: &[Value]) -> String {
        let mut out = head.to_string();
        for seg in segs {
            match seg {
                Value::String(key) => {
                    out.push('.');
                    out.push_str(key);
                }
                other => {
                    out.push('.');
                    out.push_str(&other.to_string());
                }
            }
        }
        out
    }

    pub(super) fn cond(&self, value: &Value) -> Result<String, WdlError> {
        let object = value
            .as_object()
            .ok_or_else(|| WdlError::Decompile("condition must be an object".into()))?;
        if let Some(items) = object.get("all").and_then(Value::as_array) {
            return self.join_cond(items, " && ");
        }
        if let Some(items) = object.get("any").and_then(Value::as_array) {
            return self.join_cond(items, " || ");
        }
        if let Some(inner) = object.get("not") {
            return Ok(format!("!({})", self.cond(inner)?));
        }
        let left = object
            .get("value")
            .or_else(|| object.get("left"))
            .ok_or_else(|| WdlError::Decompile("condition missing value".into()))?;
        let left_text = self.expr(left)?;
        if object.get("exists").is_some() {
            return Ok(format!("exists {left_text}"));
        }
        for (key, op) in CMP_OPS {
            if let Some(operand) = object.get(key) {
                return Ok(format!("{left_text} {op} {}", self.expr(operand)?));
            }
        }
        Err(WdlError::Decompile("unrecognized condition".into()))
    }

    fn join_cond(&self, items: &[Value], sep: &str) -> Result<String, WdlError> {
        let parts = items
            .iter()
            .map(|item| {
                let text = self.cond(item)?;
                // wrap nested compound conditions to preserve precedence.
                if item.get("all").is_some() || item.get("any").is_some() {
                    Ok(format!("({text})"))
                } else {
                    Ok(text)
                }
            })
            .collect::<Result<Vec<_>, _>>()?;
        Ok(parts.join(sep))
    }
}

const CMP_OPS: [(&str, &str); 10] = [
    ("equals", "=="),
    ("not_equals", "!="),
    ("greater_than_or_equal", ">="),
    ("less_than_or_equal", "<="),
    ("greater_than", ">"),
    ("less_than", "<"),
    ("contains", "contains"),
    ("in", "in"),
    ("starts_with", "starts_with"),
    ("ends_with", "ends_with"),
];

/// render a RuninatorType as a wdl type expression.
pub(super) fn render_type(ty: &RuninatorType) -> String {
    match ty {
        RuninatorType::Null => "null".into(),
        RuninatorType::Boolean => "boolean".into(),
        RuninatorType::Integer => "integer".into(),
        RuninatorType::Number => "number".into(),
        RuninatorType::String => "string".into(),
        RuninatorType::Any => "any".into(),
        RuninatorType::Array(inner) => format!("{}[]", render_type(inner)),
        RuninatorType::Map(inner) => format!("map<{}>", render_type(inner)),
        RuninatorType::Union(variants) => variants
            .iter()
            .map(render_type)
            .collect::<Vec<_>>()
            .join(" | "),
        RuninatorType::Struct { fields, .. } => {
            let parts = fields
                .iter()
                .map(|(name, field)| {
                    let mark = if field.required { "" } else { "?" };
                    format!("{name}{mark}: {}", render_type(&field.ty))
                })
                .collect::<Vec<_>>();
            format!("{{ {} }}", parts.join(", "))
        }
    }
}

/// recognize a `secret://<scope>/<name>` literal and render it as `secret.<scope>.<name…>`.
/// returns None (so the caller quotes it as a plain string) unless every segment is a bare
/// ident, keeping the result a clean round-trip with the lowering.
fn secret_path(text: &str) -> Option<String> {
    let rest = text.strip_prefix("secret://")?;
    let (scope, name) = rest.split_once('/')?;
    if scope.is_empty() || name.is_empty() {
        return None;
    }
    let mut out = String::from("secret");
    for seg in std::iter::once(scope).chain(name.split('/')) {
        if !is_ident(seg) {
            return None;
        }
        out.push('.');
        out.push_str(seg);
    }
    Some(out)
}

fn is_ident(seg: &str) -> bool {
    let mut chars = seg.chars();
    matches!(chars.next(), Some(c) if c.is_ascii_alphabetic() || c == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
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
            '\t' => out.push_str("\\t"),
            '\r' => out.push_str("\\r"),
            other => out.push(other),
        }
    }
    out.push('"');
    out
}
