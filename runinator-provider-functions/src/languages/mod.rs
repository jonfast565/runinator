//! per-runtime knowledge: which image, and what shim loads a handler out of the package.
//!
//! a packaged function is ordinary code — it imports no runinator library and knows nothing about
//! runs. the shim is what bridges that: it reads the input file the provider wrote, resolves the
//! declared handler, calls it, and writes the result back. keeping it here rather than requiring it
//! in the package means a published version never has to be rebuilt when the calling convention
//! gains a field.

mod node;
mod python;

use runinator_models::errors::SendableError;

use crate::errors::UNKNOWN_RUNTIME;

/// how one language runtime is invoked.
pub trait RuntimeAdapter: Send + Sync {
    /// the canonical runtime family, e.g. `python`.
    fn family(&self) -> &'static str;
    /// the image used when the manifest names no explicit one.
    fn default_image(&self, version: &str) -> String;
    /// the shim's filename inside the runtime directory.
    fn shim_filename(&self) -> &'static str;
    /// the shim's source.
    fn shim_source(&self) -> &'static str;
    /// the argv that runs the shim, given the runtime directory inside the container.
    fn command(&self, runtime_dir: &str) -> Vec<String>;
}

/// resolve a manifest's `runtime` string, e.g. `python3.13` or `node22`.
///
/// the version is carried through to the default image rather than pinned here, so a package can
/// name a version this build has never heard of and still run.
pub fn adapter_for(runtime: &str) -> Result<(&'static dyn RuntimeAdapter, String), SendableError> {
    let runtime = runtime.trim().to_ascii_lowercase();
    let (family, version) = split_version(&runtime);
    let adapter: &'static dyn RuntimeAdapter = match family {
        "python" | "py" | "python3" => &python::Python,
        "node" | "nodejs" | "javascript" | "js" => &node::Node,
        other => {
            return Err(UNKNOWN_RUNTIME.error(format!(
                "runtime '{other}' is not supported; use python or node"
            )));
        }
    };
    Ok((adapter, version))
}

/// the image a runtime string resolves to when the manifest names none.
///
/// an operator can repoint a runtime without republishing every package that uses it, through
/// `RUNINATOR_FUNCTION_IMAGE_<FAMILY>` — which is the whole reason a manifest names a runtime rather
/// than an image in the first place.
pub fn default_image(runtime: &str) -> Result<String, SendableError> {
    let (adapter, version) = adapter_for(runtime)?;
    let override_key = format!(
        "RUNINATOR_FUNCTION_IMAGE_{}",
        adapter.family().to_uppercase()
    );
    if let Ok(image) = std::env::var(&override_key)
        && !image.trim().is_empty()
    {
        return Ok(image);
    }
    Ok(adapter.default_image(&version))
}

// split `python3.13` into ("python", "3.13"); a bare `python` yields an empty version, which each
// adapter turns into its own "latest supported" tag.
fn split_version(runtime: &str) -> (&str, String) {
    let boundary = runtime
        .char_indices()
        .find(|(_, character)| character.is_ascii_digit())
        .map(|(at, _)| at)
        .unwrap_or(runtime.len());
    let (family, version) = runtime.split_at(boundary);
    (family.trim_end_matches('-'), version.to_string())
}

#[cfg(test)]
#[path = "mod_tests.rs"]
mod tests;
