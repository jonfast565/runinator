use std::collections::{BTreeSet, HashMap};

use runinator_api::{AsyncApiClient, StaticLocator};
use runinator_models::errors::SendableError;

pub(crate) async fn resolve_secret_refs(
    api_client: &AsyncApiClient<StaticLocator>,
    parameters: serde_json::Value,
) -> Result<serde_json::Value, SendableError> {
    let mut refs = BTreeSet::new();
    collect_secret_refs(&parameters, &mut refs);
    if refs.is_empty() {
        return Ok(parameters);
    }

    let mut secrets = HashMap::new();
    for secret_ref in refs {
        let secret = api_client
            .fetch_credential(&secret_ref.scope, &secret_ref.name)
            .await
            .map_err(|err| -> SendableError { Box::new(err) })?;
        secrets.insert(secret_ref, secret);
    }

    Ok(replace_secret_refs(parameters, &secrets))
}

fn collect_secret_refs(value: &serde_json::Value, refs: &mut BTreeSet<SecretRef>) {
    match value {
        serde_json::Value::String(raw) => {
            if let Some(secret_ref) = parse_secret_ref(raw) {
                refs.insert(secret_ref);
            }
        }
        serde_json::Value::Array(values) => {
            for value in values {
                collect_secret_refs(value, refs);
            }
        }
        serde_json::Value::Object(object) => {
            for value in object.values() {
                collect_secret_refs(value, refs);
            }
        }
        _ => {}
    }
}

fn replace_secret_refs(
    value: serde_json::Value,
    secrets: &HashMap<SecretRef, String>,
) -> serde_json::Value {
    match value {
        serde_json::Value::String(raw) => parse_secret_ref(&raw)
            .and_then(|secret_ref| secrets.get(&secret_ref).cloned())
            .map(serde_json::Value::String)
            .unwrap_or(serde_json::Value::String(raw)),
        serde_json::Value::Array(values) => serde_json::Value::Array(
            values
                .into_iter()
                .map(|value| replace_secret_refs(value, secrets))
                .collect(),
        ),
        serde_json::Value::Object(object) => serde_json::Value::Object(
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
