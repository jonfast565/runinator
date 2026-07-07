use std::collections::{BTreeSet, HashMap};

use runinator_api::{ApiError, AsyncApiClient, StaticLocator};
use runinator_models::errors::SendableError;
use runinator_models::value::Value;

/// whether a secret-resolution failure is transient (web service unreachable, 5xx, discovery) and
/// worth a redelivery, rather than a definitive rejection of the reference itself (4xx, malformed
/// configuration) that should fail the action.
pub(crate) fn is_transient_secret_error(err: &SendableError) -> bool {
    match err.downcast_ref::<ApiError>() {
        Some(ApiError::Request(_)) | Some(ApiError::Discovery(_)) => true,
        Some(ApiError::Http { status, .. }) => status.is_server_error(),
        _ => false,
    }
}

pub(crate) async fn resolve_secret_refs(
    api_client: &AsyncApiClient<StaticLocator>,
    parameters: Value,
) -> Result<Value, SendableError> {
    let mut refs = BTreeSet::new();
    collect_secret_refs(&parameters, &mut refs);
    if refs.is_empty() {
        return Ok(parameters);
    }

    tracing::debug!(count = refs.len(), "resolving action secret reference(s)");
    let mut secrets = HashMap::new();
    for secret_ref in refs {
        // never log the secret value itself; scope/name identify the reference, not its contents.
        let secret = api_client
            .fetch_credential(&secret_ref.scope, &secret_ref.name)
            .await
            .map_err(|err| {
                tracing::warn!(
                    scope = %secret_ref.scope,
                    name = %secret_ref.name,
                    "failed to fetch credential: {}",
                    err
                );
                Box::new(err) as SendableError
            })?;
        secrets.insert(secret_ref, secret);
    }

    Ok(replace_secret_refs(parameters, &secrets))
}

fn collect_secret_refs(value: &Value, refs: &mut BTreeSet<SecretRef>) {
    match value {
        Value::String(raw) => {
            if let Some(secret_ref) = parse_secret_ref(raw) {
                refs.insert(secret_ref);
            }
        }
        Value::Array(values) => {
            for value in values {
                collect_secret_refs(value, refs);
            }
        }
        Value::Object(object) => {
            for value in object.values() {
                collect_secret_refs(value, refs);
            }
        }
        _ => {}
    }
}

fn replace_secret_refs(value: Value, secrets: &HashMap<SecretRef, String>) -> Value {
    match value {
        Value::String(raw) => parse_secret_ref(&raw)
            .and_then(|secret_ref| secrets.get(&secret_ref).cloned())
            .map(Value::String)
            .unwrap_or(Value::String(raw)),
        Value::Array(values) => Value::Array(
            values
                .into_iter()
                .map(|value| replace_secret_refs(value, secrets))
                .collect(),
        ),
        Value::Object(object) => Value::Object(
            object
                .into_iter()
                .map(|(key, value)| (key, replace_secret_refs(value, secrets)))
                .collect(),
        ),
        other => other,
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
struct SecretRef {
    scope: String,
    name: String,
}

fn parse_secret_ref(raw: &str) -> Option<SecretRef> {
    let path = raw.strip_prefix("secret://")?;
    let (scope, name) = path.split_once('/')?;
    if scope.is_empty() || name.is_empty() {
        return None;
    }
    Some(SecretRef {
        scope: percent_decode(scope)?,
        name: percent_decode(name)?,
    })
}

fn percent_decode(raw: &str) -> Option<String> {
    let bytes = raw.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' {
            let hi = hex_value(*bytes.get(index + 1)?)?;
            let lo = hex_value(*bytes.get(index + 2)?)?;
            decoded.push((hi << 4) | lo);
            index += 3;
            continue;
        }
        decoded.push(bytes[index]);
        index += 1;
    }
    String::from_utf8(decoded).ok()
}

fn hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}
