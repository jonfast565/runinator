use super::ForeignLanguageAdapter;

pub(super) static C_LANGUAGE: C = C;

pub(super) struct C;

impl ForeignLanguageAdapter for C {
    fn canonical(&self) -> &'static str {
        "c"
    }

    fn source_filename(&self) -> &'static str {
        "foreign.c"
    }

    fn runner_filename(&self) -> &'static str {
        "runinator_runner.sh"
    }

    fn runner_source(&self) -> &'static str {
        r#"set -euo pipefail
# JSON stream ABI: foreign.c is a complete C program that reads one JSON document from stdin
# and writes exactly one JSON value to stdout. Diagnostics belong on stderr.
runner_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
gcc -std=c17 -O2 -Wall -Wextra -Wpedantic "${runner_dir}/foreign.c" -o /tmp/runinator_foreign
/tmp/runinator_foreign < "${RUNINATOR_CONTEXT}" > "${RUNINATOR_OUTPUT}"
"#
    }

    fn execute(&self) -> &'static str {
        "bash /work/runinator_runner.sh"
    }
}
