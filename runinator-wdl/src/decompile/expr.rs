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
                // a library call carries two keys ($call + args), so handle it before the
                // single-key forms.
                if let Some(name) = map.get("$call").and_then(Value::as_str) {
                    let args = map
                        .get("args")
                        .and_then(Value::as_array)
                        .cloned()
                        .unwrap_or_default();
                    // re-sugar an `at(base, key)` into `base.key` / `base[index]` access syntax.
                    if name == "at"
                        && args.len() == 2
                        && let Some(text) = self.access(&args[0], &args[1])?
                    {
                        return Ok(text);
                    }
                    let rendered = args
                        .iter()
                        .map(|arg| self.expr(arg))
                        .collect::<Result<Vec<_>, _>>()?;
                    return Ok(format!("{name}({})", rendered.join(", ")));
                }
                if map.len() == 1 {
                    if let Some(spec) = map.get("$lambda").and_then(Value::as_object) {
                        let params = spec
                            .get("params")
                            .and_then(Value::as_array)
                            .map(|items| {
                                items
                                    .iter()
                                    .filter_map(Value::as_str)
                                    .map(str::to_string)
                                    .collect::<Vec<_>>()
                            })
                            .unwrap_or_default();
                        let body = spec
                            .get("body")
                            .ok_or_else(|| WdlError::Decompile("$lambda missing body".into()))?;
                        // a single param renders bare (`x => …`); zero or many parenthesize.
                        let head = if params.len() == 1 {
                            params[0].clone()
                        } else {
                            format!("({})", params.join(", "))
                        };
                        return Ok(format!("{head} => {}", self.expr(body)?));
                    }
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
                    if let Some(inner) = map.get("$neg") {
                        return Ok(format!("-{}", self.arith_operand(inner, ARITH_UNARY)?));
                    }
                    for (key, op, prec) in [
                        ("$add", " + ", ARITH_SUM),
                        ("$sub", " - ", ARITH_SUM),
                        ("$mul", " * ", ARITH_PRODUCT),
                        ("$div", " / ", ARITH_PRODUCT),
                        ("$mod", " % ", ARITH_PRODUCT),
                    ] {
                        if let Some(items) = map.get(key).and_then(Value::as_array) {
                            return self.arith(items, op, prec);
                        }
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

    /// render `...alias` spread recipe segments (recovered from the metadata sidecar) as a
    /// comma-separated argument/object body: `...alias` for a spread, `key: value` otherwise.
    pub(super) fn render_segs(&self, segs: &[Value]) -> Result<String, WdlError> {
        let mut parts = Vec::with_capacity(segs.len());
        for seg in segs {
            if let Some(name) = seg.get("spread").and_then(Value::as_str) {
                parts.push(format!("...{name}"));
                continue;
            }
            let key = seg
                .get("key")
                .and_then(Value::as_str)
                .ok_or_else(|| WdlError::Decompile("spread recipe segment missing key".into()))?;
            let value = seg
                .get("value")
                .ok_or_else(|| WdlError::Decompile("spread recipe segment missing value".into()))?;
            parts.push(format!("{key}: {}", self.render_recipe_value(value)?));
        }
        Ok(parts.join(", "))
    }

    // render a recipe value: a `plain` value goes through the normal expr path; `object`/`array`
    // recurse so spreads nested inside object/array literals survive.
    fn render_recipe_value(&self, value: &Value) -> Result<String, WdlError> {
        if let Some(plain) = value.get("plain") {
            return self.expr(plain);
        }
        if let Some(segs) = value.get("object").and_then(Value::as_array) {
            return Ok(format!("{{ {} }}", self.render_segs(segs)?));
        }
        if let Some(items) = value.get("array").and_then(Value::as_array) {
            let parts = items
                .iter()
                .map(|item| self.render_recipe_value(item))
                .collect::<Result<Vec<_>, _>>()?;
            return Ok(format!("[{}]", parts.join(", ")));
        }
        Err(WdlError::Decompile("invalid spread recipe value".into()))
    }

    fn join_binary(&self, items: &[Value], sep: &str) -> Result<String, WdlError> {
        let parts = items
            .iter()
            .map(|item| self.expr(item))
            .collect::<Result<Vec<_>, _>>()?;
        Ok(parts.join(sep))
    }

    // render a left-associative arithmetic chain at `prec`, parenthesizing the right operand when
    // it binds equally or more loosely (preserving the lowered left-nested structure on re-parse).
    fn arith(&self, items: &[Value], sep: &str, prec: u8) -> Result<String, WdlError> {
        let mut out = String::new();
        for (index, item) in items.iter().enumerate() {
            if index > 0 {
                out.push_str(sep);
            }
            // the left operand keeps same-precedence chains flat; later operands need parens when
            // they are not strictly tighter.
            let threshold = if index == 0 { prec } else { prec + 1 };
            out.push_str(&self.arith_operand(item, threshold)?);
        }
        Ok(out)
    }

    // render an operand, wrapping it in parentheses when its top-level arithmetic precedence is
    // below `min_prec`.
    fn arith_operand(&self, value: &Value, min_prec: u8) -> Result<String, WdlError> {
        let text = self.expr(value)?;
        if arith_prec(value) < min_prec {
            return Ok(format!("({text})"));
        }
        Ok(text)
    }

    // re-sugar an `at(base, key)` call into access syntax, returning None when it must stay a call.
    // a static key on a `$ref` base is left alone: `base.key` would fold back into the path on
    // recompile and change the graph, whereas every other `at` round-trips through access syntax.
    fn access(&self, base: &Value, key: &Value) -> Result<Option<String>, WdlError> {
        let base_is_ref = base
            .as_object()
            .is_some_and(|object| object.contains_key("$ref"));
        let static_key = key.as_i64().is_some() || key.as_str().is_some();
        if base_is_ref && static_key {
            return Ok(None);
        }
        let text = self.expr(base)?;
        let base_text = if needs_access_parens(base) {
            format!("({text})")
        } else {
            text
        };
        if let Some(index) = key.as_i64() {
            return Ok(Some(format!("{base_text}[{index}]")));
        }
        if let Some(name) = key.as_str() {
            if is_ident(name) {
                return Ok(Some(format!("{base_text}.{name}")));
            }
            return Ok(Some(format!("{base_text}[{}]", quote(name))));
        }
        // a dynamic key (a ref/call/arithmetic expression) renders as a bracketed expression.
        Ok(Some(format!("{base_text}[{}]", self.expr(key)?)))
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
        // a compute local: the path array's first element is the local name, the rest are fields.
        if let Some(path) = object.get("let").and_then(Value::as_array) {
            let mut segs = path.iter();
            let head = segs
                .next()
                .and_then(Value::as_str)
                .ok_or_else(|| WdlError::Decompile("invalid let ref".into()))?;
            let rest: Vec<Value> = segs.cloned().collect();
            return Ok(self.append_path(head, &rest));
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
        if object.len() == 1 && object.contains_key("value") {
            return self.expr(left);
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

// arithmetic precedence tiers, ascending tightness, for decompile parenthesization.
const ARITH_SUM: u8 = 1;
const ARITH_PRODUCT: u8 = 2;
const ARITH_UNARY: u8 = 3;
const ARITH_ATOM: u8 = 4;

/// whether an expression value must be parenthesized before a `.key` / `[i]` access is appended.
/// operator forms bind looser than access; refs, calls, coercions, and literals do not.
fn needs_access_parens(value: &Value) -> bool {
    let Some(map) = value.as_object() else {
        return false;
    };
    [
        "$add",
        "$sub",
        "$mul",
        "$div",
        "$mod",
        "$neg",
        "$concat",
        "$coalesce",
    ]
    .iter()
    .any(|key| map.contains_key(*key))
}

/// the top-level arithmetic precedence of a lowered expression value.
fn arith_prec(value: &Value) -> u8 {
    let Some(map) = value.as_object() else {
        return ARITH_ATOM;
    };
    if map.contains_key("$add") || map.contains_key("$sub") {
        return ARITH_SUM;
    }
    if map.contains_key("$mul") || map.contains_key("$div") || map.contains_key("$mod") {
        return ARITH_PRODUCT;
    }
    if map.contains_key("$neg") {
        return ARITH_UNARY;
    }
    ARITH_ATOM
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
        RuninatorType::Struct { fields, additional } => {
            let mut parts = fields
                .iter()
                .map(|(name, field)| {
                    let mark = if field.required { "" } else { "?" };
                    format!("{name}{mark}: {}", render_type(&field.ty))
                })
                .collect::<Vec<_>>();
            if let Some(additional) = additional {
                parts.push(format!("...: {}", render_type(additional)));
            }
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
