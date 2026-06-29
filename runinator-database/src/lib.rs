use std::sync::Arc;

use chrono::Utc;
use interfaces::DatabaseImpl;
use log::{info, warn};
use runinator_models::auth::{ApiKey, ApiKeyRecord};
use runinator_models::errors::SendableError;
use runinator_models::settings::SettingKind;
use runinator_utilities::secret_cipher::SecretCipher;
use uuid::Uuid;

pub mod archive;
pub mod backend;
mod common;
pub mod errors;
pub mod interfaces;
mod mappers;
pub mod mysql;
mod operations;
pub mod postgres;
mod queries;
pub mod sqlite;

#[derive(Debug, Clone, Default)]
pub struct BootstrapOptions {
    pub auth_jwt_secret: Option<String>,
    /// previous jwt signing secret accepted on verify during a rotation overlap window. an empty/None
    /// value clears any persisted previous secret, retiring the old key.
    pub auth_jwt_secret_previous: Option<String>,
    pub auth_bootstrap_admin: Option<String>,
    /// reconcile (reset) the bootstrap admin password even when users already exist.
    pub auth_bootstrap_admin_force: bool,
    pub auth_bootstrap_service_api_key: Option<String>,
    pub auth_bootstrap_service_api_key_name: Option<String>,
}

pub async fn bootstrap_database(
    pool: &Arc<impl DatabaseImpl>,
    options: &BootstrapOptions,
) -> Result<(), SendableError> {
    info!("Run bootstrap scripts");
    let scripts: Vec<String> = Vec::new();
    pool.run_init_scripts(&scripts).await?;
    ensure_jwt_secret(pool.as_ref(), options.auth_jwt_secret.clone()).await?;
    ensure_jwt_secret_previous(pool.as_ref(), options.auth_jwt_secret_previous.clone()).await?;
    if let Some(spec) = options.auth_bootstrap_admin.as_deref() {
        seed_bootstrap_admin(pool.as_ref(), spec, options.auth_bootstrap_admin_force).await?;
    }
    if let Some(raw_key) = options.auth_bootstrap_service_api_key.as_deref() {
        seed_bootstrap_service_api_key(
            pool.as_ref(),
            options
                .auth_bootstrap_service_api_key_name
                .as_deref()
                .unwrap_or(DEFAULT_BOOTSTRAP_SERVICE_API_KEY_NAME),
            raw_key,
        )
        .await?;
    }
    Ok(())
}

/// settings-store coordinates for the persisted, replica-shared signing secret.
const SECRET_SCOPE: &str = "auth";
const SECRET_NAME: &str = "jwt_secret";
const SECRET_NAME_PREVIOUS: &str = "jwt_secret_previous";
const DEFAULT_BOOTSTRAP_SERVICE_API_KEY_NAME: &str = "bootstrap-service";

// the cipher protecting persisted auth secrets at rest, keyed from the environment
// (`RUNINATOR_CREDENTIAL_KEY` plus rotation-overlap keys). it is the same cipher the web service
// uses for user settings, so the jwt signing secret is protected exactly like every other secret.
fn auth_cipher() -> SecretCipher {
    SecretCipher::from_env()
}

// open a persisted auth secret. values written by the current scheme carry the authenticated-
// encryption header and are aead-opened; legacy values written before encryption was applied are
// headerless plaintext and returned as-is (never fed through the legacy xor path, which would
// corrupt a true plaintext).
fn open_auth_secret(cipher: &SecretCipher, value: Vec<u8>) -> Result<Vec<u8>, SendableError> {
    if !SecretCipher::is_sealed(&value) {
        return Ok(value);
    }
    cipher.try_decrypt(&value).ok_or_else(|| {
        Box::new(std::io::Error::other(
            "could not decrypt the persisted jwt secret; the credential key may be missing or wrong",
        )) as SendableError
    })
}

pub async fn ensure_jwt_secret<T: DatabaseImpl>(
    db: &T,
    explicit: Option<String>,
) -> Result<Vec<u8>, SendableError> {
    let cipher = auth_cipher();
    if let Some(secret) = explicit.filter(|s| !s.is_empty()) {
        let bytes = secret.into_bytes();
        db.upsert_setting(
            SettingKind::Secret,
            SECRET_SCOPE.into(),
            SECRET_NAME.into(),
            cipher.encrypt(&bytes),
            Utc::now().timestamp(),
        )
        .await?;
        return Ok(bytes);
    }
    if let Some(record) = db
        .fetch_setting(SettingKind::Secret, SECRET_SCOPE.into(), SECRET_NAME.into())
        .await?
    {
        if !record.value.is_empty() {
            let was_sealed = SecretCipher::is_sealed(&record.value);
            let plaintext = open_auth_secret(&cipher, record.value)?;
            // migrate a legacy plaintext secret to the encrypted-at-rest scheme on first bootstrap.
            if !was_sealed {
                db.upsert_setting(
                    SettingKind::Secret,
                    SECRET_SCOPE.into(),
                    SECRET_NAME.into(),
                    cipher.encrypt(&plaintext),
                    Utc::now().timestamp(),
                )
                .await?;
            }
            return Ok(plaintext);
        }
    }
    let generated = runinator_auth::random_secret(48);
    db.upsert_setting(
        SettingKind::Secret,
        SECRET_SCOPE.into(),
        SECRET_NAME.into(),
        cipher.encrypt(&generated),
        Utc::now().timestamp(),
    )
    .await?;
    Ok(generated)
}

