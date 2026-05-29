use runinator_models::workflows::WorkflowDefinition;

use crate::ast::Document;
use crate::errors::WdlError;
use crate::CompileOptions;

pub fn lower_document(
    _document: &Document,
    _options: &CompileOptions,
) -> Result<WorkflowDefinition, WdlError> {
    Err(WdlError::lower("not implemented"))
}
