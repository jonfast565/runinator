#[allow(unused_imports)]
use super::*;

/// one argument of one command, in the two shapes it has to be known in: a json property and a
/// command-line word.
#[derive(Debug, Clone)]
pub(crate) struct ToolArgument {
    /// the json property name, which is clap's argument id.
    pub key: String,
    pub form: Form,
    pub kind: Kind,
    /// what one value of it is, for the json type.
    pub scalar: Scalar,
    pub required: bool,
    pub description: String,
    /// the closed set of values, when clap knows one.
    pub values: Vec<String>,
    pub default: Option<String>,
}
