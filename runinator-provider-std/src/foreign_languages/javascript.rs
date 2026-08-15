use super::ForeignLanguageAdapter;

pub(super) static JAVASCRIPT: JavaScript = JavaScript;

pub(super) struct JavaScript;

impl ForeignLanguageAdapter for JavaScript {
    fn canonical(&self) -> &'static str {
        "javascript"
    }

    fn source_filename(&self) -> &'static str {
        "foreign.mjs"
    }

    fn runner_filename(&self) -> &'static str {
        "runinator_runner.mjs"
    }

    fn runner_source(&self) -> &'static str {
        r#"import { readFile, writeFile } from "node:fs/promises";
import { main } from "./foreign.mjs";

const context = JSON.parse(await readFile(process.env.RUNINATOR_CONTEXT, "utf8"));
const result = await main(context);
const encoded = JSON.stringify(result);
if (encoded === undefined) {
  throw new TypeError("main(context) returned a value that cannot be represented as JSON");
}
await writeFile(process.env.RUNINATOR_OUTPUT, encoded, "utf8");
"#
    }

    fn execute(&self) -> &'static str {
        "node /work/runinator_runner.mjs"
    }
}
