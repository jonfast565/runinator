//! the object-level S3 operations.
//!
//! S3 puts several operations behind one method and query parameters such as `?uploads`,
//! `?uploadId=`, and `?partNumber=`. Each handler checks the query before doing work.
//! The dispatch is written out because five cases are shorter than a table.

use axum::body::Body;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, Method, StatusCode, Uri};
use axum::response::{IntoResponse, Response};
use std::sync::Arc;

use futures_util::TryStreamExt;

use runinator_blob_core::{
    sha256_from_checksum_header, BlobError, ByteRange, ObjectKey, PutOptions,
};

use super::reply::{object_headers, xml_response, BlobRejection};
use super::{auth, xml, BlobService};

/// `GET /{bucket}/{key}` — read an object, optionally a byte range of it.
pub async fn get_object(
    State(service): State<Arc<BlobService>>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    Path((bucket, key)): Path<(String, String)>,
) -> Response {
    let resource = format!("/{bucket}/{key}");
    let result = async {
        service.begin(&method, &uri, &headers).await?;
        let object_key = ObjectKey::parse(&key)?;
        let range = ByteRange::parse_header(headers.get("range").and_then(|v| v.to_str().ok()))?;
        let reader = service.store.open(&bucket, &object_key, range).await?;
        let status = if reader.range.is_some() {
            StatusCode::PARTIAL_CONTENT
        } else {
            StatusCode::OK
        };
        let response_headers = object_headers(&reader.meta, reader.range);
        let body = Body::from_stream(tokio_util::io::ReaderStream::with_capacity(
            reader.body,
            runinator_blob_core::DEFAULT_IO_BUFFER_BYTES,
        ));
        Ok::<_, BlobError>((status, response_headers, body).into_response())
    }
    .await;
    unwrap(result, resource)
}

/// `HEAD /{bucket}/{key}` — the object's descriptor without its bytes.
pub async fn head_object(
    State(service): State<Arc<BlobService>>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    Path((bucket, key)): Path<(String, String)>,
) -> Response {
    let resource = format!("/{bucket}/{key}");
    let result = async {
        service.begin(&method, &uri, &headers).await?;
        let meta = service
            .store
            .head(&bucket, &ObjectKey::parse(&key)?)
            .await?;
        Ok::<_, BlobError>((StatusCode::OK, object_headers(&meta, None)).into_response())
    }
    .await;
    unwrap(result, resource)
}

/// `PUT /{bucket}/{key}` — store an object, or one part of a multipart upload.
pub async fn put_object(
    State(service): State<Arc<BlobService>>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    Path((bucket, key)): Path<(String, String)>,
    body: Body,
) -> Response {
    let resource = format!("/{bucket}/{key}");
    let result = async {
        let payload = service.begin(&method, &uri, &headers).await?;
        let object_key = ObjectKey::parse(&key)?;
        let query = auth::decode_query(uri.query().unwrap_or(""));
        let declared_length = decoded_content_length(&headers, payload.is_chunked())?;
        if let Some(length) = declared_length {
            if length > service.config.max_object_bytes as u64 {
                return Err(BlobError::BadRequest(format!(
                    "upload declares {length} bytes, above the configured {} byte limit",
                    service.config.max_object_bytes
                )));
            }
        }
        let stream = body.into_data_stream().map_err(std::io::Error::other);
        let base = tokio_util::io::StreamReader::new(stream);
        let reader: Box<dyn tokio::io::AsyncRead + Send + Unpin> = if payload.is_chunked() {
            Box::new(super::chunked::reader(base))
        } else {
            Box::new(base)
        };
        let mut reader =
            super::chunked::LimitedReader::new(reader, service.config.max_object_bytes as u64);

        if let Some(upload_id) = param(&query, "uploadId") {
            let part_number = param(&query, "partNumber")
                .ok_or_else(|| BlobError::BadRequest("part upload has no partNumber".into()))?
                .parse::<u32>()
                .map_err(|_| BlobError::BadRequest("partNumber is not a number".into()))?;
            let etag = service
                .store
                .upload_part_stream(
                    &bucket,
                    &object_key,
                    &upload_id,
                    part_number,
                    &mut reader,
                    declared_length,
                    put_options(&headers, &payload)?,
                )
                .await?;
            return Ok::<_, BlobError>(
                (StatusCode::OK, [(axum::http::header::ETAG, etag)]).into_response(),
            );
        }

        let meta = service
            .store
            .put_stream(
                &bucket,
                &object_key,
                &mut reader,
                declared_length,
                put_options(&headers, &payload)?,
            )
            .await?;
        Ok::<_, BlobError>(
            (
                StatusCode::OK,
                [
                    (axum::http::header::ETAG, meta.etag()),
                    (
                        axum::http::HeaderName::from_static("x-amz-checksum-sha256"),
                        meta.sha256.clone(),
                    ),
                ],
            )
                .into_response(),
        )
    }
    .await;
    unwrap(result, resource)
}

