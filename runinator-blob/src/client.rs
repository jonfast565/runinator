//! an http client that speaks the same s3 surface the service serves.
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
pub struct S3BlobClient {
    http: Client,
    endpoint: Url,
    region: String,
    credential: Option<BlobCredential>,
}

impl S3BlobClient {
    pub fn new(config: BlobClientConfig) -> Result<Self> {
        let endpoint = Url::parse(&config.endpoint)
            .map_err(|err| BlobError::BadRequest(format!("invalid blob endpoint: {err}")))?;
        let http = Client::builder()
            .build()
            .map_err(|err| BlobError::Transport(format!("building blob http client: {err}")))?;
        Ok(Self {
            http,
            endpoint,
            region: config.region,
            credential: config.credential,
        })
    }

    /// the endpoint this client was built against.
    pub fn endpoint(&self) -> &Url {
        &self.endpoint
    }

    /// build, sign, and send one request.
    ///
    /// the path is encoded once and then signed verbatim, matching how s3 canonicalizes a request
    /// (`use_double_uri_encode = false`); re-deriving the canonical path from the decoded key would
    /// disagree with the server for any key containing `!`, `*`, `'`, `(`, or `)`.
    async fn send(
        &self,
        method: Method,
        path: &str,
        query: Vec<(String, String)>,
        headers: Vec<(String, String)>,
        body: Option<Vec<u8>>,
    ) -> Result<Response> {
        let encoded_path = encode_path_segments(path);
        let mut url = self
            .endpoint
            .join(&encoded_path)
            .map_err(|err| BlobError::BadRequest(format!("building blob url: {err}")))?;
        if !query.is_empty() {
            let mut pairs = url.query_pairs_mut();
            for (name, value) in &query {
                pairs.append_pair(name, value);
            }
        }

        let host = url
            .host_str()
            .map(|host| match url.port() {
                Some(port) => format!("{host}:{port}"),
                None => host.to_string(),
            })
            .unwrap_or_default();
        let hash = body
            .as_ref()
            .map(|bytes| payload_hash(bytes))
            .unwrap_or_else(|| payload_hash(&[]));

        let mut signed_headers = vec![
            ("host".to_string(), host),
            ("x-amz-content-sha256".to_string(), hash.clone()),
        ];
        signed_headers.extend(
            headers
                .iter()
                .map(|(name, value)| (name.to_ascii_lowercase(), value.clone())),
        );

        let mut request = self.http.request(method.clone(), url.clone());
        for (name, value) in &headers {
            request = request.header(name, value);
        }
        request = request.header("x-amz-content-sha256", &hash);

        if let Some(credential) = &self.credential {
            let now = Utc::now();
            let amz_date = now
                .format(runinator_blob_core::sigv4::AMZ_DATE_FORMAT)
                .to_string();
            signed_headers.push(("x-amz-date".to_string(), amz_date.clone()));
            let canonical = CanonicalRequest {
                method: method.as_str(),
                path: url.path(),
                query: query.clone(),
                headers: signed_headers.clone(),
                payload_hash: &hash,
            };
            let signature = sign_request(&canonical, credential, &self.region, now);
            request = request.header("x-amz-date", amz_date).header(
                "authorization",
                signature.authorization_header(&credential.access_key_id),
            );
        }

        if let Some(body) = body {
            request = request.body(body);
        }
        let response = request
            .send()
            .await
            .map_err(|err| BlobError::Transport(format!("{method} {url}: {err}")))?;
        Ok(response)
    }

    /// turn a non-success response into the domain error it represents.
    async fn check(response: Response, context: &str) -> Result<Response> {
        if response.status().is_success() {
            return Ok(response);
        }
        let status = response.status().as_u16();
        // prefer the headers: a `HEAD` reply has no body to parse, and they carry the same code.
        let header_text = |name: &str| {
            response
                .headers()
                .get(name)
                .and_then(|value| value.to_str().ok())
                .map(str::to_string)
        };
        let header_code = header_text("x-amz-error-code");
        let header_message = header_text("x-amz-error-message");
        let body = response.text().await.unwrap_or_default();
        let code = header_code
            .or_else(|| element(&body, "Code"))
            .unwrap_or_default();
        let message = header_message
            .or_else(|| element(&body, "Message"))
            .unwrap_or_else(|| body.clone());
        Err(BlobError::from_s3_code(
            &code,
            status,
            format!("{context}: {message}"),
        ))
    }

