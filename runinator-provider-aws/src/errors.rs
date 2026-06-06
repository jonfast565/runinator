use runinator_models::errors::{ErrorDescriptor, ProviderErrors};

use crate::AwsProvider;

// numbered error dictionary for the aws provider. the dotted/upper `key` stays
// the runtime error code; the message renders as "AWS00N - <summary>: <detail>".
pub(crate) const UNSUPPORTED_CALL: ErrorDescriptor =
    ErrorDescriptor::new("AWS001", "UNSUPPORTED_CALL", "Unsupported provider call");
pub(crate) const DYNAMO_TIMEOUT: ErrorDescriptor =
    ErrorDescriptor::new("AWS002", "DYNAMO_TIMEOUT", "DynamoDB query timed out");
pub(crate) const MISSING_KEY_CONDITION: ErrorDescriptor = ErrorDescriptor::new(
    "AWS003",
    "MISSING_KEY_CONDITION",
    "Missing key condition expression",
);
pub(crate) const MISSING_PARTIQL_STATEMENT: ErrorDescriptor = ErrorDescriptor::new(
    "AWS004",
    "MISSING_PARTIQL_STATEMENT",
    "Missing PartiQL statement",
);
pub(crate) const INVALID_ATTRIBUTE_VALUE: ErrorDescriptor = ErrorDescriptor::new(
    "AWS005",
    "INVALID_ATTRIBUTE_VALUE",
    "Invalid DynamoDB attribute value",
);

pub(crate) const DICTIONARY: &[ErrorDescriptor] = &[
    UNSUPPORTED_CALL,
    DYNAMO_TIMEOUT,
    MISSING_KEY_CONDITION,
    MISSING_PARTIQL_STATEMENT,
    INVALID_ATTRIBUTE_VALUE,
];

impl ProviderErrors for AwsProvider {
    fn error_dictionary() -> &'static [ErrorDescriptor] {
        DICTIONARY
    }
}
