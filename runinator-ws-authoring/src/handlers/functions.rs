//! packaged functions: publishing code, promoting it, and serving the artifact a worker runs.
//!
//! the two halves of the surface have different shapes on purpose. everything about packages,
//! versions, exports, and aliases is ordinary json; the artifact endpoints move raw bytes, because
//! a package archive is a blob the client already addressed by digest and buffering it into a json
//! envelope on the way past would double it in memory for no gain.
//!
//! upload comes before publish rather than in one request: the digest is computed client-side, so a
//! republish of unchanged code can skip the upload entirely once the server says it already holds
//! those bytes.

use std::sync::Arc;

use axum::{
    Extension, Json,
    body::Body,
    extract::Path,
    http::{StatusCode, header},
    response::{IntoResponse, Response},
};
use runinator_blob_core::BlobStore;
use runinator_database::interfaces::DatabaseImpl;
use runinator_models::{
    auth::AuthContext,
    capabilities::Capability,
    functions::{
        ARTIFACT_MEDIA_TYPE, FunctionPackage, FunctionVersionRef, NewFunctionVersion,
        is_valid_digest,
    },
};
use serde::Deserialize;
use uuid::Uuid;

use crate::repository;
use runinator_ws_core::models::ApiResponse;
use runinator_ws_core::openapi::docs::{EndpointDoc, Example, endpoint, json_body};
use runinator_ws_core::responses::{api_error, bad_request, not_found};
use runinator_ws_middleware::authz::AuthContextExt;

/// list packages visible to the caller.
pub async fn get_functions<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Extension(ctx): Extension<AuthContext>,
) -> (StatusCode, Json<ApiResponse>) {
    match repository::functions::fetch_packages(db.as_ref()).await {
        Ok(packages) => {
            let packages: Vec<_> = packages
                .into_iter()
                .filter(|package| package.archived_at.is_none() && ctx.org_visible(package.org_id))
                .collect();
            (
                StatusCode::OK,
                Json(ApiResponse::FunctionPackageList(packages)),
            )
        }
        Err(err) => api_error(err.to_string()),
    }
}

/// the flattened catalog of every published export.
///
/// this is what an offline compile is handed, so it lists every *version*'s exports rather than
/// only the current release: a workflow pinned to version 2 must still type-check after version 3
/// ships.
pub async fn get_function_catalog<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Extension(ctx): Extension<AuthContext>,
) -> (StatusCode, Json<ApiResponse>) {
    let packages = match repository::functions::fetch_packages(db.as_ref()).await {
        Ok(packages) => packages,
        Err(err) => return api_error(err.to_string()),
    };
    let visible: Vec<Uuid> = packages
        .iter()
        .filter(|package| ctx.org_visible(package.org_id))
        .map(|package| package.id)
        .collect();
    match repository::functions::fetch_catalog(db.as_ref()).await {
        Ok(entries) => {
            let entries: Vec<_> = entries
                .into_iter()
                .filter(|entry| visible.contains(&entry.package_id))
                .collect();
            (StatusCode::OK, Json(ApiResponse::FunctionCatalog(entries)))
        }
        Err(err) => api_error(err.to_string()),
    }
}

/// publish one version of a package.
pub async fn publish_function<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Extension(ctx): Extension<AuthContext>,
    Json(mut request): Json<NewFunctionVersion>,
) -> (StatusCode, Json<ApiResponse>) {
    if let Err(reply) = ctx.require_capability(Capability::FunctionsManage) {
        return reply;
    }
    // the owning org is the caller's, never the request's: a manifest that named an org would be
    // publishing into a tenant the publisher may not belong to.
    request.package.org_id = ctx.org_id;
    if !is_valid_digest(&request.artifact_digest) {
        return bad_request(format!(
            "'{}' is not a sha256 artifact digest",
            request.artifact_digest
        ));
    }
    if request.exports.is_empty() {
        return bad_request("a version must declare at least one export");
    }
    match repository::functions::publish_version(db.as_ref(), &request).await {
        Ok(version) => (StatusCode::OK, Json(ApiResponse::FunctionVersion(version))),
        Err(err) => api_error(err.to_string()),
    }
}