    /// read an object descriptor out of response headers.
    fn meta_from_headers(key: &ObjectKey, response: &Response) -> ObjectMeta {
        let headers = response.headers();
        let text = |name: &str| {
            headers
                .get(name)
                .and_then(|value| value.to_str().ok())
                .map(str::to_string)
        };
        // the checksum header is base64 on the wire; the etag fallback is already hex.
        let sha256 = text("x-amz-checksum-sha256")
            .and_then(|value| runinator_blob_core::sha256_from_checksum_header(&value))
            .or_else(|| text("etag").map(|etag| etag.trim_matches('"').to_string()))
            .unwrap_or_default();
        let mut metadata = BTreeMap::new();
        for (name, value) in headers {
            if let Some(suffix) = name.as_str().strip_prefix("x-amz-meta-") {
                if let Ok(value) = value.to_str() {
                    metadata.insert(suffix.to_string(), value.to_string());
                }
            }
        }
        ObjectMeta {
            key: key.as_str().to_string(),
            // a ranged read reports the slice length in `content-length`, so the object's true size
            // comes from `content-range` when one is present.
            size: content_range_total(headers)
                .or_else(|| text("content-length").and_then(|value| value.parse().ok()))
                .unwrap_or_default(),
            sha256,
            content_type: text("content-type").unwrap_or_else(|| DEFAULT_CONTENT_TYPE.to_string()),
            last_modified: text("last-modified")
                .and_then(|value| {
                    chrono::DateTime::parse_from_rfc2822(&value)
                        .ok()
                        .map(|parsed| parsed.with_timezone(&Utc))
                })
                .unwrap_or_else(Utc::now),
            metadata,
        }
    }
}

