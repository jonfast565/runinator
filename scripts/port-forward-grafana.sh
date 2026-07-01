#!/usr/bin/env bash
# Port-forward the runinator-grafana Service so the provisioned "Runinator
# Overview" dashboard is reachable from the host browser.
#
# Usage:
#   bash scripts/port-forward-grafana.sh [--port 3000] [--namespace runinator] [--context <kubectl-ctx>]
#
# Then open:
#   http://localhost:<port>   (anonymous admin; Prometheus + Jaeger datasources wired up)

set -euo pipefail

local_port=3000
namespace="runinator"
context=""
service="runinator-grafana"
remote_port=3000

while [[ $# -gt 0 ]]; do
  case "$1" in
    --port)        local_port="$2"; shift 2 ;;
    --namespace)   namespace="$2"; shift 2 ;;
    --context)     context="$2"; shift 2 ;;
    --service)     service="$2"; shift 2 ;;
    --remote-port) remote_port="$2"; shift 2 ;;
    -h|--help)
      sed -n '2,9p' "$0"
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

if ! command -v kubectl >/dev/null 2>&1; then
  echo "kubectl not on PATH" >&2
  exit 1
fi

if ! kubectl ${ctx_args[@]+"${ctx_args[@]}"} -n "$namespace" get svc "$service" >/dev/null 2>&1; then
  echo "Service $namespace/$service not found. Deploy the stack with the observability component enabled (it is on in the local overlay)." >&2
  exit 1
fi

echo "Forwarding http://localhost:${local_port} -> ${namespace}/svc/${service}:${remote_port}"
echo "Open http://localhost:${local_port} for the Runinator Overview dashboard. Ctrl+C to stop."

exec kubectl ${ctx_args[@]+"${ctx_args[@]}"} -n "$namespace" port-forward "svc/${service}" "${local_port}:${remote_port}"
