//! what a compiled workflow records so a promotion never changes what it calls.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::{FUNCTIONS_NAMESPACE_PREFIX, PROVISIONAL_FUNCTION_VERSION};

mod function_binding;
pub use function_binding::FunctionBinding;

mod function_invocation_context;
pub use function_invocation_context::FunctionInvocationContext;