/// one package with its versions, aliases, and current exports.
pub async fn get_function<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Extension(ctx): Extension<AuthContext>,
    Path(package): Path<String>,
) -> (StatusCode, Json<ApiResponse>) {
    let (namespace, name) = split_qualified(&package);
    match repository::functions::fetch_package_detail(
        db.as_ref(),
        ctx.org_id,
        namespace.as_deref(),
        &name,
    )
    .await
    {
        // an archived package reads as absent, the same way the list endpoint filters it out.
        // deleting is an archive rather than a row removal so a restore can bring it back, but that
        // is a property of how deletion is *stored* — to every reader the package is gone, and a
        // deleted package that still answered a fetch would be a delete that did not delete.
        Ok(Some(detail))
            if !ctx.org_visible(detail.package.org_id) || detail.package.archived_at.is_some() =>
        {
            not_found(format!("function package '{package}' not found"))
        }
        Ok(Some(detail)) => (StatusCode::OK, Json(ApiResponse::FunctionPackage(detail))),
        Ok(None) => not_found(format!("function package '{package}' not found")),
        Err(err) => api_error(err.to_string()),
    }
}

/// delete a package and everything under it.
pub async fn delete_function<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Extension(ctx): Extension<AuthContext>,
    Path(package): Path<String>,
) -> (StatusCode, Json<ApiResponse>) {
    if let Err(reply) = ctx.require_capability(Capability::FunctionsManage) {
        return reply;
    }
    let found = match resolve_package(db.as_ref(), &ctx, &package).await {
        Ok(Some(found)) => found,
        Ok(None) => return not_found(format!("function package '{package}' not found")),
        Err(err) => return api_error(err.to_string()),
    };
    match repository::functions::delete_package(db.as_ref(), found.id).await {
        Ok(true) => (
            StatusCode::OK,
            Json(ApiResponse::JsonValue(runinator_models::json!({
                "archived": true,
                "package": package,
            }))),
        ),
        Ok(false) => not_found(format!("function package '{package}' not found")),
        Err(err) => api_error(err.to_string()),
    }
}

/// restore an archived package.
pub async fn restore_function<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Extension(ctx): Extension<AuthContext>,
    Path(package): Path<String>,
) -> (StatusCode, Json<ApiResponse>) {
    if let Err(reply) = ctx.require_capability(Capability::FunctionsManage) {
        return reply;
    }
    let (namespace, name) = split_qualified(&package);
    let found = match db
        .fetch_function_package(ctx.org_id, namespace.as_deref(), &name)
        .await
    {
        Ok(Some(found)) if found.archived_at.is_some() && ctx.org_visible(found.org_id) => found,
        Ok(_) => return not_found(format!("archived function package '{package}' not found")),
        Err(err) => return api_error(err.to_string()),
    };
    match repository::functions::restore_package(db.as_ref(), found.id).await {
        Ok(true) => (
            StatusCode::OK,
            Json(ApiResponse::JsonValue(runinator_models::json!({
                "restored": true,
                "package": package,
            }))),
        ),
        Ok(false) => not_found(format!("archived function package '{package}' not found")),
        Err(err) => api_error(err.to_string()),
    }
}

/// what a caller sends to move an alias.
#[derive(Debug, Deserialize)]
pub struct SetFunctionAliasRequest {
    pub alias: String,
    /// the version to point at, by number. omitted means the newest version.
    #[serde(default)]
    pub version: Option<i64>,
    /// or by another alias, so `production` can be pointed at whatever `latest` currently is.
    #[serde(default)]
    pub from_alias: Option<String>,
}

/// point an alias at a version.
pub async fn set_function_alias<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Extension(ctx): Extension<AuthContext>,
    Path(package): Path<String>,
    Json(request): Json<SetFunctionAliasRequest>,
) -> (StatusCode, Json<ApiResponse>) {
    if let Err(reply) = ctx.require_capability(Capability::FunctionsManage) {
        return reply;
    }
    let found = match resolve_package(db.as_ref(), &ctx, &package).await {
        Ok(Some(found)) => found,
        Ok(None) => return not_found(format!("function package '{package}' not found")),
        Err(err) => return api_error(err.to_string()),
    };
    let target = match (&request.version, &request.from_alias) {
        (Some(_), Some(_)) => {
            return bad_request("name the target by version or by from_alias, not both");
        }
        (Some(version), None) => FunctionVersionRef::Exact(*version),
        (None, Some(alias)) => FunctionVersionRef::Alias(alias.clone()),
        // no target at all means the newest published version, which is what a release promotion
        // reaches for and saves the caller a round trip to look the number up.
        (None, None) => match newest_version(db.as_ref(), found.id).await {
            Ok(Some(version)) => FunctionVersionRef::Exact(version),
            Ok(None) => return bad_request("package has no published versions"),
            Err(err) => return api_error(err.to_string()),
        },
    };
    match repository::functions::set_alias(db.as_ref(), found.id, &request.alias, &target).await {
        Ok(alias) => (StatusCode::OK, Json(ApiResponse::FunctionAlias(alias))),
        Err(err) => api_error(err.to_string()),
    }
}

