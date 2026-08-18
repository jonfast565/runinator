#!/usr/bin/env bash
# Register `runinatorctl mcp` as an MCP server with Claude Code, so every runinatorctl command is
# available as a tool. Two shapes, matching how the web service is reached:
#
#   --local (default)  run the built runinatorctl directly against a web service on localhost
#   --k8s              run scripts/start-runinatorctl.sh --mcp, which brings up the port-forward,
#                      signs in, and tears the forward down when the client disconnects
#
# Usage:
#   scripts/install-mcp-claude-code.sh [options] [-- extra runinatorctl mcp args...]
#
# Options:
#   --local              target a web service on localhost (default)
#   --k8s                target the kubernetes service through the port-forward launcher
#   --name <name>        MCP server name (default runinator)
#   --scope <scope>      local | project | user (default project)
#   --url <url>          web service base url for --local (default http://127.0.0.1:8080)
#   --port <n>           forwarded local port for --k8s (default 8081)
#   --namespace <ns>     kubernetes namespace for --k8s (default runinator)
#   --release            use the release binary instead of the debug one
#   --api-key <key>      pass RUNINATOR_API_KEY to the server through the client's env
#   --workflow-tools     also advertise one tool per workflow (off by default)
#   --force              replace an existing registration of the same name
#
# Remaining arguments after `--` go to `runinatorctl mcp`.

set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

mode="local"
name="runinator"
scope="project"
base_url="http://127.0.0.1:8080"
local_port=8081
namespace="runinator"
profile="debug"
api_key=""
workflow_tools=0
force=0

while [[ $# -gt 0 ]]; do
  case "$1" in
    --local)          mode="local"; shift ;;
    --k8s)            mode="k8s"; shift ;;
    --name)           name="$2"; shift 2 ;;
    --scope)          scope="$2"; shift 2 ;;
    --url)            base_url="$2"; shift 2 ;;
    --port)           local_port="$2"; shift 2 ;;
    --namespace)      namespace="$2"; shift 2 ;;
    --release)        profile="release"; shift ;;
    --api-key)        api_key="$2"; shift 2 ;;
    --workflow-tools) workflow_tools=1; shift ;;
    --force)          force=1; shift ;;
    -h|--help)
      sed -n '2,27p' "$0"
      exit 0
      ;;
    --) shift; break ;;
    *)
      echo "Unknown option: $1 (see --help)" >&2
      exit 1
      ;;
  esac
done

if ! command -v claude >/dev/null 2>&1; then
  echo "The 'claude' CLI is not on PATH; install Claude Code first." >&2
  exit 1
fi

case "$scope" in
  local|project|user) ;;
  *) echo "Unknown scope '$scope' (expected local, project, or user)." >&2; exit 1 ;;
esac

# the server command itself, plus whatever the caller appended after `--`.
server_args=()
if [[ "$mode" == "k8s" ]]; then
  server_args+=("${ROOT_DIR}/scripts/start-runinatorctl.sh" "--mcp" "--port" "$local_port" "--namespace" "$namespace")
  if [[ "$profile" == "release" ]]; then
    server_args+=("--release")
  fi
  server_args+=("--")
else
  ctl_bin="${ROOT_DIR}/target/${profile}/runinatorctl"
  if [[ ! -x "$ctl_bin" ]]; then
    echo "No ${profile} binary at ${ctl_bin}; build it first:" >&2
    if [[ "$profile" == "release" ]]; then
      echo "  cargo build -p runinator-ctl --release" >&2
    else
      echo "  cargo build -p runinator-ctl" >&2
    fi
    exit 1
  fi
  server_args+=("$ctl_bin" "mcp" "--api-base-url" "$base_url")
fi

if [[ "$workflow_tools" -eq 1 ]]; then
  server_args+=("--workflow-tools")
fi
server_args+=("$@")

add_args=("mcp" "add" "$name" "--scope" "$scope")
if [[ -n "$api_key" ]]; then
  add_args+=("--env" "RUNINATOR_API_KEY=${api_key}")
fi

# `claude mcp add` refuses a name that is already registered, so removing first is what makes
# --force a re-install rather than an error.
if [[ "$force" -eq 1 ]]; then
  claude mcp remove "$name" --scope "$scope" >/dev/null 2>&1 || true
fi

cd "$ROOT_DIR"
claude "${add_args[@]}" -- "${server_args[@]}"

echo ""
echo "Registered '${name}' (${mode}, scope ${scope})."
echo "Verify with: claude mcp list    then /mcp inside a session."
