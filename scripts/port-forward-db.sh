#!/usr/bin/env bash
# Port-forward the runinator-postgres Service so the database is reachable from
# the host (psql, a GUI client, runinator-db-cli, etc.).
#
# Usage:
#   bash scripts/port-forward-db.sh [--port 5432] [--namespace runinator] [--context <kubectl-ctx>]
#                                     [--reconnect-delay 30]
#
# Then connect with the creds from your postgres secret, e.g.:
#   psql "postgresql://runinator:<password>@localhost:<port>/runinator"

set -euo pipefail

# shellcheck source=lib/port-forward.sh
source "$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/lib/port-forward.sh"

local_port=5432
namespace="runinator"
context=""
service="runinator-postgres"
remote_port=5432
reconnect_delay="${RUNINATOR_PORT_FORWARD_RECONNECT_DELAY:-30}"

while [[ $# -gt 0 ]]; do
  case "$1" in
    --port)        local_port="$2"; shift 2 ;;
    --namespace)   namespace="$2"; shift 2 ;;
    --context)     context="$2"; shift 2 ;;
    --service)     service="$2"; shift 2 ;;
    --remote-port) remote_port="$2"; shift 2 ;;
    --reconnect-delay) reconnect_delay="$2"; shift 2 ;;
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

port_forward_validate_reconnect_delay "$reconnect_delay"

ctx_args=()
if [[ -n "$context" ]]; then
  ctx_args=("--context" "$context")
fi

if ! command -v kubectl >/dev/null 2>&1; then
  echo "kubectl not on PATH" >&2
  exit 1
fi

if ! kubectl ${ctx_args[@]+"${ctx_args[@]}"} -n "$namespace" get svc "$service" >/dev/null 2>&1; then
  echo "Service $namespace/$service not found. Deploy the stack first (e.g. cargo run -p xtask -- k8s deploy)." >&2
  exit 1
fi

echo "Forwarding localhost:${local_port} -> ${namespace}/svc/${service}:${remote_port}"
echo "Connect with: psql \"postgresql://runinator:<password>@localhost:${local_port}/runinator\". Reconnects every ${reconnect_delay}s after a disconnect; Ctrl+C to stop."

port_forward_forever "$reconnect_delay" kubectl ${ctx_args[@]+"${ctx_args[@]}"} -n "$namespace" port-forward "svc/${service}" "${local_port}:${remote_port}"
