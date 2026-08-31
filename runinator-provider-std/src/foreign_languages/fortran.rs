use super::ForeignLanguageAdapter;

pub(super) static FORTRAN: Fortran = Fortran;

pub(super) struct Fortran;

impl ForeignLanguageAdapter for Fortran {
    fn canonical(&self) -> &'static str {
        "fortran"
    }

    fn source_filename(&self) -> &'static str {
        "foreign.f90"
    }

    fn runner_filename(&self) -> &'static str {
        "runinator_runner.sh"
    }

    fn runner_source(&self) -> &'static str {
        r#"set -euo pipefail
# JSON stream ABI: foreign.f90 is a complete Fortran program that reads one compact JSON document
# from stdin and writes exactly one JSON value to stdout. Diagnostics belong on stderr.
runner_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
gfortran -std=f2018 -O2 -Wall -Wextra -pedantic "${runner_dir}/foreign.f90" -o /tmp/runinator_foreign
/tmp/runinator_foreign "$@" < "${RUNINATOR_CONTEXT}" > "${RUNINATOR_OUTPUT}"
"#
    }

    fn execute(&self) -> &'static str {
        "bash /work/runinator_runner.sh"
    }
}