#[async_trait]
impl BlobStore for S3BlobClient {
    fn backend(&self) -> &'static str {
        "s3"
    }

    async fn create_bucket(&self, bucket: &str) -> Result<()> {
        let response = self
            .send(
                Method::PUT,
                &format!("/{bucket}"),
                vec![],
                vec![],
                Some(Vec::new()),
            )
            .await?;
        Self::check(response, "create bucket").await.map(|_| ())
    }

    async fn delete_bucket(&self, bucket: &str) -> Result<()> {
        let response = self
            .send(Method::DELETE, &format!("/{bucket}"), vec![], vec![], None)
            .await?;
        Self::check(response, "delete bucket").await.map(|_| ())
    }

    async fn bucket_exists(&self, bucket: &str) -> Result<bool> {
        let response = self
            .send(Method::HEAD, &format!("/{bucket}"), vec![], vec![], None)
            .await?;
        if response.status() == StatusCode::NOT_FOUND {
            return Ok(false);
        }
        Self::check(response, "head bucket").await.map(|_| true)
    }

    async fn list_buckets(&self) -> Result<Vec<BucketSummary>> {
        let response = self.send(Method::GET, "/", vec![], vec![], None).await?;
        let body = Self::check(response, "list buckets")
            .await?
            .text()
            .await
            .map_err(|err| BlobError::Transport(format!("reading bucket list: {err}")))?;
        Ok(body
            .split("<Bucket>")
            .skip(1)
            .filter_map(|chunk| {
                let name = element(chunk, "Name")?;
                Some(BucketSummary {
                    created_at: element(chunk, "CreationDate")
                        .and_then(|raw| chrono::DateTime::parse_from_rfc3339(&raw).ok())
                        .map(|parsed| parsed.with_timezone(&Utc))
                        .unwrap_or_else(Utc::now),
                    name,
                })
            })
            .collect())
    }

    async fn put(
        &self,
        bucket: &str,
        key: &ObjectKey,
        body: Vec<u8>,
        options: PutOptions,
    ) -> Result<ObjectMeta> {
        let mut headers = vec![(
            "content-type".to_string(),
            options
                .content_type
                .clone()
                .unwrap_or_else(|| DEFAULT_CONTENT_TYPE.to_string()),
        )];
        if options.if_none_match {
            headers.push(("if-none-match".to_string(), "*".to_string()));
        }
        if let Some(expected) = &options.expected_sha256 {
            let encoded = runinator_blob_core::sha256_hex_to_base64(expected)
                .unwrap_or_else(|| expected.clone());
            headers.push(("x-amz-checksum-sha256".to_string(), encoded));
        }
        for (name, value) in &options.metadata {
            headers.push((format!("x-amz-meta-{name}"), value.clone()));
        }
        let size = body.len() as u64;
        let sha256 = runinator_blob_core::sha256_hex(&body);
        let response = self
            .send(
                Method::PUT,
                &format!("/{bucket}/{key}"),
                vec![],
                headers,
                Some(body),
            )
            .await?;
        Self::check(response, "put object").await?;
        Ok(ObjectMeta {
            key: key.as_str().to_string(),
            size,
            sha256,
            content_type: options
                .content_type
                .unwrap_or_else(|| DEFAULT_CONTENT_TYPE.to_string()),
            last_modified: Utc::now(),
            metadata: options.metadata,
        })
    }

    async fn head(&self, bucket: &str, key: &ObjectKey) -> Result<ObjectMeta> {
        let response = self
            .send(
                Method::HEAD,
                &format!("/{bucket}/{key}"),
                vec![],
                vec![],
                None,
            )
            .await?;
        let response = Self::check(response, &format!("head {bucket}/{key}")).await?;
        Ok(Self::meta_from_headers(key, &response))
    }

    async fn open(
        &self,
        bucket: &str,
        key: &ObjectKey,
        range: Option<ByteRange>,
    ) -> Result<ObjectReader> {
        let headers = range
            .map(|range| vec![("range".to_string(), range.to_string())])
            .unwrap_or_default();
        let response = self
            .send(
                Method::GET,
                &format!("/{bucket}/{key}"),
                vec![],
                headers,
                None,
            )
            .await?;
        let response = Self::check(response, &format!("get {bucket}/{key}")).await?;
        let meta = Self::meta_from_headers(key, &response);
        let resolved = range.map(|range| range.resolve(meta.size)).transpose()?;
        let stream = response.bytes_stream();
        let reader = tokio_util::io::StreamReader::new(stream.map_err(std::io::Error::other));
        Ok(ObjectReader {
            meta,
            range: resolved,
            body: Box::new(reader),
        })
    }

    async fn delete(&self, bucket: &str, key: &ObjectKey) -> Result<()> {
        let response = self
            .send(
                Method::DELETE,
                &format!("/{bucket}/{key}"),
                vec![],
                vec![],
                None,
            )
            .await?;
        Self::check(response, "delete object").await.map(|_| ())
    }

    async fn list(&self, bucket: &str, request: &ListRequest) -> Result<ListResponse> {
        let mut query = vec![("list-type".to_string(), "2".to_string())];
        if let Some(prefix) = &request.prefix {
            query.push(("prefix".to_string(), prefix.clone()));
        }
        if let Some(delimiter) = &request.delimiter {
            query.push(("delimiter".to_string(), delimiter.clone()));
        }
        if let Some(token) = &request.continuation_token {
            query.push(("continuation-token".to_string(), token.clone()));
        }
        if let Some(max_keys) = request.max_keys {
            query.push(("max-keys".to_string(), max_keys.to_string()));
        }
        let response = self
            .send(Method::GET, &format!("/{bucket}"), query, vec![], None)
            .await?;
        let body = Self::check(response, "list objects")
            .await?
            .text()
            .await
            .map_err(|err| BlobError::Transport(format!("reading listing: {err}")))?;
        Ok(parse_listing(&body))
    }

    async fn create_multipart(
        &self,
        bucket: &str,
        key: &ObjectKey,
        options: PutOptions,
    ) -> Result<String> {
        let headers = options
            .content_type
            .map(|value| vec![("content-type".to_string(), value)])
            .unwrap_or_default();
        let response = self
            .send(
                Method::POST,
                &format!("/{bucket}/{key}"),
                vec![("uploads".to_string(), String::new())],
                headers,
                Some(Vec::new()),
            )
            .await?;
        let body = Self::check(response, "create multipart")
            .await?
            .text()
            .await
            .map_err(|err| BlobError::Transport(format!("reading upload id: {err}")))?;
        element(&body, "UploadId")
            .ok_or_else(|| BlobError::Transport("create multipart returned no UploadId".into()))
    }

    async fn upload_part(
        &self,
        bucket: &str,
        key: &ObjectKey,
        upload_id: &str,
        part_number: u32,
        body: Vec<u8>,
    ) -> Result<String> {
        let response = self
            .send(
                Method::PUT,
                &format!("/{bucket}/{key}"),
                vec![
                    ("partNumber".to_string(), part_number.to_string()),
                    ("uploadId".to_string(), upload_id.to_string()),
                ],
                vec![],
                Some(body),
            )
            .await?;
        let response = Self::check(response, "upload part").await?;
        Ok(response
            .headers()
            .get("etag")
            .and_then(|value| value.to_str().ok())
            .unwrap_or_default()
            .to_string())
    }

    async fn complete_multipart(
        &self,
        bucket: &str,
        key: &ObjectKey,
        upload_id: &str,
        parts: &[CompletedPart],
    ) -> Result<ObjectMeta> {
        let mut body = String::from("<CompleteMultipartUpload>");
        for part in parts {
            body.push_str(&format!(
                "<Part><PartNumber>{}</PartNumber><ETag>{}</ETag></Part>",
                part.part_number,
                part.etag.replace('"', "&quot;")
            ));
        }
        body.push_str("</CompleteMultipartUpload>");
        let response = self
            .send(
                Method::POST,
                &format!("/{bucket}/{key}"),
                vec![("uploadId".to_string(), upload_id.to_string())],
                vec![("content-type".to_string(), "application/xml".to_string())],
                Some(body.into_bytes()),
            )
            .await?;
        Self::check(response, "complete multipart").await?;
        self.head(bucket, key).await
    }

    async fn abort_multipart(&self, bucket: &str, key: &ObjectKey, upload_id: &str) -> Result<()> {
        let response = self
            .send(
                Method::DELETE,
                &format!("/{bucket}/{key}"),
                vec![("uploadId".to_string(), upload_id.to_string())],
                vec![],
                None,
            )
            .await?;
        Self::check(response, "abort multipart").await.map(|_| ())
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
