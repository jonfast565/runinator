use super::ForeignLanguageAdapter;

pub(super) static BASH: Bash = Bash;

pub(super) struct Bash;

impl ForeignLanguageAdapter for Bash {
    fn canonical(&self) -> &'static str {
        "bash"
    }

    fn source_filename(&self) -> &'static str {
        "foreign.sh"
    }

    fn runner_filename(&self) -> &'static str {
        "runinator_runner.sh"
    }

    fn runner_source(&self) -> &'static str {
        r#"set -euo pipefail
runner_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "${runner_dir}/foreign.sh"
declare -F main >/dev/null || { echo "foreign code must define main" >&2; exit 1; }
context="$(cat "${RUNINATOR_CONTEXT}")"
result="$(main "${context}")"
printf '%s' "${result}" > "${RUNINATOR_OUTPUT}"
"#
    }

    fn execute(&self) -> &'static str {
        "bash /work/runinator_runner.sh"
    }
}
