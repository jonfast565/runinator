// typed encoding and validation for the unified settings store. config values carry a json-schema
// (declared on the request, else inferred from the value on first write) that is pinned per
// (scope, name) and validated on every later write (hard error on mismatch); secrets are
// implicitly string-typed. the encode/decode helpers are pure: callers pass the previously-stored
// schema (decoded from the persisted bytes) so they never touch the database. `config_type_tree`
// is the one database-reading helper, used by workflow validation to type-check `config.*` refs.

use std::{
    collections::BTreeMap,
    sync::{Arc, RwLock},
    time::Duration,
};

use chrono::{DateTime, Utc};
use runinator_models::types::RuninatorType;
use runinator_models::value::Value;
use runinator_models::{
    bundles::{SecretBundle, SecretBundleEntry},
    errors::SendableError,
    server_settings::{SERVER_SETTINGS_NAME, SERVER_SETTINGS_SCOPE, ServerSettings},
    settings::SettingKind,
};
use runinator_secrets::secret_cipher::SecretCipher;
use runinator_secrets::stored_secret::StoredSecret;
use runinator_store::{RuntimeStore, roles::SettingStore};

use serde::{Deserialize, Serialize};
use tokio::sync::Notify;

// the persisted form of a config entry: the json value plus the schema it was validated against,
// so the schema is pinned per (scope, name) and later value-only updates reuse it.
#[derive(Debug, Serialize, Deserialize)]
struct StoredConfig {
    value: Value,
    schema: Value,
}

/// decode a stored config payload back to its json value (back-compat: a bare value or string).
pub fn decode_config_value(bytes: &[u8]) -> Value {
    if let Ok(stored) = serde_json::from_slice::<StoredConfig>(bytes) {
        return stored.value;
    }
    serde_json::from_slice::<Value>(bytes)
        .unwrap_or_else(|_| Value::String(String::from_utf8_lossy(bytes).into_owned()))
}

/// the schema pinned in a stored config payload, if it carries one.
pub fn decode_config_schema(bytes: &[u8]) -> Option<Value> {
    serde_json::from_slice::<StoredConfig>(bytes)
        .ok()
        .map(|stored| stored.schema)
}

/// the pinned type of a stored config slot, decoded from its bytes (back-compat: infer from the
/// bare value when no schema is stored).
pub fn stored_config_type(bytes: &[u8]) -> Option<RuninatorType> {
    match decode_config_schema(bytes) {
        Some(schema) => Some(RuninatorType::from_json_schema(&schema)),
        None => Some(RuninatorType::infer_from_value(&decode_config_value(bytes))),
    }
}

/// validate a value for its kind and produce the bytes to persist. config validates against a
/// schema (the request's, else `stored_schema` pinned on the first write, else one inferred from
/// the value on first write) and must conform to it; secrets must be a non-empty string.
/// `stored_schema` is the schema decoded from this slot's previously-stored bytes, if any.
pub fn validate_and_encode(
    kind: SettingKind,
    scope: &str,
    name: &str,
    value: &Value,
    schema: Option<&Value>,
    stored_schema: Option<&Value>,
) -> Result<Vec<u8>, String> {
    validate_and_encode_with_expiry(kind, scope, name, value, schema, stored_schema, None)
}

/// validate and encode a setting, attaching optional expiry metadata to secrets.
pub fn validate_and_encode_with_expiry(
    kind: SettingKind,
    scope: &str,
    name: &str,
    value: &Value,
    schema: Option<&Value>,
    stored_schema: Option<&Value>,
    expires_at: Option<DateTime<Utc>>,
) -> Result<Vec<u8>, String> {
    if kind == SettingKind::Config && expires_at.is_some() {
        return Err(format!(
            "config '{scope}/{name}' cannot carry secret expiry metadata"
        ));
    }
    match kind {
        SettingKind::Secret => {
            let Value::String(text) = value else {
                return Err(format!(
                    "secret '{scope}/{name}' value must be a string, got {}",
                    value_type(value)
                ));
            };
            if text.trim().is_empty() {
                return Err(format!("secret '{scope}/{name}' value must not be empty"));
            }
            StoredSecret::new(text.clone(), expires_at)
                .encode()
                .map_err(|err| format!("failed to encode secret '{scope}/{name}': {err}"))
        }
        SettingKind::Config => {
            // a caller-supplied schema is checked as untrusted input; a stored schema (pinned on
            // the first write) is trusted; with neither, infer the schema from the value itself.
            let ty = match schema {
                Some(schema) => RuninatorType::from_json_schema_checked(schema)
                    .map_err(|err| format!("invalid config schema for '{scope}/{name}': {err}"))?,
                None => match stored_schema {
                    Some(stored) => RuninatorType::from_json_schema(stored),
                    None => RuninatorType::infer_from_value(value),
                },
            };
            ty.validate_value(value).map_err(|violation| {
                format!("config '{scope}/{name}' value does not match schema: {violation}")
            })?;
            serde_json::to_vec(&StoredConfig {
                value: value.clone(),
                schema: ty.to_json_schema(),
            })
            .map_err(|err| format!("failed to encode config '{scope}/{name}': {err}"))
        }
    }
}

