#!/usr/bin/env bash
# Open the runinatorctl console (the durable REXRAP repl), or serve the Model Context Protocol on
# stdin/stdout with --mcp, against the runinator-ws reached through scripts/port-forward-ws.sh.
# The forward is started here when the port is not already serving, and is torn down when the
# console (or the MCP server) exits. When the service enforces auth, the session signs in as
# admin/admin unless RUNINATOR_USERNAME/RUNINATOR_PASSWORD say otherwise.
#
# Usage:
#   scripts/start-runinatorctl.sh [options] [--] [console args...]
#   scripts/start-runinatorctl.sh --mcp [options] [--] [mcp args...]
#
# Options:
#   --mcp              serve `runinatorctl mcp` instead of the console. it speaks json-rpc on
#                      stdout, so every message this script prints moves to stderr
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
# Remaining arguments go to `runinatorctl console` (or `runinatorctl mcp` under --mcp), e.g.:
#   scripts/start-runinatorctl.sh --session my-session
#   scripts/start-runinatorctl.sh --new scratch
#   scripts/start-runinatorctl.sh --mcp --workflow-tools

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
subcommand="console"

while [[ $# -gt 0 ]]; do
  case "$1" in
    --mcp)       subcommand="mcp"; shift ;;
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
      sed -n '2,28p' "$0"
      exit 0
      ;;
    --) shift; break ;;
    *) break ;;
  esac
done

base_url="http://127.0.0.1:${local_port}/"

# the mcp server owns stdout for json-rpc, so a progress line printed there would desynchronise
# the client. everything this script says goes to stderr in that mode.
note() {
  if [[ "$subcommand" == "mcp" ]]; then
    echo "$@" >&2
  else
    echo "$@"
  fi
}

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
  note "Using the service already answering on http://127.0.0.1:${local_port}"
elif [[ "$start_forward" -eq 0 ]]; then
  echo "Nothing is serving http://127.0.0.1:${local_port} and --no-forward was given." >&2
  exit 1
else
  forward_args=("--port" "$local_port" "--namespace" "$namespace")
  if [[ -n "$context" ]]; then
    forward_args+=("--context" "$context")
  fi

  note "Starting port-forward to ${namespace}/svc/runinator-ws on ${local_port}..."
  if [[ "$subcommand" == "mcp" ]]; then
    bash "${ROOT_DIR}/scripts/port-forward-ws.sh" "${forward_args[@]}" >&2 &
  else
    bash "${ROOT_DIR}/scripts/port-forward-ws.sh" "${forward_args[@]}" &
  fi
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
  note "runinatorctl will sign in as ${username} if the service requires it."
else
  unset RUNINATOR_USERNAME RUNINATOR_PASSWORD
fi

if [[ "$subcommand" == "mcp" ]]; then
  note "runinatorctl mcp is bound to ${base_url}; speaking json-rpc on stdin/stdout."
  note "Close the client's connection to stop the port-forward."
else
  note "runinatorctl console is bound to ${base_url}"
  note "Exit the console to stop the port-forward."
fi
note ""

# prefer an already-built binary so opening the console does not pay for a cargo rebuild, and so
# an mcp client is not left waiting on a build before the first json-rpc frame. the command runs
# in the foreground rather than via exec, so the cleanup trap still tears the forward down when
# it exits.
ctl_bin="${ROOT_DIR}/target/${profile}/runinatorctl"
if [[ -x "$ctl_bin" ]]; then
  "$ctl_bin" "$subcommand" "$@"
  exit $?
fi

cargo_args=("run" "-q" "-p" "runinator-ctl")
if [[ "$profile" == "release" ]]; then
  cargo_args+=("--release")
fi
cd "$ROOT_DIR"
cargo "${cargo_args[@]}" -- "$subcommand" "$@"
