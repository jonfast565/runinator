#!/usr/bin/env bash
# Open the runinatorctl console (the durable WDL repl) against the runinator-ws reached
# through scripts/port-forward-ws.sh. The forward is started here when the port is not
# already serving, and is torn down when the repl exits. When the service enforces auth, the
# console signs in as admin/admin unless RUNINATOR_USERNAME/RUNINATOR_PASSWORD say otherwise.
#
# Usage:
#   scripts/start-runinatorctl.sh [options] [--] [console args...]
#
# Options:
#   --port <n>         local forwarded port (default 8081, matching port-forward-ws.sh)
#   --namespace <ns>   kubernetes namespace (default runinator)
#   --context <ctx>    kubectl context
#   --release          use/build the release binary instead of the debug one
#   --no-forward       assume something already serves the port; never start a forward
#   --timeout <sec>    how long to wait for the forward to answer (default 30)
#   --username <name>  login username (default $RUNINATOR_USERNAME, else admin)
#   --password <pass>  login password (default $RUNINATOR_PASSWORD, else admin)
#   --no-login         never log in; use whatever session/api key is already present
#
# Remaining arguments go to `runinatorctl console`, e.g.:
#   scripts/start-runinatorctl.sh --session my-session
#   scripts/start-runinatorctl.sh --new scratch

set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

local_port=8081
namespace="runinator"
context=""
profile="debug"
start_forward=1
timeout_seconds=30
username="${RUNINATOR_USERNAME:-admin}"
password="${RUNINATOR_PASSWORD:-admin}"
do_login=1

while [[ $# -gt 0 ]]; do
  case "$1" in
    --port)      local_port="$2"; shift 2 ;;
    --namespace) namespace="$2"; shift 2 ;;
    --context)   context="$2"; shift 2 ;;
    --release)   profile="release"; shift ;;
    --no-forward) start_forward=0; shift ;;
    --timeout)   timeout_seconds="$2"; shift 2 ;;
    --username)  username="$2"; shift 2 ;;
    --password)  password="$2"; shift 2 ;;
    --no-login)  do_login=0; shift ;;
    -h|--help)
      sed -n '2,23p' "$0"
      exit 0
      ;;
    --) shift; break ;;
    *) break ;;
  esac
done

base_url="http://127.0.0.1:${local_port}/"

# the forward is up as soon as something answers on the port; a 401 from an auth-enabled
# service still means the tunnel is live, so any http status counts and only curl's own
# "no response" code (000) does not.
service_answers() {
  local status
  status="$(curl -s -o /dev/null -m 2 -w '%{http_code}' "http://127.0.0.1:${local_port}/health" || true)"
  [[ -n "$status" && "$status" != "000" ]]
}

forward_pid=""
cleanup() {
  if [[ -n "$forward_pid" ]] && kill -0 "$forward_pid" 2>/dev/null; then
    kill "$forward_pid" 2>/dev/null || true
    wait "$forward_pid" 2>/dev/null || true
  fi
}
trap cleanup EXIT INT TERM

if service_answers; then
  echo "Using the service already answering on http://127.0.0.1:${local_port}"
elif [[ "$start_forward" -eq 0 ]]; then
  echo "Nothing is serving http://127.0.0.1:${local_port} and --no-forward was given." >&2
  exit 1
else
  forward_args=("--port" "$local_port" "--namespace" "$namespace")
  if [[ -n "$context" ]]; then
    forward_args+=("--context" "$context")
  fi

  echo "Starting port-forward to ${namespace}/svc/runinator-ws on ${local_port}..."
  bash "${ROOT_DIR}/scripts/port-forward-ws.sh" "${forward_args[@]}" &
  forward_pid=$!

  deadline=$((SECONDS + timeout_seconds))
  until service_answers; do
    if ! kill -0 "$forward_pid" 2>/dev/null; then
      echo "Port-forward exited before the service answered." >&2
      forward_pid=""
      exit 1
    fi
    if (( SECONDS >= deadline )); then
      echo "Timed out after ${timeout_seconds}s waiting for http://127.0.0.1:${local_port}/health." >&2
      exit 1
    fi
    sleep 0.5
  done
fi

export RUNINATOR_API_BASE_URL="$base_url"

# runinatorctl signs in on demand when the service enforces auth and no session is stored, so
# the credentials only need to be in the environment. the password goes through the env var
# rather than an argument so it stays out of the process listing.
if [[ "$do_login" -eq 1 ]]; then
  export RUNINATOR_USERNAME="$username"
  export RUNINATOR_PASSWORD="$password"
  echo "runinatorctl will sign in as ${username} if the service requires it."
else
  unset RUNINATOR_USERNAME RUNINATOR_PASSWORD
fi

echo "runinatorctl console is bound to ${base_url}"
echo "Exit the console to stop the port-forward."
echo

# prefer an already-built binary so opening the repl does not pay for a cargo rebuild. the
# repl runs in the foreground rather than via exec, so the cleanup trap still tears the
# forward down when it exits.
ctl_bin="${ROOT_DIR}/target/${profile}/runinatorctl"
if [[ -x "$ctl_bin" ]]; then
  "$ctl_bin" console "$@"
  exit $?
fi

cargo_args=("run" "-q" "-p" "runinator-ctl")
if [[ "$profile" == "release" ]]; then
  cargo_args+=("--release")
fi
cd "$ROOT_DIR"
cargo "${cargo_args[@]}" -- console "$@"
