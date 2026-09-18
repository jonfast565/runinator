use runinator_models::errors::{ErrorDescriptor, ProviderErrors, SendableError};

use crate::provider::JiraProvider;

pub(crate) use runinator_jira::errors::{
    DICTIONARY, HTTP_ERROR, INVALID_PARAMS, IO_ERROR, MISSING_OPERATION_KEY, UNSUPPORTED_ACTION,
};

impl ProviderErrors for JiraProvider {
    fn error_dictionary() -> &'static [ErrorDescriptor] {
        DICTIONARY
    }
}

// Preserve the shared client's detailed cause without duplicating its descriptor.
pub(crate) fn client_error(context: &str, err: runinator_jira::errors::JiraError) -> SendableError {
    err.descriptor()
        .error(format!("{context}: {}", err.detail()))
}
