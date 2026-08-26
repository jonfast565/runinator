//! what a compiled workflow records so a promotion never changes what it calls.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::{FUNCTIONS_NAMESPACE_PREFIX, PROVISIONAL_FUNCTION_VERSION};

/// the pinned identity of one packaged-function call, recorded on the action at compile time.
///
/// this is why moving an alias cannot reach into work that already exists: the binding names a
/// version and an artifact digest, and a workflow revision captures the whole definition, so an old
/// revision keeps calling exactly the code it was compiled against.
///
/// it carries names as well as ids on purpose. decompiling has to render
/// `functions.image_tools.resize(...)` from the action alone, and a decompiler that had to reach
/// into the catalog to do it would produce different text depending on what the catalog currently
/// holds — including nothing at all for a package that has since been deleted.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FunctionBinding {
    pub package_id: Uuid,
    pub package_name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub namespace: Option<String>,
    pub version_id: Uuid,
    pub version: i64,
    pub export_id: Uuid,
    pub export_name: String,
    pub artifact_digest: String,
}

impl FunctionBinding {
    /// True when this binding was produced while compiling a pack that also publishes its package.
    ///
    /// The pack import service must replace it with the real package/version/export UUIDs before
    /// validating and storing the workflow definition.
    pub fn is_provisional(&self) -> bool {
        self.version == PROVISIONAL_FUNCTION_VERSION
    }

    /// the authoring provider name this call was written as, e.g. `functions.image_tools`.
    pub fn provider_name(&self) -> String {
        match &self.namespace {
            Some(namespace) => {
                format!(
                    "{FUNCTIONS_NAMESPACE_PREFIX}{namespace}.{}",
                    self.package_name
                )
            }
            None => format!("{FUNCTIONS_NAMESPACE_PREFIX}{}", self.package_name),
        }
    }

    /// the dotted call this binding decompiles back to, e.g. `functions.image_tools.resize`.
    pub fn call_path(&self) -> String {
        format!("{}.{}", self.provider_name(), self.export_name)
    }
}

/// what a running invocation is told about itself.
///
/// passed into the container so packaged code can log, correlate, and emit artifacts against the
/// right run without being handed the control plane.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FunctionInvocationContext {
    pub package: String,
    pub export: String,
    pub version: i64,
    pub workflow_run_id: Uuid,
    pub workflow_node_run_id: Uuid,
    pub attempt: i64,
}
