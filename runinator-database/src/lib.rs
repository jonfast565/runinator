use std::sync::Arc;

use chrono::Utc;
use log::{info, warn};
use runinator_models::auth::{ApiKey, ApiKeyRecord, PrincipalKind};
use runinator_models::errors::SendableError;
use runinator_models::orgs::OrgRole;
use runinator_models::rbac::{PlatformRole, Role, ScopeRef};
use runinator_models::settings::SettingKind;
use runinator_secrets::secret_cipher::SecretCipher;
use runinator_store::DatabaseImpl;
use uuid::Uuid;

pub mod backend;
#[cfg(test)]
mod backend_tests;
mod common;
#[cfg(all(
    test,
    any(feature = "sqlite", feature = "postgres", feature = "mariadb")
))]
mod dialect_parity;
pub mod errors;
mod mappers;
#[cfg(test)]
#[path = "migration_parity_tests.rs"]
mod migration_parity;

#[cfg(feature = "mariadb")]
pub mod mariadb;
mod operations;
mod pool;
#[cfg(feature = "postgres")]
pub mod postgres;
mod queries;
#[cfg(feature = "sqlite")]
pub mod sqlite;

#[derive(Debug, Clone, Default)]
pub struct BootstrapOptions {
    pub auth_jwt_secret: Option<String>,
    /// Previous JWT signing secret accepted during key rotation. Empty or `None` retires the old key.
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
const PLATFORM_ORGANIZATION_NAME: &str = "Platform";
const PLATFORM_ORGANIZATION_SLUG: &str = "platform";

// the cipher protecting persisted auth secrets at rest, keyed from the environment
// (`RUNINATOR_CREDENTIAL_KEY` plus rotation-overlap keys). it is the same cipher the web service
// It uses the same store as user settings, so the JWT signing secret is protected like every other secret.
fn auth_cipher() -> SecretCipher {
    SecretCipher::from_env()
}

// Open a persisted auth secret sealed by the configured credential key.
fn open_auth_secret(cipher: &SecretCipher, value: Vec<u8>) -> Result<Vec<u8>, SendableError> {
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
            None,
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
        .fetch_setting(
            None,
            SettingKind::Secret,
            SECRET_SCOPE.into(),
            SECRET_NAME.into(),
        )
        .await?
        && !record.value.is_empty()
    {
        let plaintext = open_auth_secret(&cipher, record.value)?;
        return Ok(plaintext);
    }
    let generated = runinator_auth::random_secret(48);
    db.upsert_setting(
        None,
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
        .fetch_setting(
            None,
            SettingKind::Secret,
            SECRET_SCOPE.into(),
            SECRET_NAME.into(),
        )
        .await?
        .filter(|record| !record.value.is_empty());
    let Some(record) = record else {
        return Err(Box::new(std::io::Error::other(
            "missing auth jwt secret; run runinator-bootstrap before starting runinator-ws",
        )) as SendableError);
    };
    open_auth_secret(&auth_cipher(), record.value)
}

/// Save or clear the previous JWT signing secret. A non-empty value replaces it.
/// Empty or `None` deletes it and retires the old key.
pub async fn ensure_jwt_secret_previous<T: DatabaseImpl>(
    db: &T,
    explicit: Option<String>,
) -> Result<(), SendableError> {
    match explicit.filter(|secret| !secret.is_empty()) {
        Some(secret) => {
            db.upsert_setting(
                None,
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
                None,
                SettingKind::Secret,
                SECRET_SCOPE.into(),
                SECRET_NAME_PREVIOUS.into(),
            )
            .await
        }
    }
}

/// Load the optional previous JWT signing secret used during key rotation.
pub async fn load_jwt_secret_previous<T: DatabaseImpl>(
    db: &T,
) -> Result<Option<Vec<u8>>, SendableError> {
    let record = db
        .fetch_setting(
            None,
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
        let Some(user_id) = existing.id else {
            warn!("bootstrap admin '{username}' has no id; skipping seed");
            return Ok(());
        };
        if !force {
            let is_platform_admin = db
                .list_principal_role_assignments(PrincipalKind::User, user_id)
                .await?
                .iter()
                .any(|assignment| assignment.role == Role::Platform(PlatformRole::Admin));
            if is_platform_admin {
                ensure_platform_organization(db, user_id).await?;
            }
            return Ok(());
        }
        db.set_local_password(user_id, hash_admin_password(password)?)
            .await?;
        db.update_user(user_id, None, Some(false)).await?;
        db.upsert_role_assignment(
            PrincipalKind::User,
            user_id,
            ScopeRef::PLATFORM,
            Role::Platform(PlatformRole::Admin),
            None,
        )
        .await?;
        ensure_platform_organization(db, user_id).await?;
        info!("Reset bootstrap admin '{username}' password (force).");
        return Ok(());
    }

    // the bootstrap admin is absent. preserve the original guard: only seed into an empty user table,
    // unless force is set, which provisions the admin even alongside existing users.
    if !force && db.count_users().await? > 0 {
        return Ok(());
    }
    let user = db
        .create_user_with_platform_role(
            username.to_string(),
            None,
            Some(hash_admin_password(password)?),
            PlatformRole::Admin,
            None,
        )
        .await?;
    let Some(user_id) = user.id else {
        return Err(Box::new(std::io::Error::other(
            "bootstrap admin was created without an id",
        )));
    };
    ensure_platform_organization(db, user_id).await?;
    info!("Seeded bootstrap admin user '{username}'.");
    Ok(())
}

/// Ensure the bootstrap administrator has an owner membership in the durable platform org.
/// Retrying the lookup after a failed insert makes parallel bootstrap attempts converge on the
/// same organization instead of treating the unique slug race as a startup failure.
async fn ensure_platform_organization<T: DatabaseImpl>(
    db: &T,
    user_id: Uuid,
) -> Result<(), SendableError> {
    let organization = match db
        .fetch_org_by_slug(PLATFORM_ORGANIZATION_SLUG.into())
        .await?
    {
        Some(organization) => organization,
        None => match db
            .create_org(
                PLATFORM_ORGANIZATION_NAME.into(),
                PLATFORM_ORGANIZATION_SLUG.into(),
            )
            .await
        {
            Ok(organization) => organization,
            Err(error) => db
                .fetch_org_by_slug(PLATFORM_ORGANIZATION_SLUG.into())
                .await?
                .ok_or(error)?,
        },
    };
    let organization_id = organization.id.ok_or_else(|| {
        Box::new(std::io::Error::other(
            "platform organization was created without an id",
        )) as SendableError
    })?;
    db.add_org_member(organization_id, user_id, OrgRole::Owner)
        .await?;
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

    let service = match db
        .list_service_accounts()
        .await?
        .into_iter()
        .find(|account| account.name == name)
    {
        Some(account) => account,
        None => db.create_service_account(name.to_string(), None).await?,
    };
    db.upsert_role_assignment(
        PrincipalKind::Service,
        service.id,
        ScopeRef::PLATFORM,
        Role::Platform(PlatformRole::Admin),
        None,
    )
    .await?;
    let record = ApiKeyRecord {
        key: ApiKey {
            id: Some(Uuid::now_v7()),
            name: name.to_string(),
            principal_kind: PrincipalKind::Service,
            principal_id: service.id,
            system_role: None,
            org_id: None,
            action_ceiling: Vec::new(),
            key_prefix: prefix.to_string(),
            last_used_at: None,
            expires_at: None,
            disabled: false,
            created_at: Utc::now(),
        },
        key_hash: runinator_auth::hash_secret(raw_key),
    };
    db.create_api_key(record).await?;
    info!("Seeded bootstrap service api key '{name}'");
    Ok(())
}
