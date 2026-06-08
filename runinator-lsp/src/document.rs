//! in-memory store of open document text, kept in sync via full-text `textDocument/didChange`.

use std::collections::HashMap;
use std::sync::RwLock;

use tower_lsp::lsp_types::Url;

/// thread-safe map of document uri to its current full text.
#[derive(Default)]
pub struct DocumentStore {
    docs: RwLock<HashMap<Url, String>>,
}

impl DocumentStore {
    pub fn upsert(&self, uri: Url, text: String) {
        if let Ok(mut docs) = self.docs.write() {
            docs.insert(uri, text);
        }
    }

    pub fn remove(&self, uri: &Url) {
        if let Ok(mut docs) = self.docs.write() {
            docs.remove(uri);
        }
    }

    /// clone the current text for `uri`, if open. callers clone out and drop the lock before any
    /// `.await` so the guard is never held across a suspension point.
    pub fn get(&self, uri: &Url) -> Option<String> {
        self.docs.read().ok()?.get(uri).cloned()
    }
}
