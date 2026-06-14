//! authentication primitives + the request-gating middleware. authorization (resource grants) is a
//! later phase; phase 1 enforces "is this a valid principal?" plus an `is_admin` flag.

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
}

// ---- request gating ----

/// paths reachable without a credential.
fn is_public_path(path: &str) -> bool {
    matches!(
        path,
        "/health"
            | "/ready"
            | "/openapi.json"
            | "/docs"
            | "/auth/config"
            | "/auth/login"
            | "/auth/refresh"
    )
}

/// pull a presented credential from `Authorization: Bearer …`, `X-Api-Key`, or `?token=` (the last
/// for browser WebSocket upgrades, which cannot set headers).
fn extract_credential(req: &Request<Body>) -> Option<String> {
    if let Some(value) = req
        .headers()
        .get(AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
    {
        if let Some(rest) = value.strip_prefix("Bearer ") {
            return Some(rest.trim().to_string());
        }
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
        req.extensions_mut().insert(AuthContext::disabled_admin());
        return next.run(req).await;
    }
    if is_public_path(req.uri().path()) {
        return next.run(req).await;
    }
    let Some(presented) = extract_credential(&req) else {
        return unauthorized("missing credential");
    };
    let Some(context) = runinator_auth::resolve_credential(&state.config, &state, &presented).await
    else {
        return unauthorized("invalid or expired credential");
    };
    req.extensions_mut().insert(context);
    next.run(req).await
}

fn unauthorized(message: &str) -> Response {
    (StatusCode::UNAUTHORIZED, message.to_string()).into_response()
}
