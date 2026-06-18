//! turn `complete_source` results into lsp completion items. provider/action/setting metadata is
//! supplied from the cache; with an empty cache the wdl completer still returns language items.

use runinator_wdl::{WdlCompletionItem, WdlCompletionRequest, complete_source};
use tower_lsp::lsp_types::{
    CompletionItem, CompletionItemKind, CompletionList, CompletionResponse, CompletionTextEdit,
    Documentation, InsertTextFormat, Position, Range, TextEdit,
};

use crate::metadata::MetadataSnapshot;
use crate::position::{bytes_to_range, position_to_byte};

pub fn complete(text: &str, position: Position, metadata: &MetadataSnapshot) -> CompletionResponse {
    let request = WdlCompletionRequest {
        source: text.to_string(),
        cursor_byte: position_to_byte(text, position),
        providers: metadata.providers.clone(),
        settings: metadata.settings.clone(),
    };
    let response = complete_source(request);
    let range = bytes_to_range(text, response.replace_start_byte, response.replace_end_byte);
    let items = response
        .items
        .iter()
        .map(|item| to_item(range, item))
        .collect();
    CompletionResponse::List(CompletionList {
        is_incomplete: false,
        items,
    })
}

fn to_item(range: Range, item: &WdlCompletionItem) -> CompletionItem {
    CompletionItem {
        label: item.label.clone(),
        kind: Some(map_kind(&item.kind)),
        detail: item.detail.clone(),
        documentation: item.documentation.clone().map(Documentation::String),
        text_edit: Some(CompletionTextEdit::Edit(TextEdit {
            range,
            new_text: item.insert_text.clone(),
        })),
        insert_text_format: Some(if item.is_snippet {
            InsertTextFormat::SNIPPET
        } else {
            InsertTextFormat::PLAIN_TEXT
        }),
        ..Default::default()
    }
}

fn map_kind(kind: &str) -> CompletionItemKind {
    match kind {
        "class" => CompletionItemKind::CLASS,
        "function" => CompletionItemKind::FUNCTION,
        "keyword" => CompletionItemKind::KEYWORD,
        "module" => CompletionItemKind::MODULE,
        "property" => CompletionItemKind::PROPERTY,
        "type" => CompletionItemKind::TYPE_PARAMETER,
        "variable" => CompletionItemKind::VARIABLE,
        _ => CompletionItemKind::TEXT,
    }
}
