#!/usr/bin/env bash
# Port-forward the runinator-command-center-web Service so the browser-mode
# command center is reachable from the host. The web pod's nginx already
# proxies /api/* and /ws/* to runinator-ws inside the cluster, so this is the
# only forward you need.
#
# Usage:
#   scripts/port-forward-command-center.sh [--port 8080] [--namespace runinator] [--context <kubectl-ctx>]
#
# Then open http://localhost:<port> in a browser.

set -euo pipefail

local_port=8080
namespace="runinator"
context=""
service="runinator-command-center-web"
remote_port=80

while [[ $# -gt 0 ]]; do
  case "$1" in
    --port)        local_port="$2"; shift 2 ;;
    --namespace)   namespace="$2"; shift 2 ;;
    --context)     context="$2"; shift 2 ;;
    --service)     service="$2"; shift 2 ;;
    --remote-port) remote_port="$2"; shift 2 ;;
    -h|--help)
      sed -n '2,12p' "$0"
      exit 0
      ;;
    *)
      echo "Unknown argument: $1" >&2
      exit 2
      ;;
  esac
done

ctx_args=()
if [[ -n "$context" ]]; then
  ctx_args=("--context" "$context")
fi

if ! kubectl ${ctx_args[@]+"${ctx_args[@]}"} -n "$namespace" get svc "$service" >/dev/null 2>&1; then
  echo "Service $namespace/$service not found. Deploy the stack first (e.g. pwsh ./build.ps1 -DeployKube)." >&2
  exit 1
fi

echo "Forwarding http://localhost:${local_port} -> ${namespace}/svc/${service}:${remote_port}"
echo "Open http://localhost:${local_port} in a browser. Ctrl+C to stop."

exec kubectl ${ctx_args[@]+"${ctx_args[@]}"} -n "$namespace" port-forward "svc/${service}" "${local_port}:${remote_port}"