/// `POST /{bucket}/{key}` — begin or complete a multipart upload.
pub async fn post_object(
    State(service): State<Arc<BlobService>>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    Path((bucket, key)): Path<(String, String)>,
    body: Body,
) -> Response {
    let resource = format!("/{bucket}/{key}");
    let result = async {
        let payload = service.begin(&method, &uri, &headers).await?;
        const COMPLETION_BODY_LIMIT: usize = 2 * 1024 * 1024;
        let body = axum::body::to_bytes(body, COMPLETION_BODY_LIMIT)
            .await
            .map_err(|err| BlobError::BadRequest(format!("multipart request body: {err}")))?;
        let body = service.decode_body(&payload, body)?;
        let object_key = ObjectKey::parse(&key)?;
        let query = auth::decode_query(uri.query().unwrap_or(""));

        if query.iter().any(|(name, _)| name == "uploads") {
            let upload_id = service
                .store
                .create_multipart(
                    &bucket,
                    &object_key,
                    put_options(&headers, &auth::PayloadDescriptor::Unsigned)?,
                )
                .await?;
            return Ok::<_, BlobError>(xml_response(
                StatusCode::OK,
                xml::initiate_multipart(&bucket, &key, &upload_id),
            ));
        }

        let upload_id = param(&query, "uploadId").ok_or_else(|| {
            BlobError::BadRequest("post to an object requires ?uploads or ?uploadId=".into())
        })?;
        let text = String::from_utf8(body)
            .map_err(|_| BlobError::BadRequest("completion body is not utf-8".into()))?;
        let parts = xml::parse_completed_parts(&text)?;
        let meta = service
            .store
            .complete_multipart(&bucket, &object_key, &upload_id, &parts)
            .await?;
        Ok::<_, BlobError>(xml_response(
            StatusCode::OK,
            xml::complete_multipart(&resource, &bucket, &key, &meta.etag()),
        ))
    }
    .await;
    unwrap(result, resource.clone())
}

/// `DELETE /{bucket}/{key}` — remove an object, or abort a multipart upload.
pub async fn delete_object(
    State(service): State<Arc<BlobService>>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    Path((bucket, key)): Path<(String, String)>,
) -> Response {
    let resource = format!("/{bucket}/{key}");
    let result = async {
        service.begin(&method, &uri, &headers).await?;
        let object_key = ObjectKey::parse(&key)?;
        let query = auth::decode_query(uri.query().unwrap_or(""));
        if let Some(upload_id) = param(&query, "uploadId") {
            service
                .store
                .abort_multipart(&bucket, &object_key, &upload_id)
                .await?;
        } else {
            service.store.delete(&bucket, &object_key).await?;
        }
        Ok::<_, BlobError>(StatusCode::NO_CONTENT.into_response())
    }
    .await;
    unwrap(result, resource)
}

/// read the write options a request carries.
fn put_options(
    headers: &HeaderMap,
    payload: &auth::PayloadDescriptor,
) -> Result<PutOptions, BlobError> {
    let mut options = PutOptions {
        content_type: auth::header(headers, "content-type"),
        if_none_match: auth::header(headers, "if-none-match")
            .is_some_and(|value| value.trim() == "*"),
        ..PutOptions::default()
    };
    if let Some(checksum) = auth::header(headers, "x-amz-checksum-sha256") {
        // accepted as hex (what runinator's own callers send) or base64 (what an aws sdk sends).
        options.expected_sha256 =
            Some(sha256_from_checksum_header(&checksum).ok_or_else(|| {
                BlobError::BadRequest(format!(
                    "x-amz-checksum-sha256 '{checksum}' is neither a hex nor a base64 sha-256"
                ))
            })?);
    }
    if let Some(signed) = payload.signed_sha256() {
        if let Some(checksum) = &options.expected_sha256 {
            if !checksum.eq_ignore_ascii_case(signed) {
                return Err(BlobError::DigestMismatch {
                    expected: checksum.clone(),
                    actual: signed.to_ascii_lowercase(),
                });
            }
        }
        options.expected_sha256 = Some(signed.to_ascii_lowercase());
    }
    for (name, value) in headers {
        let name = name.as_str();
        let Some(key) = name.strip_prefix("x-amz-meta-") else {
            continue;
        };

        if let Ok(value) = value.to_str() {
            options.metadata.insert(key.to_string(), value.to_string());
        }
    }
    Ok(options)
}

fn decoded_content_length(headers: &HeaderMap, chunked: bool) -> Result<Option<u64>, BlobError> {
    let name = if chunked {
        "x-amz-decoded-content-length"
    } else {
        "content-length"
    };
    auth::header(headers, name)
        .map(|value| {
            value
                .parse::<u64>()
                .map_err(|_| BlobError::BadRequest(format!("{name} is not a byte count")))
        })
        .transpose()
}

fn param(query: &[(String, String)], name: &str) -> Option<String> {
    query
        .iter()
        .find(|(candidate, _)| candidate == name)
        .map(|(_, value)| value.clone())
}

fn unwrap(result: Result<Response, BlobError>, resource: String) -> Response {
    match result {
        Ok(response) => response,
        Err(error) => BlobRejection::new(error, resource).into_response(),
    }
}