/// delete an alias, leaving the version it named untouched.
pub async fn delete_function_alias<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Extension(ctx): Extension<AuthContext>,
    Path((package, alias)): Path<(String, String)>,
) -> (StatusCode, Json<ApiResponse>) {
    if let Err(reply) = ctx.require_capability(Capability::FunctionsManage) {
        return reply;
    }
    let found = match resolve_package(db.as_ref(), &ctx, &package).await {
        Ok(Some(found)) => found,
        Ok(None) => return not_found(format!("function package '{package}' not found")),
        Err(err) => return api_error(err.to_string()),
    };
    match repository::functions::delete_alias(db.as_ref(), found.id, &alias).await {
        Ok(true) => (
            StatusCode::OK,
            Json(ApiResponse::JsonValue(runinator_models::json!({
                "deleted": true,
                "alias": alias,
            }))),
        ),
        Ok(false) => not_found(format!("alias '{alias}' not found")),
        Err(err) => api_error(err.to_string()),
    }
}

/// everything a worker needs to run one export: its handler, runtime, limits, and artifact digest.
///
/// the invocation path's single read. a version is immutable, so a worker caches the answer for as
/// long as it caches the code, and a promotion never changes what an already-dispatched action runs.
pub async fn resolve_function_export<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Extension(ctx): Extension<AuthContext>,
    Path(export_id): Path<Uuid>,
) -> (StatusCode, Json<ApiResponse>) {
    if let Err(reply) = ctx.require_agent_service_or_admin() {
        return reply;
    }
    if !function_export_visible(db.as_ref(), &ctx, export_id).await {
        return not_found(format!("function export {export_id} not found"));
    }
    match repository::functions::resolve_invocation_target(db.as_ref(), export_id).await {
        Ok(Some(target)) => (
            StatusCode::OK,
            Json(ApiResponse::FunctionInvocationTarget(Box::new(target))),
        ),
        Ok(None) => not_found(format!("function export {export_id} not found")),
        Err(err) => api_error(err.to_string()),
    }
}

/// the artifact record for a digest, or 404. this is the "do I need to upload?" probe.
pub async fn get_function_artifact<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Extension(ctx): Extension<AuthContext>,
    Path(digest): Path<String>,
) -> (StatusCode, Json<ApiResponse>) {
    if let Err(reply) = ctx.require_capability(Capability::FunctionsManage) {
        return reply;
    }
    if !is_valid_digest(&digest) {
        return bad_request(format!("'{digest}' is not a sha256 artifact digest"));
    }
    match repository::functions::fetch_artifact(db.as_ref(), &digest).await {
        Ok(Some(artifact)) => (
            StatusCode::OK,
            Json(ApiResponse::FunctionArtifact(artifact)),
        ),
        Ok(None) => not_found(format!("artifact {digest} not found")),
        Err(err) => api_error(err.to_string()),
    }
}

/// upload package bytes under their digest. re-uploading bytes already stored is a no-op.
pub async fn upload_function_artifact<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Extension(blobs): Extension<Arc<dyn BlobStore>>,
    Extension(ctx): Extension<AuthContext>,
    Path(digest): Path<String>,
    body: axum::body::Bytes,
) -> (StatusCode, Json<ApiResponse>) {
    if let Err(reply) = ctx.require_capability(Capability::FunctionsManage) {
        return reply;
    }
    if !is_valid_digest(&digest) {
        return bad_request(format!("'{digest}' is not a sha256 artifact digest"));
    }
    match repository::functions::put_artifact_if_absent(db.as_ref(), &blobs, &digest, body.to_vec())
        .await
    {
        Ok(artifact) => (
            StatusCode::OK,
            Json(ApiResponse::FunctionArtifact(artifact)),
        ),
        Err(err) => api_error(err.to_string()),
    }
}

