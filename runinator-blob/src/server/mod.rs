//! the S3-compatible http service.
//!
//! path-style addressing only (`/{bucket}/{key}`), which is what an endpoint-overridden client uses
//! and what avoids needing wildcard dns. virtual-host addressing (`{bucket}.host/{key}`) is not
//! served: nothing in runinator asks for it, and supporting both would double the routing surface
//! that signature verification depends on.

pub mod auth;
mod buckets;
mod chunked;
mod objects;
mod reply;
pub mod xml;

use std::net::SocketAddr;
use std::sync::Arc;

use axum::extract::DefaultBodyLimit;
use axum::http::{HeaderMap, Method, StatusCode, Uri};
use axum::routing::get;
use axum::Router;
use tokio::net::TcpListener;

use runinator_blob_core::listing::BucketSummary;
use runinator_blob_core::{BlobError, BlobStore, FsBlobStore};

use crate::config::BlobServerConfig;

use auth::PayloadDescriptor;

/// the service's shared state: the backing store plus the credentials it verifies against.
pub struct BlobService {
    pub store: Arc<dyn BlobStore>,
    config: BlobServerConfig,
}

impl BlobService {
    pub fn new(store: Arc<dyn BlobStore>, config: BlobServerConfig) -> Self {
        Self { store, config }
    }

    /// authenticate a request and report how its payload was described.
    ///
    /// every handler calls this first. it is one call rather than a middleware layer because the
    /// signature covers the payload hash, and a middleware would have to buffer every body — turning
    /// a ranged `GET` of a large object into a needless allocation.
    async fn begin(
        &self,
        method: &Method,
        uri: &Uri,
        headers: &HeaderMap,
    ) -> Result<PayloadDescriptor, BlobError> {
        let payload = auth::payload_descriptor(headers)?;
        auth::authenticate(
            method,
            uri,
            headers,
            &payload,
            &self.config.credentials,
            &self.config.region,
        )?;
        Ok(payload)
    }

    /// unwrap a request body: strip `aws-chunked` framing if present, then check it against the
    /// hash the sender signed.
    fn decode_body(
        &self,
        payload: &PayloadDescriptor,
        body: axum::body::Bytes,
    ) -> Result<Vec<u8>, BlobError> {
        if payload.is_chunked() {
            // the framing is not part of what was signed, so the hash check does not apply to it.
            return chunked::decode(&body);
        }
        auth::verify_payload(payload, &body)?;
        Ok(body.to_vec())
    }

    /// the buckets this store holds, for `ListBuckets`.
    async fn buckets(&self) -> Result<Vec<BucketSummary>, BlobError> {
        self.store.list_buckets().await
    }
}

/// the service's routes.
pub fn router(service: Arc<BlobService>) -> Router {
    let max_body = service.config.max_object_bytes;
    Router::new()
        .route("/", get(buckets::list_buckets))
        .route(
            "/{bucket}",
            get(buckets::list_objects)
                .put(buckets::put_bucket)
                .head(buckets::head_bucket)
                .delete(buckets::delete_bucket),
        )
        .route(
            "/{bucket}/{*key}",
            get(objects::get_object)
                .put(objects::put_object)
                .head(objects::head_object)
                .delete(objects::delete_object)
                .post(objects::post_object),
        )
        .fallback(fallback)
        .layer(DefaultBodyLimit::max(max_body))
        .with_state(service)
}

async fn fallback(uri: Uri) -> (StatusCode, String) {
    (
        StatusCode::NOT_FOUND,
        xml::error("NoSuchKey", "no such route", uri.path(), ""),
    )
}

/// run the service until `shutdown` resolves.
pub async fn run_server(
    config: BlobServerConfig,
    shutdown: impl std::future::Future<Output = ()> + Send + 'static,
) -> Result<(), BlobError> {
    let store = FsBlobStore::open(&config.data_dir).await?;
    // the buckets runinator itself relies on exist from the first boot, so no deployment step has to
    // remember to create them.
    for bucket in [
        runinator_blob_core::FUNCTION_ARTIFACT_BUCKET,
        runinator_blob_core::RUN_ARTIFACT_BUCKET,
    ] {
        store.create_bucket(bucket).await?;
    }
    let addr: SocketAddr = config
        .listen_addr
        .parse()
        .map_err(|err| BlobError::BadRequest(format!("invalid listen address: {err}")))?;
    let service = Arc::new(BlobService::new(Arc::new(store), config.clone()));
    let listener = TcpListener::bind(addr)
        .await
        .map_err(|err| BlobError::Io(format!("binding {addr}: {err}")))?;
    tracing::info!(
        addr = %addr,
        data_dir = %config.data_dir,
        anonymous = config.credentials.allows_anonymous(),
        "runinator blob service listening"
    );
    axum::serve(listener, router(service))
        .with_graceful_shutdown(shutdown)
        .await
        .map_err(|err| BlobError::Io(format!("serving: {err}")))
}

#[cfg(test)]
#[path = "mod_tests.rs"]
mod tests;