/// Decode a stored versioned secret envelope and its optional expiry metadata.
pub fn decode_secret(bytes: &[u8]) -> Result<StoredSecret, String> {
    StoredSecret::decode(bytes)
}

fn value_type(value: &Value) -> &'static str {
    match value {
        Value::Null => "null",
        Value::Bool(_) => "boolean",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

// the cipher that protects setting values at rest, keyed by `RUNINATOR_CREDENTIAL_KEY` (plus any
// rotation-overlap keys in `RUNINATOR_CREDENTIAL_KEY_PREVIOUS`). the value column holds ciphertext;
// only the web service and engine hold the keys.
fn settings_cipher() -> SecretCipher {
    SecretCipher::from_env()
}

/// A setting bundle failed either validation (safe to report as a bad request) or persistence.
#[derive(Debug)]
pub struct SettingBundleImportError {
    pub bad_request: bool,
    pub message: String,
}

/// Import settings through the supplied store, preserving the reconciliation semantics used by
/// both the credentials endpoint and compiled packs. Passing a transactional store makes these
/// writes part of the caller's larger operation.
pub async fn import_setting_bundle_with<T: SettingStore + RuntimeStore>(
    db: &T,
    bundle: &SecretBundle,
    overwrite: bool,
) -> Result<Vec<SecretBundleEntry>, SettingBundleImportError> {
    let cipher = settings_cipher();
    let mut imported = Vec::with_capacity(bundle.secrets.len());
    for setting in &bundle.secrets {
        if runinator_models::server_settings::is_reserved_server_setting(
            setting.kind,
            &setting.scope,
            &setting.name,
        ) {
            return Err(SettingBundleImportError {
                bad_request: true,
                message: "server/operational_policy is reserved for the server settings API".into(),
            });
        }
        let incoming_ts = setting.updated_at.map(|updated_at| updated_at.timestamp());
        let stored = db
            .fetch_setting(setting.kind, setting.scope.clone(), setting.name.clone())
            .await
            .map_err(|error| SettingBundleImportError {
                bad_request: false,
                message: error.to_string(),
            })?;
        if let Some(stored) = &stored {
            let is_newer = incoming_ts
                .map(|timestamp| timestamp > stored.updated_at)
                .unwrap_or(false);
            if !overwrite && !is_newer {
                log::info!(
                    "Skipping import of {} {}/{}: stored copy is up to date",
                    setting.kind.as_str(),
                    setting.scope,
                    setting.name
                );
                imported.push(redacted_entry(setting));
                continue;
            }
        }
        let stored_schema = stored
            .as_ref()
            .and_then(|record| cipher.try_decrypt(&record.value))
            .and_then(|bytes| decode_config_schema(&bytes));
        let bytes = validate_and_encode_with_expiry(
            setting.kind,
            &setting.scope,
            &setting.name,
            &setting.value,
            setting.schema.as_ref(),
            stored_schema.as_ref(),
            setting.expires_at,
        )
        .map_err(|message| SettingBundleImportError {
            bad_request: true,
            message,
        })?;
        db.upsert_setting(
            setting.kind,
            setting.scope.clone(),
            setting.name.clone(),
            cipher.encrypt(&bytes),
            incoming_ts.unwrap_or_else(|| Utc::now().timestamp()),
        )
        .await
        .map_err(|error| SettingBundleImportError {
            bad_request: false,
            message: error.to_string(),
        })?;
        imported.push(redacted_entry(setting));
    }
    Ok(imported)
}

fn redacted_entry(setting: &SecretBundleEntry) -> SecretBundleEntry {
    SecretBundleEntry {
        scope: setting.scope.clone(),
        name: setting.name.clone(),
        value: Value::Null,
        schema: None,
        kind: setting.kind,
        updated_at: setting.updated_at,
        expires_at: setting.expires_at,
    }
}

/// the config type tree `{ <scope>: { <name>: <type> } }` used to type-check `config.*` references
/// at workflow validation. each level is an open struct, so a not-yet-configured scope or name
/// stays permissive (`any`) rather than failing validation.
pub async fn config_type_tree<T: RuntimeStore>(db: &T) -> RuninatorType {
    let cipher = settings_cipher();
    let Ok(entries) = db.list_settings().await else {
        return RuninatorType::map(RuninatorType::Any);
    };
    let mut scopes: BTreeMap<String, BTreeMap<String, RuninatorType>> = BTreeMap::new();
    for entry in entries {
        if entry.kind != SettingKind::Config {
            continue;
        }
        if runinator_models::server_settings::is_reserved_server_setting(
            entry.kind,
            &entry.scope,
            &entry.name,
        ) {
            continue;
        }
        let Some(plaintext) = cipher.try_decrypt(&entry.value) else {
            continue;
        };
        let Some(ty) = stored_config_type(&plaintext) else {
            continue;
        };
        scopes
            .entry(entry.scope)
            .or_default()
            .insert(entry.name, ty);
    }
    let scope_fields = scopes.into_iter().map(|(scope, names)| {
        (
            scope,
            RuninatorType::open_structure(names, RuninatorType::Any),
        )
    });
    RuninatorType::open_structure(scope_fields, RuninatorType::Any)
}

/// Load the platform operating policy, falling back field-by-field to the compiled defaults when
/// no row exists or an older stored document predates newly-added fields.
pub async fn load_server_settings<T: RuntimeStore>(
    db: &T,
) -> Result<ServerSettings, SendableError> {
    let records = db.list_settings().await?;
    let Some(record) = records.iter().find(|record| {
        record.kind == SettingKind::Config
            && record.scope == SERVER_SETTINGS_SCOPE
            && record.name == SERVER_SETTINGS_NAME
    }) else {
        // Compatibility for deployments that saved the original one-field auth policy before the
        // unified server policy existed. The next UI save writes the consolidated document.
        let mut settings = ServerSettings::default();
        if let Some(record) = records.iter().find(|record| {
            record.kind == SettingKind::Config
                && record.scope == "auth"
                && record.name == "max_refreshes"
        }) && let Some(bytes) = settings_cipher().try_decrypt(&record.value)
            && let Ok(value) = serde_json::from_slice::<u64>(&bytes)
        {
            settings.authentication.max_refreshes = value;
        }
        settings.validate().map_err(|message| {
            Box::new(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                message,
            )) as SendableError
        })?;
        return Ok(settings);
    };
    let plaintext = settings_cipher()
        .try_decrypt(&record.value)
        .ok_or_else(|| {
            Box::new(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "server settings could not be decrypted with the configured credential key",
            )) as SendableError
        })?;
    let decoded = decode_config_value(&plaintext);
    let settings: ServerSettings = serde_json::from_value(decoded.into()).map_err(|error| {
        Box::new(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("stored server settings are invalid: {error}"),
        )) as SendableError
    })?;
    settings.validate().map_err(|message| {
        Box::new(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            message,
        )) as SendableError
    })?;
    Ok(settings)
}