/// stream an artifact's bytes. this is the worker's fetch path.
pub async fn download_function_artifact<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Extension(blobs): Extension<Arc<dyn BlobStore>>,
    Extension(ctx): Extension<AuthContext>,
    Path(digest): Path<String>,
) -> Response {
    if let Err(reply) = ctx.require_agent_service_or_admin() {
        return reply.into_response();
    }
    if !is_valid_digest(&digest) {
        return (
            StatusCode::BAD_REQUEST,
            format!("'{digest}' is not a sha256 artifact digest"),
        )
            .into_response();
    }
    if !function_artifact_visible(db.as_ref(), &ctx, &digest).await {
        return (StatusCode::NOT_FOUND, "artifact not found").into_response();
    }
    let content =
        match repository::functions::open_artifact(db.as_ref(), &blobs, &digest, None).await {
            Ok(content) => content,
            Err(err) => return (StatusCode::NOT_FOUND, err.to_string()).into_response(),
        };
    let length = content.size_bytes;
    let body = Body::from_stream(tokio_util::io::ReaderStream::new(content.body));
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, ARTIFACT_MEDIA_TYPE)
        .header(header::CONTENT_LENGTH, length)
        // the digest names the bytes, so the response is immutable by construction and a worker's
        // cache never needs to revalidate it.
        .header(header::CACHE_CONTROL, "public, max-age=31536000, immutable")
        .header(header::ETAG, format!("\"{digest}\""))
        .body(body)
        .unwrap_or_else(|_| {
            (StatusCode::INTERNAL_SERVER_ERROR, "response build failed").into_response()
        })
}

async fn function_export_visible<T: DatabaseImpl>(
    db: &T,
    ctx: &AuthContext,
    export_id: Uuid,
) -> bool {
    let Ok(Some(package)) = repository::functions::fetch_export_package(db, export_id).await else {
        return false;
    };
    ctx.org_visible(package.org_id)
}

async fn function_artifact_visible<T: DatabaseImpl>(
    db: &T,
    ctx: &AuthContext,
    digest: &str,
) -> bool {
    let Ok(packages) = repository::functions::packages_with_artifact(db, digest).await else {
        return false;
    };
    packages
        .iter()
        .any(|package| ctx.org_visible(package.org_id))
}

// `namespace.name` or a bare `name`. names cannot contain dots (the manifest rejects them), so this
// split is unambiguous and the dotted call path stays parseable in the same way everywhere.
fn split_qualified(package: &str) -> (Option<String>, String) {
    match package.split_once('.') {
        Some((namespace, name)) => (Some(namespace.to_string()), name.to_string()),
        None => (None, package.to_string()),
    }
}

async fn resolve_package<T: DatabaseImpl>(
    db: &T,
    ctx: &AuthContext,
    package: &str,
) -> Result<Option<FunctionPackage>, runinator_models::errors::SendableError> {
    let (namespace, name) = split_qualified(package);
    let found =
        repository::functions::fetch_package_detail(db, ctx.org_id, namespace.as_deref(), &name)
            .await?;
    Ok(found
        .map(|detail| detail.package)
        .filter(|package| package.archived_at.is_none() && ctx.org_visible(package.org_id)))
}

async fn newest_version<T: DatabaseImpl>(
    db: &T,
    package_id: Uuid,
) -> Result<Option<i64>, runinator_models::errors::SendableError> {
    let detail = repository::functions::fetch_package_versions(db, package_id).await?;
    Ok(detail.first().map(|version| version.version))
}

/// the `functions` endpoints.
pub fn routes<T: DatabaseImpl>(pool: std::sync::Arc<T>) -> axum::Router {
    use axum::Extension;
    use axum::routing::{delete, get, post};
    axum::Router::new()
        .route(
            "/functions",
            get(get_functions::<T>)
                .post(publish_function::<T>)
                .layer(Extension(pool.clone())),
        )
        .route(
            "/functions/catalog",
            get(get_function_catalog::<T>).layer(Extension(pool.clone())),
        )
        .route(
            "/functions/{package}",
            get(get_function::<T>)
                .delete(delete_function::<T>)
                .layer(Extension(pool.clone())),
        )
        .route(
            "/functions/{package}/restore",
            post(restore_function::<T>).layer(Extension(pool.clone())),
        )
        .route(
            "/functions/{package}/aliases",
            post(set_function_alias::<T>).layer(Extension(pool.clone())),
        )
        .route(
            "/functions/{package}/aliases/{alias}",
            delete(delete_function_alias::<T>).layer(Extension(pool.clone())),
        )
        .route(
            "/function_exports/{export_id}",
            get(resolve_function_export::<T>).layer(Extension(pool.clone())),
        )
        .route(
            "/function_artifacts/{digest}",
            get(get_function_artifact::<T>)
                .post(upload_function_artifact::<T>)
                .layer(Extension(pool.clone())),
        )
        .route(
            "/function_artifacts/{digest}/content",
            get(download_function_artifact::<T>).layer(Extension(pool.clone())),
        )
}

