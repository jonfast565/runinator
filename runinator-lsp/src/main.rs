//! runinator-lsp: an editor-agnostic language server for the rexrap workflow language. it reuses the
//! pure analyzer/completer/formatter in `runinator-rexrap` and the API client in `runinator-api`, so
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
mod service;

use std::sync::Arc;

use tower_lsp::{LspService, Server};

use crate::metadata::MetadataCache;
use crate::server::Backend;
use crate::service::LanguageServerService;

#[tokio::main]
async fn main() {
    LanguageServerService::new().run().await;
}

async fn run_process() {
    // Metadata completion uses the process-level base URL. Apply-on-save uses the editor's
    // configured service URL instead.
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
