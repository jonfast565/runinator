use super::ForeignLanguageAdapter;

pub(super) static CPP: Cpp = Cpp;

pub(super) struct Cpp;

impl ForeignLanguageAdapter for Cpp {
    fn canonical(&self) -> &'static str {
        "cpp"
    }

    fn source_filename(&self) -> &'static str {
        "foreign.cpp"
    }

    fn runner_filename(&self) -> &'static str {
        "runinator_runner.sh"
    }

    fn runner_source(&self) -> &'static str {
        r#"set -euo pipefail
# JSON stream ABI: foreign.cpp is a complete C++ program that reads one JSON document from stdin
# and writes exactly one JSON value to stdout. Diagnostics belong on stderr.
runner_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
g++ -std=c++20 -O2 -Wall -Wextra -Wpedantic "${runner_dir}/foreign.cpp" -o /tmp/runinator_foreign
/tmp/runinator_foreign "$@" < "${RUNINATOR_CONTEXT}" > "${RUNINATOR_OUTPUT}"
"#
    }

    fn execute(&self) -> &'static str {
        "bash /work/runinator_runner.sh"
    }
}
