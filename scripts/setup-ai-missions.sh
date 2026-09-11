#!/usr/bin/env bash
# prepare a Runinator installation for bounded Claude Code missions.
#
# usage:
#   bash scripts/setup-ai-missions.sh [options]
#
# options:
#   --api-base-url <url>       Runinator API URL (default RUNINATOR_API_BASE_URL or http://127.0.0.1:8080/)
#   --ctl <path>               runinatorctl binary (default PATH, then target/debug/runinatorctl)
#   --skip-tests               skip offline pack validation
#   --skip-profile-bootstrap   do not install the claude profile when it is absent
#   -h, --help                 show this help

set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
api_base_url="${RUNINATOR_API_BASE_URL:-http://127.0.0.1:8080/}"
ctl_path=""
skip_tests=0
skip_profile_bootstrap=0

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
    --skip-profile-bootstrap)
      skip_profile_bootstrap=1
      shift
      ;;
    -h|--help)
      sed -n '2,13p' "$0"
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
elif [[ -x "${ROOT_DIR}/target/debug/runinatorctl" ]]; then
  ctl=("${ROOT_DIR}/target/debug/runinatorctl")
else
  command -v cargo >/dev/null 2>&1 || {
    echo "runinatorctl is not installed and cargo is not available to build it" >&2
    exit 1
  }
  echo "Building runinatorctl..."
  cargo build --manifest-path "${ROOT_DIR}/Cargo.toml" -p runinator-ctl
  ctl=("${ROOT_DIR}/target/debug/runinatorctl")
fi

export RUNINATOR_API_BASE_URL="$api_base_url"

if [[ "$skip_tests" -eq 0 ]]; then
  echo "Validating mission packs offline..."
  "${ctl[@]}" workflows test "${ROOT_DIR}/packs/claude-availability"
  "${ctl[@]}" workflows test "${ROOT_DIR}/packs/ai-missions"
fi

echo "Connecting to ${RUNINATOR_API_BASE_URL}..."
if ! profile_listing="$("${ctl[@]}" execution-profiles list 2>&1)"; then
  printf '%s\n' "$profile_listing" >&2
  echo >&2
  echo "Could not query execution profiles. Check the API URL and authenticate first:" >&2
  echo "  RUNINATOR_API_BASE_URL=${RUNINATOR_API_BASE_URL} runinatorctl login" >&2
  exit 1
fi

claude_line="$(printf '%s\n' "$profile_listing" | awk '$2 == "claude" { print; exit }')"
if [[ -z "$claude_line" ]]; then
  if [[ "$skip_profile_bootstrap" -eq 1 ]]; then
    echo "No execution profile named 'claude' exists, and profile bootstrap was skipped." >&2
    exit 1
  fi
  echo "Installing the claude execution-profile definition and availability probe..."
  "${ctl[@]}" workflows apply "${ROOT_DIR}/packs/claude-availability"
fi

echo "Installing the coding and research/report mission recipes..."
"${ctl[@]}" workflows apply "${ROOT_DIR}/packs/ai-missions"

profile_listing="$("${ctl[@]}" execution-profiles list)"
claude_line="$(printf '%s\n' "$profile_listing" | awk '$2 == "claude" { print; exit }')"
claude_id="$(printf '%s\n' "$claude_line" | awk '{ print $1 }')"
claude_health="$(printf '%s\n' "$claude_line" | awk '{ print $3 }')"

echo
echo "Mission setup is installed."
if [[ "$claude_health" == "ready" ]]; then
  echo "The claude profile is ready. You can start a mission now."
else
  echo "The claude profile is installed but reports '${claude_health:-unknown}'."
  echo "Sign in with Claude Code on the desktop that runs runinator-desktop-agent, start the agent,"
  echo "select the claude profile, and press 'a' to approve its current configuration."
fi

if [[ -n "$claude_id" ]]; then
  echo
  echo "Inspect publication status:"
  printf '  %q execution-profiles status %q\n' "${ctl[0]}" "$claude_id"
  echo "Optional authenticated probe:"
  printf '  %q workflows run runinator.tests.claude.claude_availability\n' "${ctl[0]}"
fi

echo
echo "Next: docs/help/missions.md"
echo "Command Center: open Missions to start, inspect, or steer a mission."
