//! Turn store results into S3-shaped HTTP responses.

use axum::http::{header, HeaderMap, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};

use runinator_blob_core::{BlobError, ObjectMeta, ResolvedRange};

use super::xml;

/// content type for every xml document this service returns.
pub const XML_CONTENT_TYPE: &str = "application/xml";

/// A failure and its resource, rendered as an S3 `<Error>` document.
pub struct BlobRejection {
    error: BlobError,
    resource: String,
    request_id: String,
}

impl BlobRejection {
    pub fn new(error: BlobError, resource: impl Into<String>) -> Self {
        Self {
            error,
            resource: resource.into(),
            request_id: uuid::Uuid::now_v7().to_string(),
        }
    }
}

impl IntoResponse for BlobRejection {
    fn into_response(self) -> Response {
        let status = StatusCode::from_u16(self.error.http_status())
            .unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
        // the code and the rendered message are what a client branches on, so log the pair rather
        // than only the status; a 403 with no reason is the hardest blob failure to diagnose.
        if status.is_server_error() {
            tracing::error!(code = self.error.s3_code(), request_id = %self.request_id, "{}", self.error);
        } else {
            tracing::debug!(code = self.error.s3_code(), request_id = %self.request_id, "{}", self.error);
        }
        let body = xml::error(
            self.error.s3_code(),
            &self.error.to_string(),
            &self.resource,
            &self.request_id,
        );
        // the code also rides in a header: a `HEAD` response carries no body, so the xml document
        // below is invisible to the one verb where distinguishing "no such bucket" from "no such
        // key" matters most. S3 sends `x-amz-error-code` for the same reason.
        let mut headers = HeaderMap::new();
        insert(
            &mut headers,
            header::CONTENT_TYPE.as_str(),
            XML_CONTENT_TYPE,
        );
        insert(&mut headers, "x-amz-request-id", &self.request_id);
        insert(&mut headers, "x-amz-error-code", self.error.s3_code());
        insert(&mut headers, "x-amz-error-message", &self.error.to_string());
        (status, headers, body).into_response()
    }
}

/// an xml document response.
pub fn xml_response(status: StatusCode, body: String) -> Response {
    (status, [(header::CONTENT_TYPE, XML_CONTENT_TYPE)], body).into_response()
}

/// the headers describing a stored object, shared by `GET` and `HEAD` so the two can never disagree.
pub fn object_headers(meta: &ObjectMeta, range: Option<ResolvedRange>) -> HeaderMap {
    let mut headers = HeaderMap::new();
    let length = range.map(|range| range.length).unwrap_or(meta.size);
    insert(
        &mut headers,
        header::CONTENT_LENGTH.as_str(),
        &length.to_string(),
    );
    insert(
        &mut headers,
        header::CONTENT_TYPE.as_str(),
        &meta.content_type,
    );
    insert(&mut headers, header::ETAG.as_str(), &meta.etag());
    insert(
        &mut headers,
        header::LAST_MODIFIED.as_str(),
        &meta
            .last_modified
            .format("%a, %d %b %Y %H:%M:%S GMT")
            .to_string(),
    );
    insert(&mut headers, header::ACCEPT_RANGES.as_str(), "bytes");
    if let Some(range) = range {
        insert(
            &mut headers,
            header::CONTENT_RANGE.as_str(),
            &range.content_range(),
        );
    } else {
        // only on a whole-object response, and base64-encoded. an aws sdk treats this header as the
        // checksum *of the bytes it just received* and fails the transfer when they disagree — so
        // sending the whole-object digest alongside a 206 breaks every ranged download (which is how
        // the sdk fetches anything large), and sending hex breaks every download outright.
        if let Some(encoded) = runinator_blob_core::sha256_hex_to_base64(&meta.sha256) {
            insert(&mut headers, "x-amz-checksum-sha256", &encoded);
        }
    }
    for (name, value) in &meta.metadata {
        insert(&mut headers, &format!("x-amz-meta-{name}"), value);
    }
    headers
}

/// insert a header, dropping any value that cannot be one. a user-supplied metadata value with a
/// newline in it is the realistic case, and silently omitting it is better than failing the read.
fn insert(headers: &mut HeaderMap, name: &str, value: &str) {
    let (Ok(name), Ok(value)) = (
        name.parse::<axum::http::HeaderName>(),
        HeaderValue::from_str(value),
    ) else {
        return;
    };
    headers.insert(name, value);
}

#[cfg(test)]
#[path = "reply_tests.rs"]
mod tests;