/// the openapi entries for the routes above.
pub const DOCS: &[EndpointDoc] = &[
    endpoint(
        "get",
        "/functions",
        "Functions",
        "List function packages",
        "Lists packaged-function packages visible to the caller.",
        false,
        None,
        &[],
        200,
        "function packages",
        Example::FunctionPackageList,
    ),
    endpoint(
        "post",
        "/functions",
        "Functions",
        "Publish a function version",
        "Publishes one immutable version of a package. The artifact named by `artifact_digest` \
         must already be uploaded.",
        false,
        json_body("Publish request.", Example::FunctionPublish),
        &[],
        200,
        "version published",
        Example::FunctionVersion,
    ),
    endpoint(
        "get",
        "/functions/catalog",
        "Functions",
        "List published exports",
        "Lists every published export as a flattened catalog entry, which is what an offline \
         compile is handed to type a packaged-function call.",
        false,
        None,
        &[],
        200,
        "catalog entries",
        Example::FunctionCatalog,
    ),
    endpoint(
        "get",
        "/functions/{package}",
        "Functions",
        "Show a function package",
        "Returns one package with its versions, aliases, and the exports of its default alias.",
        false,
        None,
        &[],
        200,
        "function package",
        Example::FunctionPackage,
    ),
    endpoint(
        "delete",
        "/functions/{package}",
        "Functions",
        "Delete a function package",
        "Archives a package and every version, export, and alias under it. The rows are kept so \
         the package can be restored; to every reader it is gone.",
        false,
        None,
        &[],
        200,
        "package deleted",
        Example::None,
    ),
    endpoint(
        "post",
        "/functions/{package}/restore",
        "Functions",
        "Restore a deleted function package",
        "Reactivates a package that was deleted, along with its versions, exports, and aliases. \
         Fails with 404 when no archived package of that name exists.",
        false,
        None,
        &[],
        200,
        "package restored",
        Example::None,
    ),
    endpoint(
        "post",
        "/functions/{package}/aliases",
        "Functions",
        "Move an alias",
        "Points an alias at a version, by number, by another alias, or at the newest version.",
        false,
        json_body("Alias target.", Example::FunctionAliasRequest),
        &[],
        200,
        "alias moved",
        Example::FunctionAlias,
    ),
    endpoint(
        "delete",
        "/functions/{package}/aliases/{alias}",
        "Functions",
        "Delete an alias",
        "Deletes an alias. The version it pointed at is untouched.",
        false,
        None,
        &[],
        200,
        "alias deleted",
        Example::None,
    ),
    endpoint(
        "get",
        "/function_exports/{export_id}",
        "Functions",
        "Resolve an export",
        "Returns the handler, runtime, limits, and artifact digest a worker needs to run one \
         export. A version is immutable, so this answer is cacheable indefinitely.",
        false,
        None,
        &[],
        200,
        "invocation target",
        Example::FunctionInvocationTarget,
    ),
    endpoint(
        "get",
        "/function_artifacts/{digest}",
        "Functions",
        "Show an artifact",
        "Returns the artifact record for a digest, or 404 when those bytes have not been uploaded.",
        false,
        None,
        &[],
        200,
        "artifact record",
        Example::FunctionArtifact,
    ),
    endpoint(
        "post",
        "/function_artifacts/{digest}",
        "Functions",
        "Upload an artifact",
        "Uploads package archive bytes under their sha-256 digest. The digest is verified against \
         the bytes; re-uploading bytes already stored is a no-op.",
        false,
        Some(runinator_ws_core::openapi::docs::RequestDoc {
            description: "The package archive, as raw bytes.",
            example: Example::None,
            content_type: ARTIFACT_MEDIA_TYPE,
        }),
        &[],
        200,
        "artifact stored",
        Example::FunctionArtifact,
    ),
    endpoint(
        "get",
        "/function_artifacts/{digest}/content",
        "Functions",
        "Download an artifact",
        "Streams a package archive's bytes. This is the path a worker fetches code from.",
        false,
        None,
        &[],
        200,
        "artifact bytes",
        Example::None,
    ),
];
