#!/usr/bin/env bash
# validate and install the Codex execution profile and mission packs.
#
# usage:
#   bash scripts/setup-codex-missions.sh [options]
#
# options:
#   --api-base-url <url>  Runinator API URL (default RUNINATOR_API_BASE_URL or localhost)
#   --ctl <path>          runinatorctl executable (default PATH, then cargo run)
#   --skip-tests          skip offline pack validation
#   -h, --help            show this help

set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
api_base_url="${RUNINATOR_API_BASE_URL:-http://127.0.0.1:8080/}"
ctl_path=""
skip_tests=0

while [[ $# -gt 0 ]]; do
  case "$1" in
    --api-base-url)
      [[ $# -ge 2 ]] || { echo "--api-base-url requires a value" >&2; exit 2; }
      api_base_url="$2"
      shift 2
      ;;
    --ctl)
      [[ $# -ge 2 ]] || { echo "--ctl requires a value" >&2; exit 2; }
      ctl_path="$2"
      shift 2
      ;;
    --skip-tests)
      skip_tests=1
      shift
      ;;
    -h|--help)
      sed -n '2,11p' "$0"
      exit 0
      ;;
    *)
      echo "Unknown option: $1 (see --help)" >&2
      exit 2
      ;;
  esac
done

if [[ -n "$ctl_path" ]]; then
  [[ -x "$ctl_path" ]] || { echo "runinatorctl is not executable: $ctl_path" >&2; exit 1; }
  ctl=("$ctl_path")
elif command -v runinatorctl >/dev/null 2>&1; then
  ctl=("$(command -v runinatorctl)")
else
  command -v cargo >/dev/null 2>&1 || {
    echo "runinatorctl is not installed and cargo is not available" >&2
    exit 1
  }
  ctl=(cargo run -q --manifest-path "${ROOT_DIR}/Cargo.toml" -p runinator-ctl --)
fi

export RUNINATOR_API_BASE_URL="$api_base_url"

if [[ "$skip_tests" -eq 0 ]]; then
  "${ctl[@]}" workflows test "${ROOT_DIR}/packs/codex-availability"
  "${ctl[@]}" workflows test "${ROOT_DIR}/packs/codex-missions"
fi

echo "Installing Codex execution-profile definition and mission recipes..."
"${ctl[@]}" workflows apply "${ROOT_DIR}/packs/codex-availability"
"${ctl[@]}" workflows apply "${ROOT_DIR}/packs/codex-missions"

echo "Codex mission setup is installed. Approve and refresh the codex profile before starting work."
echo "See docs/help/missions.md for login, profile, and mission commands."
