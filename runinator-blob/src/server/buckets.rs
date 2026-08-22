//! the bucket-level S3 operations.

use axum::extract::{Path, State};
use axum::http::{HeaderMap, Method, StatusCode, Uri};
use axum::response::{IntoResponse, Response};
use std::sync::Arc;

use runinator_blob_core::{BlobError, ListRequest};

use super::reply::{xml_response, BlobRejection};
use super::{auth, xml, BlobService};

/// `GET /` — list the buckets this store holds.
pub async fn list_buckets(
    State(service): State<Arc<BlobService>>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
) -> Response {
    let result = async {
        service.begin(&method, &uri, &headers).await?;
        let buckets = service.buckets().await?;
        Ok::<_, BlobError>(xml_response(StatusCode::OK, xml::list_buckets(&buckets)))
    }
    .await;
    unwrap(result, "/")
}

/// `PUT /{bucket}` creates a bucket. Repeating it is safe for the owner, as in S3.
pub async fn put_bucket(
    State(service): State<Arc<BlobService>>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    Path(bucket): Path<String>,
) -> Response {
    let resource = format!("/{bucket}");
    let result = async {
        service.begin(&method, &uri, &headers).await?;
        service.store.create_bucket(&bucket).await?;
        Ok::<_, BlobError>(
            (
                StatusCode::OK,
                [(axum::http::header::LOCATION, resource.clone())],
            )
                .into_response(),
        )
    }
    .await;
    unwrap(result, format!("/{bucket}"))
}

/// `HEAD /{bucket}` — does this bucket exist?
pub async fn head_bucket(
    State(service): State<Arc<BlobService>>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    Path(bucket): Path<String>,
) -> Response {
    let resource = format!("/{bucket}");
    let result = async {
        service.begin(&method, &uri, &headers).await?;
        if !service.store.bucket_exists(&bucket).await? {
            return Err(BlobError::NoSuchBucket(bucket.clone()));
        }
        Ok::<_, BlobError>(StatusCode::OK.into_response())
    }
    .await;
    unwrap(result, resource)
}

/// `DELETE /{bucket}` — remove an empty bucket.
pub async fn delete_bucket(
    State(service): State<Arc<BlobService>>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    Path(bucket): Path<String>,
) -> Response {
    let resource = format!("/{bucket}");
    let result = async {
        service.begin(&method, &uri, &headers).await?;
        service.store.delete_bucket(&bucket).await?;
        Ok::<_, BlobError>(StatusCode::NO_CONTENT.into_response())
    }
    .await;
    unwrap(result, resource)
}

/// `GET /{bucket}` — list objects. only the v2 shape is served; a v1 caller gets the same document,
/// which differs from what it asked for in ways no runinator client depends on.
pub async fn list_objects(
    State(service): State<Arc<BlobService>>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    Path(bucket): Path<String>,
) -> Response {
    let resource = format!("/{bucket}");
    let result = async {
        service.begin(&method, &uri, &headers).await?;
        let query = auth::decode_query(uri.query().unwrap_or(""));
        let request = ListRequest {
            prefix: param(&query, "prefix").filter(|value| !value.is_empty()),
            delimiter: param(&query, "delimiter").filter(|value| !value.is_empty()),
            // `continuation-token` is the v2 cursor; `start-after` is the v1 spelling of the same
            // "resume after this key" idea, and the backend treats them identically.
            continuation_token: param(&query, "continuation-token")
                .or_else(|| param(&query, "start-after"))
                .filter(|value| !value.is_empty()),
            max_keys: param(&query, "max-keys").and_then(|value| value.parse().ok()),
        };
        let response = service.store.list(&bucket, &request).await?;
        Ok::<_, BlobError>(xml_response(
            StatusCode::OK,
            xml::list_objects_v2(
                &bucket,
                request.prefix.as_deref(),
                request.delimiter.as_deref(),
                request.effective_max_keys(),
                &response,
            ),
        ))
    }
    .await;
    unwrap(result, resource)
}

fn param(query: &[(String, String)], name: &str) -> Option<String> {
    query
        .iter()
        .find(|(candidate, _)| candidate == name)
        .map(|(_, value)| value.clone())
}

fn unwrap(result: Result<Response, BlobError>, resource: impl Into<String>) -> Response {
    match result {
        Ok(response) => response,
        Err(error) => BlobRejection::new(error, resource).into_response(),
    }
}
