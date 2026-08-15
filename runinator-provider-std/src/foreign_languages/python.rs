use super::ForeignLanguageAdapter;

pub(super) static PYTHON: Python = Python;

pub(super) struct Python;

impl ForeignLanguageAdapter for Python {
    fn canonical(&self) -> &'static str {
        "python"
    }

    fn source_filename(&self) -> &'static str {
        "foreign.py"
    }

    fn runner_filename(&self) -> &'static str {
        "runinator_runner.py"
    }

    fn runner_source(&self) -> &'static str {
        r#"import asyncio
import importlib.util
import inspect
import json
import os
from pathlib import Path

source_path = Path(__file__).with_name("foreign.py")
spec = importlib.util.spec_from_file_location("runinator_foreign", source_path)
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)
if not hasattr(module, "main"):
    raise RuntimeError("foreign code must define main(context)")

with open(os.environ["RUNINATOR_CONTEXT"], encoding="utf-8") as context_file:
    context = json.load(context_file)

result = module.main(context)
if inspect.isawaitable(result):
    result = asyncio.run(result)

with open(os.environ["RUNINATOR_OUTPUT"], "w", encoding="utf-8") as output_file:
    json.dump(result, output_file, allow_nan=False)
"#
    }

    fn execute(&self) -> &'static str {
        "python /work/runinator_runner.py"
    }
}
