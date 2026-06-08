//! runinator-lsp: an editor-agnostic language server for the wdl workflow language. it reuses the
//! pure analyzer/completer/formatter in `runinator-wdl` and the api client in `runinator-api`, so
//! any lsp-capable editor gets diagnostics, completion, hover, formatting, and apply-on-save.

mod apply;
mod completion;
mod config;
mod diagnostics;
mod document;
mod errors;
mod hover;
mod metadata;
mod position;
mod server;

use std::sync::Arc;

use tower_lsp::{LspService, Server};

use crate::metadata::MetadataCache;
use crate::server::Backend;

#[tokio::main]
async fn main() {
    // metadata completion targets the process-level base url; apply-on-save targets the per-editor
    // configured service url instead.
    let base_url = std::env::var("RUNINATOR_API_BASE_URL")
        .unwrap_or_else(|_| "http://127.0.0.1:8080/".to_string());

    let metadata = match MetadataCache::new(base_url) {
        Ok(cache) => Arc::new(cache),
        Err(err) => {
            eprintln!("runinator-lsp: failed to build api client: {err}");
            std::process::exit(1);
        }
    };

    let (service, socket) = LspService::new(|client| Backend::new(client, metadata.clone()));
    Server::new(tokio::io::stdin(), tokio::io::stdout(), socket)
        .serve(service)
        .await;
}
