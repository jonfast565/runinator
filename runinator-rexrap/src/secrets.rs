// the `.rrx` settings surface: config/secret/profile declarations compiled into a versioned
// `SettingsBundle`, with compatibility wrappers for the former secret-named API.

use chrono::{DateTime, Utc};
use runinator_models::bundles::{
    ExecutionProfileBundleEntry, SecretBundle, SettingBundleEntry, SettingsBundle,
};
use runinator_models::settings::SettingKind;
use runinator_models::validation::Validate;
use runinator_models::value::{Map, Value};

use crate::ast::{Expr, ExprKind, PathSeg, ProfileDecl, SecretDecl, StrPart};
use crate::errors::{RexRapError, Span};
use runinator_rexrap_syntax::parser::parse_settings_document;

/// Compatibility wrapper for the former secret-named parser.
pub fn parse_secrets_str(src: &str) -> Result<SecretBundle, RexRapError> {
    parse_settings_str(src)
}

pub fn parse_settings_str(src: &str) -> Result<SettingsBundle, RexRapError> {
    let document = parse_settings_document(src)?;
    let mut settings = Vec::with_capacity(document.settings.len());
    let mut seen = std::collections::BTreeSet::new();
    for decl in &document.settings {
        let entry = lower_decl(decl)?;
        if !seen.insert((entry.kind, entry.scope.clone(), entry.name.clone())) {
            return Err(RexRapError::syntax(
                decl.span,
                "duplicate setting declaration",
            ));
        }
        settings.push(entry);
    }
    let mut execution_profiles = Vec::with_capacity(document.execution_profiles.len());
    let mut profile_names = std::collections::BTreeSet::new();
    for decl in &document.execution_profiles {
        if !profile_names.insert(decl.name.to_ascii_lowercase()) {
            return Err(RexRapError::syntax(
                decl.span,
                "duplicate execution profile declaration",
            ));
        }
        execution_profiles.push(lower_profile(decl)?);
    }
    Ok(SettingsBundle {
        version: 1,
        settings,
        execution_profiles,
    })
}

/// Compatibility wrapper for the former secret-named formatter.
pub fn secrets_to_rexraps(bundle: &SecretBundle) -> String {
    settings_to_rexrap(bundle)
}

pub fn settings_to_rexrap(bundle: &SettingsBundle) -> String {
    let mut out = String::new();
    for entry in &bundle.settings {
        let kind = entry.kind.as_str();
        // a `/`-joined name re-renders as dotted segments so it re-parses to the same name.
        let address = format!("{}.{}", entry.scope, entry.name.replace('/', "."));
        if let Some(schema) = &entry.schema {
            out.push_str(&format!("@schema({})\n", render_value(schema)));
        }
        if let Some(expires_at) = entry.expires_at {
            out.push_str(&format!(
                "@expires_at({})\n",
                quote(&expires_at.to_rfc3339())
            ));
        }
        out.push_str(&format!(
            "{kind} {address} = {}\n",
            render_value(&entry.value)
        ));
    }
    for entry in &bundle.execution_profiles {
        let mut value = serde_json::to_value(&entry.configuration).unwrap_or_default();
        if let Some(object) = value.as_object_mut() {
            object.remove("name");
        }
        out.push_str(&format!(
            "profile {} = {}\n",
            quote(&entry.configuration.name),
            render_value(&Value::from(value))
        ));
    }
    out
}

fn lower_decl(decl: &SecretDecl) -> Result<SettingBundleEntry, RexRapError> {
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
    if decl.schema.is_some() && kind != SettingKind::Config {
        return Err(RexRapError::syntax(
            decl.span,
            "@schema applies only to config",
        ));
    }
    if decl.expires_at.is_some() && kind != SettingKind::Secret {
        return Err(RexRapError::syntax(
            decl.span,
            "@expires_at applies only to secrets",
        ));
    }
    let schema = decl.schema.as_ref().map(literal_value).transpose()?;
    let expires_at = decl
        .expires_at
        .as_deref()
        .map(DateTime::parse_from_rfc3339)
        .transpose()
        .map_err(|error| RexRapError::syntax(decl.span, format!("invalid @expires_at: {error}")))?
        .map(|value| value.with_timezone(&Utc));
    Ok(SettingBundleEntry {
        scope,
        name: name_parts.join("/"),
        value,
        schema,
        kind,
        updated_at: None,
        expires_at,
    })
}

