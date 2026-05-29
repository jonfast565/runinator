use runinator_models::workflows::WorkflowDefinition;

use crate::errors::WdlError;

pub fn decompile_definition(_definition: &WorkflowDefinition) -> Result<String, WdlError> {
    Err(WdlError::Decompile("not implemented".into()))
}
