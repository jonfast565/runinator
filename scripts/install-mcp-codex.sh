#!/usr/bin/env bash
# Register `runinatorctl mcp` as an MCP server with Codex, so every runinatorctl command is
# available as a tool. Two shapes, matching how the web service is reached:
#
#   --local (default)  run the built runinatorctl directly against a web service on localhost
#   --k8s              run scripts/start-runinatorctl.sh --mcp, which brings up the port-forward,
#                      signs in, and tears the forward down when the client disconnects
#
# Usage:
#   scripts/install-mcp-codex.sh [options] [-- extra runinatorctl mcp args...]
#
# Options:
#   --local              target a web service on localhost (default)
#   --k8s                target the kubernetes service through the port-forward launcher
#   --name <name>        MCP server name (default runinator)
#   --scope <scope>      project | user (default project)
#   --url <url>          web service base URL for --local (default http://127.0.0.1:8080)
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

if ! command -v codex >/dev/null 2>&1; then
  echo "The 'codex' CLI is not on PATH; install Codex first." >&2
  exit 1
fi

case "$scope" in
  project)
    # Codex's project-scoped configuration lives at <project>/.codex/config.toml. CODEX_HOME
    # points the `codex mcp` subcommands at that file while this script performs the registration.
    mkdir -p "${ROOT_DIR}/.codex"
    export CODEX_HOME="${ROOT_DIR}/.codex"
    ;;
  user) ;;
  *) echo "Unknown scope '$scope' (expected project or user)." >&2; exit 1 ;;
esac

# The server command itself, plus whatever the caller appended after `--`.
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

# Codex replaces an existing server when `mcp add` is called again. Keep that operation explicit
# so a typo cannot silently change a project or user configuration.
if codex mcp get "$name" >/dev/null 2>&1; then
  if [[ "$force" -ne 1 ]]; then
    echo "MCP server '${name}' is already registered (use --force to replace it)." >&2
    exit 1
  fi
  codex mcp remove "$name" >/dev/null
fi

add_args=(mcp add "$name")
if [[ -n "$api_key" ]]; then
  add_args+=(--env "RUNINATOR_API_KEY=${api_key}")
fi

cd "$ROOT_DIR"
codex "${add_args[@]}" -- "${server_args[@]}"

echo ""
echo "Registered '${name}' (${mode}, scope ${scope})."
echo "Verify with: codex mcp list    then /mcp inside a session."
