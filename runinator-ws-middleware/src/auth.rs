//! authentication primitives + the request-gating middleware. authorization (resource grants) is a
//! Authentication resolves a credential to a live principal and current RBAC assignments.

use std::sync::Arc;

use axum::{
    body::Body,
    extract::State,
    http::{Request, StatusCode, header::AUTHORIZATION},
    middleware::Next,
    response::{IntoResponse, Response},
};
use runinator_database::interfaces::DatabaseImpl;
use runinator_models::auth::AuthContext;
use uuid::Uuid;

// the crypto/token primitives live in the transport-agnostic `runinator-auth` crate; re-export the
// surface callers expect from `crate::auth` so handlers/router/server stay unchanged.
pub use runinator_auth::{
    AuthConfig, AuthOptions, hash_password, hash_secret, issue_access_token, new_api_key,
    new_refresh_token, verify_password,
};

/// state threaded into the auth middleware: config + db for api-key/session lookups.
pub struct AuthState<T: DatabaseImpl> {
    pub config: Arc<AuthConfig>,
    pub db: Arc<T>,
}

// manual Clone: the fields are `Arc`, so cloning never requires `T: Clone` (the derive would).
impl<T: DatabaseImpl> Clone for AuthState<T> {
    fn clone(&self) -> Self {
        Self {
            config: self.config.clone(),
            db: self.db.clone(),
        }
    }
}

// bridge the database to the auth library's storage trait so credential resolution lives in the lib.
impl<T: DatabaseImpl> runinator_auth::CredentialStore for AuthState<T> {
    async fn api_key_by_prefix(
        &self,
        prefix: String,
    ) -> Option<runinator_models::auth::ApiKeyRecord> {
        self.db.fetch_api_key_by_prefix(prefix).await.ok().flatten()
    }

    async fn touch_api_key(&self, id: Uuid, last_used_at: i64) {
        let _ = self.db.touch_api_key(id, last_used_at).await;
    }

    async fn user_by_id(&self, id: Uuid) -> Option<runinator_models::auth::User> {
        self.db.fetch_user(id).await.ok().flatten()
    }

    async fn session_by_id(&self, id: Uuid) -> Option<runinator_models::auth::AuthSession> {
        self.db.fetch_session(id).await.ok().flatten()
    }

    async fn service_account_by_id(
        &self,
        id: Uuid,
    ) -> Option<runinator_models::rbac::ServiceAccount> {
        self.db.fetch_service_account(id).await.ok().flatten()
    }

    async fn role_assignments(
        &self,
        kind: runinator_models::auth::PrincipalKind,
        id: Uuid,
    ) -> Option<Vec<runinator_models::rbac::RoleAssignment>> {
        self.db.list_principal_role_assignments(kind, id).await.ok()
    }
}

// ---- request gating ----

/// paths reachable without a credential.
fn is_public_path(path: &str) -> bool {
    matches!(
        path,
        "/health"
            | "/ready"
            | "/metrics"
            | "/openapi.json"
            | "/docs"
            | "/auth/config"
            | "/auth/login"
            | "/auth/refresh"
            | "/agents/enroll"
    )
}

/// pull a presented credential from `Authorization: Bearer …`, `X-Api-Key`, or `?token=` (the last
/// for browser WebSocket upgrades, which cannot set headers).
fn extract_credential(req: &Request<Body>) -> Option<String> {
    if let Some(value) = req
        .headers()
        .get(AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        && let Some(rest) = value.strip_prefix("Bearer ")
    {
        return Some(rest.trim().to_string());
    }
    if let Some(value) = req.headers().get("x-api-key").and_then(|v| v.to_str().ok()) {
        return Some(value.trim().to_string());
    }
    req.uri()
        .query()
        .and_then(|query| url_query_value(query, "token"))
}

fn url_query_value(query: &str, key: &str) -> Option<String> {
    query.split('&').find_map(|pair| {
        let (k, v) = pair.split_once('=')?;
        (k == key).then(|| v.to_string())
    })
}

/// gate every non-public request. when auth is disabled, inject a synthetic admin so existing
/// behavior is unchanged.
pub async fn auth_middleware<T: DatabaseImpl>(
    State(state): State<AuthState<T>>,
    mut req: Request<Body>,
    next: Next,
) -> Response {
    if !state.config.enabled {
        req.extensions_mut()
            .insert(AuthContext::disabled_platform_admin());
        return next.run(req).await;
    }
    if is_public_path(req.uri().path()) {
        return next.run(req).await;
    }
    let Some(presented) = extract_credential(&req) else {
        return unauthorized("missing credential");
    };
    let Some(mut context) =
        runinator_auth::resolve_credential(&state.config, &state, &presented).await
    else {
        return unauthorized("invalid or expired credential");
    };
    // jwt principals carry their active org in the token; api-key/service principals select one per
    // request via `X-Org-Id`. resolve the header's org here so downstream handlers see org context.
    if context.org_id.is_none()
        && let Some(org_id) = req
            .headers()
            .get("x-org-id")
            .and_then(|v| v.to_str().ok())
            .and_then(|raw| Uuid::parse_str(raw.trim()).ok())
    {
        resolve_header_org(&state, &mut context, org_id).await;
    }
    req.extensions_mut().insert(context);
    next.run(req).await
}

/// Bind an `X-Org-Id` only when a live assignment authorizes that organization.
async fn resolve_header_org<T: DatabaseImpl>(
    _state: &AuthState<T>,
    context: &mut AuthContext,
    org_id: Uuid,
) {
    if context.platform_role == Some(runinator_models::rbac::PlatformRole::Admin)
        || context.assignments.iter().any(|assignment| {
            assignment.scope.kind == runinator_models::rbac::ScopeKind::Organization
                && assignment.scope.id == Some(org_id)
        })
    {
        context.org_id = Some(org_id);
    }
}

fn unauthorized(message: &str) -> Response {
    (StatusCode::UNAUTHORIZED, message.to_string()).into_response()
}
