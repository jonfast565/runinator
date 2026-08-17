#!/usr/bin/env bash
# Port-forward the runinator-command-center-web Service so the browser-mode
# command center is reachable from the host. The web pod's nginx already
# proxies /api/* and /ws/* to runinator-ws inside the cluster, so this is the
# only forward you need.
#
# Usage:
#   scripts/port-forward-command-center.sh [--port 8080] [--no-open]
#                                          [--namespace runinator] [--context <kubectl-ctx>]
#
# The command center is opened in a browser unless --no-open is passed.

set -euo pipefail

local_port=8080
namespace="runinator"
context=""
service="runinator-command-center"
remote_port=80
open_ui=1

while [[ $# -gt 0 ]]; do
  case "$1" in
    --port)        local_port="$2"; shift 2 ;;
    --no-open)     open_ui=0; shift ;;
    --namespace)   namespace="$2"; shift 2 ;;
    --context)     context="$2"; shift 2 ;;
    --service)     service="$2"; shift 2 ;;
    --remote-port) remote_port="$2"; shift 2 ;;
    -h|--help)
      sed -n '2,11p' "$0"
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
  echo "Service $namespace/$service not found. Deploy the stack first (e.g. cargo run -p xtask -- k8s deploy)." >&2
  exit 1
fi

echo "Command center: http://localhost:${local_port}"
echo "Forwarding ${local_port}:${remote_port} -> ${namespace}/svc/${service}. Ctrl+C to stop."

# open the command center once the forward is actually accepting connections.
if [[ "$open_ui" -eq 1 ]]; then
  opener=""
  command -v open >/dev/null 2>&1 && opener="open"
  command -v xdg-open >/dev/null 2>&1 && opener="${opener:-xdg-open}"
  if [[ -n "$opener" ]]; then
    (
      for _ in $(seq 1 40); do
        if curl -sf -o /dev/null "http://localhost:${local_port}" 2>/dev/null; then
          "$opener" "http://localhost:${local_port}" >/dev/null 2>&1
          exit 0
        fi
        sleep 0.25
      done
    ) &
  fi
fi

exec kubectl ${ctx_args[@]+"${ctx_args[@]}"} -n "$namespace" port-forward "svc/${service}" "${local_port}:${remote_port}"
