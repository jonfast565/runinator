// the `.rexraps` secrets/config surface: a flat list of `secret`/`config` declarations addressing a
// dotted `scope.name`, each assigned a pure literal. mirrors rexrap's `secret.*` / `config.*`
// reference surface and lowers to a `SecretBundle` for import. the reverse (`secrets_to_rexraps`)
// re-renders a bundle so exports round-trip.

use runinator_models::bundles::{SecretBundle, SecretBundleEntry};
use runinator_models::settings::SettingKind;
use runinator_models::value::{Map, Value};

use crate::ast::{Expr, ExprKind, PathSeg, SecretDecl, StrPart};
use crate::errors::{RexRapError, Span};
use runinator_rexrap_syntax::parser::parse_secrets_document;

/// parse `.rexraps` source into a `SecretBundle`. values must be literals; references and
/// interpolation are rejected with a span-anchored error.
pub fn parse_secrets_str(src: &str) -> Result<SecretBundle, RexRapError> {
    let decls = parse_secrets_document(src)?;
    let mut secrets = Vec::with_capacity(decls.len());
    for decl in &decls {
        secrets.push(lower_decl(decl)?);
    }
    Ok(SecretBundle { secrets })
}

/// render a `SecretBundle` back into `.rexraps` source.
pub fn secrets_to_rexraps(bundle: &SecretBundle) -> String {
    let mut out = String::new();
    for entry in &bundle.secrets {
        let kind = entry.kind.as_str();
        // a `/`-joined name re-renders as dotted segments so it re-parses to the same name.
        let address = format!("{}.{}", entry.scope, entry.name.replace('/', "."));
        out.push_str(&format!(
            "{kind} {address} = {}\n",
            render_value(&entry.value)
        ));
    }
    out
}

fn lower_decl(decl: &SecretDecl) -> Result<SecretBundleEntry, RexRapError> {
    let mut segments = decl.path.iter().map(|segment| match segment {
        PathSeg::Key(key) => key.clone(),
        PathSeg::Index(index) => index.to_string(),
    });
    let scope = segments
        .next()
        .ok_or_else(|| RexRapError::syntax(decl.span, "secret address needs a scope"))?;
    let name_parts: Vec<String> = segments.collect();
    if name_parts.is_empty() {
        return Err(RexRapError::syntax(
            decl.span,
            "secret address must be `<scope>.<name>`",
        ));
    }
    let value = literal_value(&decl.value)?;
    let kind = if decl.is_config {
        SettingKind::Config
    } else {
        SettingKind::Secret
    };
    Ok(SecretBundleEntry {
        scope,
        name: name_parts.join("/"),
        value,
        schema: None,
        kind,
        updated_at: None,
        expires_at: None,
    })
}

/// evaluate a pure-literal expression to a concrete value, rejecting references, interpolation,
/// and any other dynamic expression so a secret/config value is always concrete data.
fn literal_value(expr: &Expr) -> Result<Value, RexRapError> {
    match &expr.kind {
        ExprKind::Null => Ok(Value::Null),
        ExprKind::Bool(value) => Ok(Value::Bool(*value)),
        ExprKind::Int(value) => Ok(Value::from(*value)),
        ExprKind::Float(value) => Ok(Value::from(*value)),
        ExprKind::Str(parts) => literal_string(parts, expr.span),
        ExprKind::Array(items) => {
            let mut out = Vec::with_capacity(items.len());
            for item in items {
                out.push(literal_value(item)?);
            }
            Ok(Value::Array(out))
        }
        ExprKind::Object(entries) => {
            let mut map = Map::new();
            for (key, value) in entries {
                map.insert(key.clone(), literal_value(value)?);
            }
            Ok(Value::Object(map))
        }
        _ => Err(RexRapError::syntax(
            expr.span,
            "secret values must be literals, not references or expressions",
        )),
    }
}

fn literal_string(parts: &[StrPart], span: Span) -> Result<Value, RexRapError> {
    let mut text = String::new();
    for part in parts {
        match part {
            StrPart::Lit(lit) => text.push_str(lit),
            StrPart::Expr(_) => {
                return Err(RexRapError::syntax(
                    span,
                    "secret strings cannot interpolate `${...}`",
                ));
            }
        }
    }
    Ok(Value::String(text))
}

/// render a concrete value as a rexrap literal for `.rexraps` export.
fn render_value(value: &Value) -> String {
    match value {
        Value::Null => "null".into(),
        Value::Bool(value) => value.to_string(),
        Value::Number(_) => value.to_string(),
        Value::String(text) => quote(text),
        Value::Array(items) => {
            let parts = items.iter().map(render_value).collect::<Vec<_>>();
            format!("[{}]", parts.join(", "))
        }
        Value::Object(map) => {
            let parts = map
                .iter()
                .map(|(key, value)| format!("{key}: {}", render_value(value)))
                .collect::<Vec<_>>();
            format!("{{ {} }}", parts.join(", "))
        }
    }
}

fn quote(text: &str) -> String {
    let escaped = text.replace('\\', "\\\\").replace('"', "\\\"");
    format!("\"{escaped}\"")
}