fn lower_profile(decl: &ProfileDecl) -> Result<ExecutionProfileBundleEntry, RexRapError> {
    let mut value = profile_literal_value(&decl.configuration)?;
    let Some(object) = value.as_object_mut() else {
        return Err(RexRapError::syntax(
            decl.span,
            "profile configuration must be a literal object",
        ));
    };
    object.insert("name".into(), Value::String(decl.name.clone()));
    let configuration = serde_json::from_value::<
        runinator_models::execution_profiles::ExecutionProfilePutRequest,
    >(serde_json::Value::from(value))
    .map_err(|error| RexRapError::syntax(decl.span, format!("invalid profile object: {error}")))?;
    configuration.validate().map_err(|error| {
        RexRapError::syntax(decl.span, format!("invalid profile object: {error}"))
    })?;
    runinator_engine_profile_validation(&configuration, decl.span)?;
    Ok(ExecutionProfileBundleEntry {
        configuration,
        updated_at: None,
    })
}

/// Profile exposure strings deliberately use `${PROFILE_HOME}`-style placeholders. Preserve those
/// as literal template text while retaining the settings rule that ordinary secret/config values
/// cannot interpolate runtime expressions.
fn profile_literal_value(expr: &Expr) -> Result<Value, RexRapError> {
    match &expr.kind {
        ExprKind::Str(parts) => {
            let mut text = String::new();
            for part in parts {
                match part {
                    StrPart::Lit(lit) => text.push_str(lit),
                    StrPart::Expr(inner) => {
                        let ExprKind::Path(path) = &inner.kind else {
                            return Err(RexRapError::syntax(
                                inner.span,
                                "profile templates accept only named placeholders",
                            ));
                        };
                        let mut rendered = String::new();
                        for segment in path {
                            match segment {
                                PathSeg::Key(key) => {
                                    if !rendered.is_empty() {
                                        rendered.push('.');
                                    }
                                    rendered.push_str(key);
                                }
                                PathSeg::Index(index) => {
                                    rendered.push_str(&format!("[{index}]"));
                                }
                            }
                        }
                        text.push_str("${");
                        text.push_str(&rendered);
                        text.push('}');
                    }
                }
            }
            Ok(Value::String(text))
        }
        ExprKind::Array(items) => items
            .iter()
            .map(profile_literal_value)
            .collect::<Result<Vec<_>, _>>()
            .map(Value::Array),
        ExprKind::Object(entries) => {
            let mut map = Map::new();
            for (key, value) in entries {
                map.insert(key.clone(), profile_literal_value(value)?);
            }
            Ok(Value::Object(map))
        }
        _ => literal_value(expr),
    }
}

fn runinator_engine_profile_validation(
    request: &runinator_models::execution_profiles::ExecutionProfilePutRequest,
    span: Span,
) -> Result<(), RexRapError> {
    // Keep syntax lowering transport-neutral while applying the model-owned safety checks. The
    // engine repeats canonical normalization before persistence.
    if request.collection.version != 1 || request.exposure.version != 1 {
        return Err(RexRapError::syntax(
            span,
            "unsupported profile specification version",
        ));
    }
    for source in &request.collection.sources {
        let target = match source {
            runinator_models::execution_profiles::ExecutionProfileSource::File {
                target, ..
            }
            | runinator_models::execution_profiles::ExecutionProfileSource::Directory {
                target,
                ..
            }
            | runinator_models::execution_profiles::ExecutionProfileSource::Command {
                target,
                ..
            } => target,
        };
        runinator_models::execution_profiles::validate_bundle_path(target)
            .map_err(|message| RexRapError::syntax(span, message))?;
    }
    for value in request.exposure.environment.values() {
        runinator_models::execution_profiles::validate_environment_template(value)
            .map_err(|message| RexRapError::syntax(span, message))?;
    }
    Ok(())
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
