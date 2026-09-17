//! An HTTP client for the S3 surface served by this crate.
//!
//! it implements [`BlobStore`], so a caller holding `Arc<dyn BlobStore>` cannot tell whether its
//! objects are on a local disk or behind the blob service. signing goes through the same
//! `runinator-blob-core` canonicalization the server verifies with, which is what makes a signature
//! failure a real bug rather than a disagreement between two hand-written implementations.

use std::collections::BTreeMap;

use async_trait::async_trait;
use chrono::Utc;
use futures_util::TryStreamExt;
use reqwest::{Client, Method, Response, StatusCode, Url};
use sha2::{Digest, Sha256};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncSeekExt, AsyncWriteExt};

use runinator_blob_core::listing::{BucketSummary, ListRequest, ListResponse, ObjectSummary};
use runinator_blob_core::multipart::CompletedPart;
use runinator_blob_core::sigv4::{
    canonical::{encode_path_segments, payload_hash},
    sign_request, BlobCredential, CanonicalRequest,
};
use runinator_blob_core::store::{BlobStore, ObjectReader, Result};
use runinator_blob_core::{
    BlobError, ByteRange, ObjectKey, ObjectMeta, PutOptions, DEFAULT_CONTENT_TYPE,
};

use crate::config::BlobClientConfig;

/// a blob store reached over http.

enum RequestBody {
    Bytes(Vec<u8>),
    Spool(Spool),
}

impl RequestBody {
    fn sha256(&self) -> String {
        match self {
            RequestBody::Bytes(bytes) => payload_hash(bytes),
            RequestBody::Spool(spool) => spool.sha256.clone(),
        }
    }
}

/// the total object size a `Content-Range` reports, when one is present.
fn content_range_total(headers: &reqwest::header::HeaderMap) -> Option<u64> {
    headers
        .get("content-range")?
        .to_str()
        .ok()?
        .rsplit('/')
        .next()?
        .parse()
        .ok()
}

fn parse_listing(body: &str) -> ListResponse {
    let objects = body
        .split("<Contents>")
        .skip(1)
        .filter_map(|chunk| {
            Some(ObjectSummary {
                key: element(chunk, "Key")?,
                size: element(chunk, "Size")?.parse().ok()?,
                sha256: element(chunk, "ETag")
                    .unwrap_or_default()
                    .replace("&quot;", "")
                    .trim_matches('"')
                    .to_string(),
                last_modified: element(chunk, "LastModified")
                    .and_then(|raw| chrono::DateTime::parse_from_rfc3339(&raw).ok())
                    .map(|parsed| parsed.with_timezone(&Utc))
                    .unwrap_or_else(Utc::now),
            })
        })
        .collect();
    let common_prefixes = body
        .split("<CommonPrefixes>")
        .skip(1)
        .filter_map(|chunk| element(chunk, "Prefix"))
        .collect();
    ListResponse {
        objects,
        common_prefixes,
        is_truncated: element(body, "IsTruncated").as_deref() == Some("true"),
        next_continuation_token: element(body, "NextContinuationToken"),
    }
}

/// read one element's text out of an xml fragment.
fn element(source: &str, name: &str) -> Option<String> {
    let open = format!("<{name}>");
    let close = format!("</{name}>");
    let start = source.find(&open)? + open.len();
    let end = source[start..].find(&close)? + start;
    Some(
        source[start..end]
            .replace("&quot;", "\"")
            .replace("&lt;", "<")
            .replace("&gt;", ">")
            .replace("&amp;", "&"),
    )
}

mod s3_blob_client;
pub use s3_blob_client::S3BlobClient;

mod spool;
use spool::Spool;
