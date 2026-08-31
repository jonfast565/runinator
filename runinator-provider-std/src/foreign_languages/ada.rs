use super::ForeignLanguageAdapter;

pub(super) static ADA: Ada = Ada;

pub(super) struct Ada;

impl ForeignLanguageAdapter for Ada {
    fn canonical(&self) -> &'static str {
        "ada"
    }

    fn source_filename(&self) -> &'static str {
        "runinator_foreign.adb"
    }

    fn runner_filename(&self) -> &'static str {
        "runinator_runner.sh"
    }

    fn runner_source(&self) -> &'static str {
        r#"set -euo pipefail
# JSON stream ABI: runinator_foreign.adb must define procedure Runinator_Foreign, read one compact
# JSON document from stdin, and write exactly one JSON value to stdout. Diagnostics use stderr.
runner_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
build_dir="$(mktemp -d)"
cp "${runner_dir}/runinator_foreign.adb" "${build_dir}/"
cd "${build_dir}"
gnatmake -q -gnat2022 -O2 -gnatwa runinator_foreign.adb -o /tmp/runinator_foreign
/tmp/runinator_foreign < "${RUNINATOR_CONTEXT}" > "${RUNINATOR_OUTPUT}"
"#
    }

    fn execute(&self) -> &'static str {
        "bash /work/runinator_runner.sh"
    }
}
