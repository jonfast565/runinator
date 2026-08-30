#!/usr/bin/env bash
# Open the runinatorctl console (the durable REXRAP repl), or serve the MCP on
# stdin/stdout with --mcp, against a web service already reachable on localhost. To debug a
# Kubernetes cluster, first run scripts/port-forward-ws.sh and pass its local port with --port.
# When the service enforces auth, the session signs in as admin/admin unless
# RUNINATOR_USERNAME/RUNINATOR_PASSWORD say otherwise.
#
# Usage:
#   scripts/start-runinatorctl.sh [options] [--] [console args...]
#   scripts/start-runinatorctl.sh --mcp [options] [--] [MCP args...]
#
# Options:
#   --mcp              serve `runinatorctl mcp` instead of the console. it speaks JSON-RPC on
#                      stdout, so every message this script prints moves to stderr
#   --port <n>         local web-service port (default 8081, matching port-forward-ws.sh)
#   --release          use/build the release binary instead of the debug one
#   --username <name>  login username (default $RUNINATOR_USERNAME, else admin)
#   --password <pass>  login password (default $RUNINATOR_PASSWORD, else admin)
#   --no-login         never log in; use whatever session/API key is already present
#
# Remaining arguments go to `runinatorctl console` (or `runinatorctl mcp` under --mcp), e.g.:
#   scripts/start-runinatorctl.sh --session my-session
#   scripts/start-runinatorctl.sh --new scratch
#   scripts/start-runinatorctl.sh --mcp --workflow-tools

set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

local_port=8081
profile="debug"
username="${RUNINATOR_USERNAME:-admin}"
password="${RUNINATOR_PASSWORD:-admin}"
do_login=1
subcommand="console"

while [[ $# -gt 0 ]]; do
  case "$1" in
    --mcp)       subcommand="mcp"; shift ;;
    --port)      local_port="$2"; shift 2 ;;
    --release)   profile="release"; shift ;;
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

# The MCP server owns stdout for JSON-RPC, so a progress line printed there would desynchronise
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

if service_answers; then
  note "Using the service already answering on http://127.0.0.1:${local_port}"
else
  echo "Nothing is serving http://127.0.0.1:${local_port}." >&2
  echo "For a Kubernetes cluster, first run: bash scripts/port-forward-ws.sh --port ${local_port}" >&2
  exit 1
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
else
  note "runinatorctl console is bound to ${base_url}"
fi
note ""

# prefer an already-built binary so opening the console does not pay for a cargo rebuild, and so
# An MCP client is not left waiting on a build before the first JSON-RPC frame. The command runs
# in the foreground so its exit status is preserved.
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