/// Validate and persist the complete platform operating policy as one atomic config value.
pub async fn save_server_settings<T: SettingStore>(
    db: &T,
    settings: &ServerSettings,
) -> Result<(), SendableError> {
    settings.validate().map_err(|message| {
        Box::new(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            message,
        )) as SendableError
    })?;
    let value = Value::from(
        serde_json::to_value(settings).map_err(|error| Box::new(error) as SendableError)?,
    );
    let bytes = validate_and_encode(
        SettingKind::Config,
        SERVER_SETTINGS_SCOPE,
        SERVER_SETTINGS_NAME,
        &value,
        None,
        None,
    )
    .map_err(|message| {
        Box::new(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            message,
        )) as SendableError
    })?;
    db.upsert_setting(
        SettingKind::Config,
        SERVER_SETTINGS_SCOPE.into(),
        SERVER_SETTINGS_NAME.into(),
        settings_cipher().encrypt(&bytes),
        Utc::now().timestamp(),
    )
    .await
}

/// Cheap, cloneable snapshot shared by all loops in an engine replica.
#[derive(Clone)]
pub struct ServerSettingsHandle(Arc<RwLock<ServerSettings>>);

impl ServerSettingsHandle {
    pub async fn load<T: RuntimeStore>(db: &T) -> Result<Self, SendableError> {
        Ok(Self(Arc::new(RwLock::new(load_server_settings(db).await?))))
    }

    pub fn current(&self) -> ServerSettings {
        self.0
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }

    async fn refresh<T: RuntimeStore>(&self, db: &T) -> Result<(), SendableError> {
        let next = load_server_settings(db).await?;
        *self
            .0
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = next;
        Ok(())
    }
}

/// Refresh the shared snapshot so UI changes take effect without restarting server or worker
/// replicas. The refresh interval itself is read from the current snapshot.
pub async fn run_server_settings_refresher<T: RuntimeStore>(
    db: Arc<T>,
    settings: ServerSettingsHandle,
    shutdown: Arc<Notify>,
) {
    loop {
        let delay = Duration::from_secs(
            settings
                .current()
                .orchestration
                .settings_refresh_interval_seconds,
        );
        tokio::select! {
            _ = shutdown.notified() => return,
            _ = tokio::time::sleep(delay) => {
                if let Err(error) = settings.refresh(db.as_ref()).await {
                    log::warn!("failed to refresh server settings: {error}");
                }
            }
        }
    }
}

#[cfg(test)]
#[path = "settings_tests.rs"]
mod tests;