pub async fn load_jwt_secret<T: DatabaseImpl>(db: &T) -> Result<Vec<u8>, SendableError> {
    let record = db
        .fetch_setting(SettingKind::Secret, SECRET_SCOPE.into(), SECRET_NAME.into())
        .await?
        .filter(|record| !record.value.is_empty());
    let Some(record) = record else {
        return Err(Box::new(std::io::Error::other(
            "missing auth jwt secret; run runinator-bootstrap before starting runinator-ws",
        )) as SendableError);
    };
    open_auth_secret(&auth_cipher(), record.value)
}

/// persist or clear the previous jwt signing secret. an explicit non-empty value is upserted; an
/// empty/None value deletes the slot, which retires the old key after the rotation window closes.
pub async fn ensure_jwt_secret_previous<T: DatabaseImpl>(
    db: &T,
    explicit: Option<String>,
) -> Result<(), SendableError> {
    match explicit.filter(|secret| !secret.is_empty()) {
        Some(secret) => {
            db.upsert_setting(
                SettingKind::Secret,
                SECRET_SCOPE.into(),
                SECRET_NAME_PREVIOUS.into(),
                auth_cipher().encrypt(secret.as_bytes()),
                Utc::now().timestamp(),
            )
            .await
        }
        None => {
            db.delete_setting(
                SettingKind::Secret,
                SECRET_SCOPE.into(),
                SECRET_NAME_PREVIOUS.into(),
            )
            .await
        }
    }
}

/// load the optional previous jwt signing secret accepted during a rotation overlap window.
pub async fn load_jwt_secret_previous<T: DatabaseImpl>(
    db: &T,
) -> Result<Option<Vec<u8>>, SendableError> {
    let record = db
        .fetch_setting(
            SettingKind::Secret,
            SECRET_SCOPE.into(),
            SECRET_NAME_PREVIOUS.into(),
        )
        .await?
        .filter(|record| !record.value.is_empty());
    match record {
        Some(record) => Ok(Some(open_auth_secret(&auth_cipher(), record.value)?)),
        None => Ok(None),
    }
}

/// seed the configured bootstrap admin. by default this only provisions the user into an empty user
/// table; `force` reconciles an already-present admin (resetting its password and re-enabling admin),
/// recovering operators locked out by a stale or unknown bootstrap password.
pub async fn seed_bootstrap_admin<T: DatabaseImpl>(
    db: &T,
    spec: &str,
    force: bool,
) -> Result<(), SendableError> {
    let Some((username, password)) = spec.split_once(':') else {
        warn!("RUNINATOR_AUTH_BOOTSTRAP_ADMIN must be 'username:password'; skipping seed");
        return Ok(());
    };

    // an admin with this username already exists; leave operator-managed credentials alone unless forced.
    if let Some(existing) = db.fetch_user_by_username(username.to_string()).await? {
        if !force {
            return Ok(());
        }
        let Some(user_id) = existing.id else {
            warn!("bootstrap admin '{username}' has no id; skipping force reset");
            return Ok(());
        };
        db.set_local_password(user_id, hash_admin_password(password)?)
            .await?;
        db.update_user(user_id, None, Some(true), Some(false))
            .await?;
        info!("Reset bootstrap admin '{username}' password (force).");
        return Ok(());
    }

    // the bootstrap admin is absent. preserve the original guard: only seed into an empty user table,
    // unless force is set, which provisions the admin even alongside existing users.
    if !force && db.count_users().await? > 0 {
        return Ok(());
    }
    db.create_user(
        username.to_string(),
        None,
        true,
        Some(hash_admin_password(password)?),
    )
    .await?;
    info!("Seeded bootstrap admin user '{username}'.");
    Ok(())
}

fn hash_admin_password(password: &str) -> Result<String, SendableError> {
    runinator_auth::hash_password(password)
        .map_err(|err| -> SendableError { Box::new(std::io::Error::other(err)) })
}

pub async fn seed_bootstrap_service_api_key<T: DatabaseImpl>(
    db: &T,
    name: &str,
    raw_key: &str,
) -> Result<(), SendableError> {
    let Some((prefix, _)) = raw_key.split_once('.') else {
        warn!(
            "RUNINATOR_AUTH_BOOTSTRAP_SERVICE_API_KEY must be '<prefix>.<secret>'; skipping seed"
        );
        return Ok(());
    };
    if prefix.is_empty() {
        warn!(
            "RUNINATOR_AUTH_BOOTSTRAP_SERVICE_API_KEY must include a non-empty prefix; skipping seed"
        );
        return Ok(());
    }
    if db
        .fetch_api_key_by_prefix(prefix.to_string())
        .await?
        .is_some()
    {
        return Ok(());
    }

    let record = ApiKeyRecord {
        key: ApiKey {
            id: Some(Uuid::now_v7()),
            name: name.to_string(),
            user_id: None,
            is_service: true,
            key_prefix: prefix.to_string(),
            last_used_at: None,
            expires_at: None,
            disabled: false,
            created_at: Utc::now(),
        },
        is_admin: true,
        key_hash: runinator_auth::hash_secret(raw_key),
    };
    db.create_api_key(record).await?;
    info!("Seeded bootstrap service api key '{name}'");
    Ok(())
}
