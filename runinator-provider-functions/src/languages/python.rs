//! the python runtime adapter.

use super::RuntimeAdapter;

pub struct Python;

impl RuntimeAdapter for Python {
    fn family(&self) -> &'static str {
        "python"
    }

    fn default_image(&self, version: &str) -> String {
        let version = if version.is_empty() { "3.13" } else { version };
        format!("python:{version}-slim")
    }

    fn shim_filename(&self) -> &'static str {
        "_runinator_shim.py"
    }

    fn shim_source(&self) -> &'static str {
        SHIM
    }

    fn command(&self, runtime_dir: &str) -> Vec<String> {
        vec![
            "python".into(),
            "-B".into(),
            format!("{runtime_dir}/_runinator_shim.py"),
        ]
    }
}

// `-B` above and `sys.dont_write_bytecode` here are the same decision from both sides: the package
// is mounted read-only, so a runtime that tried to write `__pycache__` beside the source would fail
// on its first import.
const SHIM: &str = r#"""" loads and calls one packaged-function export. """
import importlib
import json
import os
import sys
import traceback

sys.dont_write_bytecode = True


def main() -> int:
    package_path = os.environ["RUNINATOR_PACKAGE"]
    handler = os.environ["RUNINATOR_HANDLER"]
    input_path = os.environ["RUNINATOR_INPUT"]
    output_path = os.environ["RUNINATOR_OUTPUT"]

    # the package is the import root, and it goes first: a package shipping its own `json.py` must
    # shadow the stdlib for its own code, exactly as it would when run directly.
    sys.path.insert(0, package_path)

    with open(input_path, "r", encoding="utf-8") as handle:
        payload = json.load(handle)

    if "." not in handler:
        raise ValueError("handler must be '<module>.<function>', got %r" % handler)
    module_name, _, function_name = handler.rpartition(".")
    module = importlib.import_module(module_name)
    function = getattr(module, function_name, None)
    if function is None:
        raise AttributeError("module %r has no export %r" % (module_name, function_name))

    arguments = payload.get("input") or {}
    if not isinstance(arguments, dict):
        raise TypeError("input must be an object, got %s" % type(arguments).__name__)

    result = function(**arguments)
    with open(output_path, "w", encoding="utf-8") as handle:
        json.dump(result, handle, default=str)
    return 0


if __name__ == "__main__":
    try:
        sys.exit(main())
    except Exception:
        # the traceback goes to stderr, which the runner captures as the failure detail; the exit
        # code is what the provider actually branches on.
        traceback.print_exc()
        sys.exit(1)
"#;
