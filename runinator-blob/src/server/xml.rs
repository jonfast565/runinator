//! the handful of s3 xml documents this service speaks.
//!
//! hand-rolled rather than pulled from an xml crate: the surface is six fixed documents with no
//! namespaces, no attributes, and no mixed content, and a parser dependency would be a larger
//! attack surface than the twenty lines of scanning below. the one thing that genuinely matters is
//! escaping, since object keys are attacker-influenced and land inside element text.

use chrono::{DateTime, SecondsFormat, Utc};

use runinator_blob_core::listing::{BucketSummary, ListResponse};
use runinator_blob_core::multipart::CompletedPart;
use runinator_blob_core::BlobError;

const DECLARATION: &str = r#"<?xml version="1.0" encoding="UTF-8"?>"#;
const NAMESPACE: &str = "http://s3.amazonaws.com/doc/2006-03-01/";

/// escape text destined for element content.
pub fn escape(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for character in value.chars() {
        match character {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            other => out.push(other),
        }
    }
    out
}

fn timestamp(value: DateTime<Utc>) -> String {
    value.to_rfc3339_opts(SecondsFormat::Millis, true)
}

/// an s3 `<Error>` document.
pub fn error(code: &str, message: &str, resource: &str, request_id: &str) -> String {
    format!(
        "{DECLARATION}<Error><Code>{}</Code><Message>{}</Message><Resource>{}</Resource><RequestId>{}</RequestId></Error>",
        escape(code),
        escape(message),
        escape(resource),
        escape(request_id)
    )
}

/// a `ListObjectsV2` response.
pub fn list_objects_v2(
    bucket: &str,
    prefix: Option<&str>,
    delimiter: Option<&str>,
    max_keys: usize,
    response: &ListResponse,
) -> String {
    let mut out = format!(
        "{DECLARATION}<ListBucketResult xmlns=\"{NAMESPACE}\"><Name>{}</Name><Prefix>{}</Prefix>",
        escape(bucket),
        escape(prefix.unwrap_or(""))
    );
    if let Some(delimiter) = delimiter {
        out.push_str(&format!("<Delimiter>{}</Delimiter>", escape(delimiter)));
    }
    out.push_str(&format!(
        "<MaxKeys>{max_keys}</MaxKeys><KeyCount>{}</KeyCount><IsTruncated>{}</IsTruncated>",
        response.objects.len() + response.common_prefixes.len(),
        response.is_truncated
    ));
    if let Some(token) = &response.next_continuation_token {
        out.push_str(&format!(
            "<NextContinuationToken>{}</NextContinuationToken>",
            escape(token)
        ));
    }
    for object in &response.objects {
        out.push_str(&format!(
            "<Contents><Key>{}</Key><LastModified>{}</LastModified><ETag>&quot;{}&quot;</ETag><Size>{}</Size><StorageClass>STANDARD</StorageClass></Contents>",
            escape(&object.key),
            timestamp(object.last_modified),
            escape(&object.sha256),
            object.size
        ));
    }
    for prefix in &response.common_prefixes {
        out.push_str(&format!(
            "<CommonPrefixes><Prefix>{}</Prefix></CommonPrefixes>",
            escape(prefix)
        ));
    }
    out.push_str("</ListBucketResult>");
    out
}

/// a `ListBuckets` response.
pub fn list_buckets(buckets: &[BucketSummary]) -> String {
    let mut out = format!(
        "{DECLARATION}<ListAllMyBucketsResult xmlns=\"{NAMESPACE}\"><Owner><ID>runinator</ID><DisplayName>runinator</DisplayName></Owner><Buckets>"
    );
    for bucket in buckets {
        out.push_str(&format!(
            "<Bucket><Name>{}</Name><CreationDate>{}</CreationDate></Bucket>",
            escape(&bucket.name),
            timestamp(bucket.created_at)
        ));
    }
    out.push_str("</Buckets></ListAllMyBucketsResult>");
    out
}

/// a `CreateMultipartUpload` response.
pub fn initiate_multipart(bucket: &str, key: &str, upload_id: &str) -> String {
    format!(
        "{DECLARATION}<InitiateMultipartUploadResult xmlns=\"{NAMESPACE}\"><Bucket>{}</Bucket><Key>{}</Key><UploadId>{}</UploadId></InitiateMultipartUploadResult>",
        escape(bucket),
        escape(key),
        escape(upload_id)
    )
}

/// a `CompleteMultipartUpload` response.
pub fn complete_multipart(location: &str, bucket: &str, key: &str, etag: &str) -> String {
    format!(
        "{DECLARATION}<CompleteMultipartUploadResult xmlns=\"{NAMESPACE}\"><Location>{}</Location><Bucket>{}</Bucket><Key>{}</Key><ETag>&quot;{}&quot;</ETag></CompleteMultipartUploadResult>",
        escape(location),
        escape(bucket),
        escape(key),
        escape(etag.trim_matches('"'))
    )
}

/// parse the `<Part>` list out of a `CompleteMultipartUpload` request body.
pub fn parse_completed_parts(body: &str) -> Result<Vec<CompletedPart>, BlobError> {
    let mut parts = Vec::new();
    for chunk in body.split("<Part>").skip(1) {
        let part = chunk.split("</Part>").next().unwrap_or_default();
        let number = element(part, "PartNumber").ok_or_else(|| {
            BlobError::BadRequest("completion part is missing <PartNumber>".into())
        })?;
        let etag = element(part, "ETag")
            .ok_or_else(|| BlobError::BadRequest("completion part is missing <ETag>".into()))?;
        parts.push(CompletedPart {
            part_number: number.trim().parse().map_err(|_| {
                BlobError::BadRequest(format!("part number '{number}' is not a number"))
            })?,
            // an etag arrives quoted, and the quotes may themselves be xml-escaped.
            etag: format!("\"{}\"", unescape(&etag).trim_matches('"')),
        });
    }
    if parts.is_empty() {
        return Err(BlobError::BadRequest(
            "completion request lists no parts".into(),
        ));
    }
    Ok(parts)
}

fn element(source: &str, name: &str) -> Option<String> {
    let open = format!("<{name}>");
    let close = format!("</{name}>");
    let start = source.find(&open)? + open.len();
    let end = source[start..].find(&close)? + start;
    Some(source[start..end].to_string())
}

fn unescape(value: &str) -> String {
    value
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&amp;", "&")
}

#[cfg(test)]
#[path = "xml_tests.rs"]
mod tests;
