#!/usr/bin/env bash
# Port-forward the runinator-rabbitmq Service so the management (admin) UI and
# the amqp port are reachable from the host. The admin UI is opened in a browser
# unless --no-open is passed.
#
# Usage:
#   bash scripts/port-forward-rabbitmq.sh [--management-port 15672] [--port 5672] [--no-open]
#                                         [--namespace runinator] [--context <kubectl-ctx>]
#
# Then, e.g.:
#   open http://localhost:<management-port>   # queues, exchanges, message rates
#   RUNINATOR_RABBITMQ_URI=amqp://<user>:<pass>@127.0.0.1:<port>/%2f \
#     cargo test -p runinator-broker --features rabbitmq --test rabbitmq -- --ignored

set -euo pipefail

local_port=5672
management_port=15672
namespace="runinator"
context=""
service="runinator-rabbitmq"
remote_port=5672
remote_management_port=15672
open_ui=1

while [[ $# -gt 0 ]]; do
  case "$1" in
    --port)             local_port="$2"; shift 2 ;;
    --management-port)  management_port="$2"; shift 2 ;;
    --no-open)          open_ui=0; shift ;;
    --namespace)        namespace="$2"; shift 2 ;;
    --context)          context="$2"; shift 2 ;;
    --service)          service="$2"; shift 2 ;;
    --remote-port)      remote_port="$2"; shift 2 ;;
    -h|--help)
      sed -n '2,13p' "$0"
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
  echo "Service $namespace/$service not found. Deploy the stack first (e.g. cargo run -p xtask -- k8s deploy)." >&2
  exit 1
fi

# best-effort: show the credentials so the amqp uri can be pasted straight out.
user="<user>"
pass="<pass>"
if secret=$(kubectl ${ctx_args[@]+"${ctx_args[@]}"} -n "$namespace" get secret runinator-rabbitmq-secret -o json 2>/dev/null); then
  user=$(printf '%s' "$secret" | python3 -c 'import base64,json,sys; print(base64.b64decode(json.load(sys.stdin)["data"]["RABBIT_USER"]).decode())' 2>/dev/null || echo "<user>")
  pass=$(printf '%s' "$secret" | python3 -c 'import base64,json,sys; print(base64.b64decode(json.load(sys.stdin)["data"]["RABBIT_PASS"]).decode())' 2>/dev/null || echo "<pass>")
fi

echo "Admin UI:  http://localhost:${management_port}  (login ${user} / ${pass})"
echo "AMQP URI:  amqp://${user}:${pass}@127.0.0.1:${local_port}/%2f"
echo "Forwarding ${management_port}:${remote_management_port} (management) and ${local_port}:${remote_port} (amqp) -> ${namespace}/svc/${service}. Ctrl+C to stop."

# open the admin ui once the forward is actually accepting connections.
if [[ "$open_ui" -eq 1 ]]; then
  opener=""
  command -v open >/dev/null 2>&1 && opener="open"
  command -v xdg-open >/dev/null 2>&1 && opener="${opener:-xdg-open}"
  if [[ -n "$opener" ]]; then
    (
      for _ in $(seq 1 40); do
        if curl -sf -o /dev/null "http://localhost:${management_port}" 2>/dev/null; then
          "$opener" "http://localhost:${management_port}" >/dev/null 2>&1
          exit 0
        fi
        sleep 0.25
      done
    ) &
  fi
fi

exec kubectl ${ctx_args[@]+"${ctx_args[@]}"} -n "$namespace" port-forward "svc/${service}" \
  "${local_port}:${remote_port}" \
  "${management_port}:${remote_management_port}"
