use super::ForeignLanguageAdapter;

pub(super) static COBOL: Cobol = Cobol;

pub(super) struct Cobol;

impl ForeignLanguageAdapter for Cobol {
    fn canonical(&self) -> &'static str {
        "cobol"
    }

    fn source_filename(&self) -> &'static str {
        "foreign.cob"
    }

    fn runner_filename(&self) -> &'static str {
        "runinator_runner.sh"
    }

    fn runner_source(&self) -> &'static str {
        r#"set -euo pipefail
# JSON ABI: foreign.cob is a complete free-format program; PROGRAM-ID must not be MAIN because
# GnuCOBOL reserves the generated C main symbol for the executable entry point.
runner_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cobc -x -free -Wall "${runner_dir}/foreign.cob" -o /tmp/runinator_foreign
/tmp/runinator_foreign < "${RUNINATOR_CONTEXT}" > "${RUNINATOR_OUTPUT}"
"#
    }

    fn execute(&self) -> &'static str {
        "bash /work/runinator_runner.sh"
    }
}
