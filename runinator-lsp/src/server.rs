//! the `tower-lsp` backend: wires document sync, diagnostics, completion, hover, formatting, and
//! apply-on-save onto the reusable wdl/api building blocks.

use std::sync::{Arc, RwLock};
use std::time::Duration;

use tower_lsp::jsonrpc::Result;
use tower_lsp::lsp_types::*;
use tower_lsp::{Client, LanguageServer};

use crate::config::Config;
use crate::document::DocumentStore;
use crate::metadata::MetadataCache;
use crate::position::whole_document_range;
use crate::{apply, completion, diagnostics, hover};

const METADATA_REFRESH_SECS: u64 = 45;

pub struct Backend {
    client: Client,
    documents: DocumentStore,
    metadata: Arc<MetadataCache>,
    config: RwLock<Config>,
}

impl Backend {
    pub fn new(client: Client, metadata: Arc<MetadataCache>) -> Self {
        Self {
            client,
            documents: DocumentStore::default(),
            metadata,
            config: RwLock::new(Config::default()),
        }
    }

    fn set_config(&self, value: Option<&serde_json::Value>) {
        if let Ok(mut slot) = self.config.write() {
            *slot = Config::from_value(value);
        }
    }

    fn config(&self) -> Config {
        self.config
            .read()
            .map(|config| config.clone())
            .unwrap_or_default()
    }

    // recompute and publish diagnostics for an open document. non-workflow files (.wdlp/.wdls) get
    // their diagnostics cleared instead of analyzed with the workflow grammar.
    async fn publish(&self, uri: Url, check_lowering: bool) {
        let Some(text) = self.documents.get(&uri) else {
            return;
        };
        if !is_workflow_uri(&uri) {
            self.client.publish_diagnostics(uri, Vec::new(), None).await;
            return;
        }
        let diagnostics = diagnostics::compute(&text, check_lowering);
        self.client
            .publish_diagnostics(uri, diagnostics, None)
            .await;
    }
}

#[tower_lsp::async_trait]
impl LanguageServer for Backend {
    async fn initialize(&self, params: InitializeParams) -> Result<InitializeResult> {
        self.set_config(params.initialization_options.as_ref());
        Ok(InitializeResult {
            server_info: Some(ServerInfo {
                name: "runinator-lsp".to_string(),
                version: Some(env!("CARGO_PKG_VERSION").to_string()),
            }),
            capabilities: ServerCapabilities {
                text_document_sync: Some(TextDocumentSyncCapability::Kind(
                    TextDocumentSyncKind::FULL,
                )),
                completion_provider: Some(CompletionOptions {
                    trigger_characters: Some(vec![
                        ".".to_string(),
                        "(".to_string(),
                        ":".to_string(),
                    ]),
                    ..Default::default()
                }),
                hover_provider: Some(HoverProviderCapability::Simple(true)),
                document_formatting_provider: Some(OneOf::Left(true)),
                ..Default::default()
            },
        })
    }

    async fn initialized(&self, _: InitializedParams) {
        // refresh provider/setting metadata on a timer; failures keep the prior snapshot.
        let metadata = self.metadata.clone();
        tokio::spawn(async move {
            let mut ticker = tokio::time::interval(Duration::from_secs(METADATA_REFRESH_SECS));
            loop {
                ticker.tick().await;
                metadata.refresh().await;
            }
        });
        self.client
            .log_message(MessageType::INFO, "runinator-lsp ready")
            .await;
    }

    async fn shutdown(&self) -> Result<()> {
        Ok(())
    }

    async fn did_change_configuration(&self, params: DidChangeConfigurationParams) {
        self.set_config(Some(&params.settings));
    }

    async fn did_open(&self, params: DidOpenTextDocumentParams) {
        let uri = params.text_document.uri;
        self.documents
            .upsert(uri.clone(), params.text_document.text);
        self.publish(uri, false).await;
    }

    async fn did_change(&self, params: DidChangeTextDocumentParams) {
        let uri = params.text_document.uri;
        // full sync: the last change carries the entire document text.
        if let Some(change) = params.content_changes.into_iter().next_back() {
            self.documents.upsert(uri.clone(), change.text);
        }
        self.publish(uri, false).await;
    }

    async fn did_save(&self, params: DidSaveTextDocumentParams) {
        let uri = params.text_document.uri;
        self.publish(uri.clone(), true).await;

        let config = self.config();
        if !config.auto_apply {
            return;
        }
        let Some(service_url) = config.service_url else {
            return;
        };
        let Ok(path) = uri.to_file_path() else {
            return;
        };
        if !runinator_pack::source::is_pack_source(&path) {
            return;
        }

        match apply::apply(&service_url, &path).await {
            Ok(message) => self.client.show_message(MessageType::INFO, message).await,
            Err(err) => {
                self.client
                    .show_message(MessageType::ERROR, format!("runinator apply failed: {err}"))
                    .await
            }
        }
    }

    async fn did_close(&self, params: DidCloseTextDocumentParams) {
        let uri = params.text_document.uri;
        self.documents.remove(&uri);
        self.client.publish_diagnostics(uri, Vec::new(), None).await;
    }

    async fn completion(&self, params: CompletionParams) -> Result<Option<CompletionResponse>> {
        let uri = params.text_document_position.text_document.uri;
        if !is_workflow_uri(&uri) {
            return Ok(None);
        }
        let Some(text) = self.documents.get(&uri) else {
            return Ok(None);
        };
        let metadata = self.metadata.snapshot();
        Ok(Some(completion::complete(
            &text,
            params.text_document_position.position,
            &metadata,
        )))
    }

    async fn hover(&self, params: HoverParams) -> Result<Option<Hover>> {
        let uri = params.text_document_position_params.text_document.uri;
        if !is_workflow_uri(&uri) {
            return Ok(None);
        }
        let Some(text) = self.documents.get(&uri) else {
            return Ok(None);
        };
        Ok(hover::hover(
            &text,
            params.text_document_position_params.position,
        ))
    }

    async fn formatting(&self, params: DocumentFormattingParams) -> Result<Option<Vec<TextEdit>>> {
        let uri = params.text_document.uri;
        if !is_workflow_uri(&uri) {
            return Ok(None);
        }
        let Some(text) = self.documents.get(&uri) else {
            return Ok(None);
        };
        match runinator_wdl::format_str(&text) {
            Ok(formatted) if formatted != text => Ok(Some(vec![TextEdit {
                range: whole_document_range(&text),
                new_text: formatted,
            }])),
            // already formatted: no edits. unparseable: leave the buffer untouched.
            Ok(_) => Ok(Some(Vec::new())),
            Err(_) => Ok(None),
        }
    }
}

// true when the uri names a `.wdl` workflow source (or a non-file uri we optimistically treat as
// one); `.wdlp`/`.wdls` are not analyzed with the workflow grammar.
fn is_workflow_uri(uri: &Url) -> bool {
    match uri.to_file_path() {
        Ok(path) => path
            .extension()
            .and_then(|ext| ext.to_str())
            .map(|ext| ext.eq_ignore_ascii_case("wdl"))
            .unwrap_or(false),
        Err(_) => true,
    }
}
