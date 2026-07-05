#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
COMMAND="${1:-ui}"
NAMESPACE="${RUNINATOR_K8S_NAMESPACE:-runinator}"
CONTEXT="${RUNINATOR_K8S_CONTEXT:-}"
WS_SERVICE="${RUNINATOR_K8S_WS_SERVICE:-runinator-ws}"
LOCAL_PORT="${RUNINATOR_K8S_UI_PORT:-}"
REMOTE_PORT="${RUNINATOR_K8S_WS_PORT:-8080}"
PORT_FORWARD_LOG="${TMPDIR:-/tmp}/runinator-k8s-ui-port-forward-$$.log"

if [[ $# -gt 0 ]]; then
  shift
fi

usage() {
  cat >&2 <<MSG
usage: bash scripts/run-k8s.sh ui [--namespace NAME] [--context NAME] [--port PORT] [--service NAME] [--remote-port PORT]

Runs the Tauri command center against a deployed Kubernetes stack.
MSG
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --namespace)
      NAMESPACE="${2:?--namespace requires a value}"
      shift 2
      ;;
    --context)
      CONTEXT="${2:?--context requires a value}"
      shift 2
      ;;
    --port)
      LOCAL_PORT="${2:?--port requires a value}"
      shift 2
      ;;
    --service)
      WS_SERVICE="${2:?--service requires a value}"
      shift 2
      ;;
    --remote-port)
      REMOTE_PORT="${2:?--remote-port requires a value}"
      shift 2
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      echo "unknown option: $1" >&2
      usage
      exit 2
      ;;
  esac
done

cd "$ROOT_DIR"

kubectl_args=()
if [[ -n "$CONTEXT" ]]; then
  kubectl_args+=(--context "$CONTEXT")
fi

require_command() {
  local command_name="$1"
  if ! command -v "$command_name" >/dev/null 2>&1; then
    echo "$command_name not on PATH" >&2
    exit 1
  fi
}

port_available() {
  local port="$1"
  if command -v lsof >/dev/null 2>&1; then
    ! lsof -nP -iTCP:"$port" -sTCP:LISTEN >/dev/null 2>&1
    return
  fi
  if command -v nc >/dev/null 2>&1; then
    ! nc -z 127.0.0.1 "$port" >/dev/null 2>&1
    return
  fi
  return 0
}

choose_port() {
  if [[ -n "$LOCAL_PORT" ]]; then
    if ! port_available "$LOCAL_PORT"; then
      echo "local port $LOCAL_PORT is already in use" >&2
      exit 1
    fi
    return
  fi

  local port
  for ((port = 18080; port <= 18120; port++)); do
    if port_available "$port"; then
      LOCAL_PORT="$port"
      return
    fi
  done

  echo "no free local port found in 18080-18120" >&2
  exit 1
}

wait_for_api() {
  local service_url="$1"
  local attempt

  for ((attempt = 1; attempt <= 60; attempt++)); do
    if curl -fsS "${service_url}ready" >/dev/null 2>&1; then
      return
    fi
    if ! kill -0 "$port_forward_pid" >/dev/null 2>&1; then
      echo "kubectl port-forward exited before the API became reachable" >&2
      if [[ -f "$PORT_FORWARD_LOG" ]]; then
        cat "$PORT_FORWARD_LOG" >&2
      fi
      exit 1
    fi
    sleep 1
  done

  echo "timed out waiting for ${service_url}ready" >&2
  if [[ -f "$PORT_FORWARD_LOG" ]]; then
    cat "$PORT_FORWARD_LOG" >&2
  fi
  exit 1
}

run_ui() {
  require_command kubectl
  require_command curl
  require_command pnpm
  choose_port

  if ! kubectl ${kubectl_args[@]+"${kubectl_args[@]}"} -n "$NAMESPACE" get svc "$WS_SERVICE" >/dev/null 2>&1; then
    echo "Service $NAMESPACE/$WS_SERVICE not found. Deploy the stack first with cargo run -p xtask -- k8s deploy." >&2
    exit 1
  fi

  : > "$PORT_FORWARD_LOG"
  kubectl ${kubectl_args[@]+"${kubectl_args[@]}"} -n "$NAMESPACE" port-forward "svc/${WS_SERVICE}" "${LOCAL_PORT}:${REMOTE_PORT}" >"$PORT_FORWARD_LOG" 2>&1 &
  port_forward_pid="$!"

  cleanup() {
    if kill -0 "$port_forward_pid" >/dev/null 2>&1; then
      kill "$port_forward_pid" >/dev/null 2>&1 || true
      wait "$port_forward_pid" >/dev/null 2>&1 || true
    fi
  }
  trap cleanup EXIT INT TERM

  local service_url="http://127.0.0.1:${LOCAL_PORT}/"
  echo "Forwarding ${service_url} -> ${NAMESPACE}/svc/${WS_SERVICE}:${REMOTE_PORT}"
  wait_for_api "$service_url"
  echo "Starting command center against ${service_url}"

  RUNINATOR_COMMAND_CENTER_SERVICE_URL="$service_url" VITE_RUNINATOR_WS_URL="$service_url" pnpm --dir runinator-command-center tauri dev
}

case "$COMMAND" in
  ui)
    run_ui
    ;;
  -h|--help)
    usage
    ;;
  *)
    usage
    exit 2
    ;;
esac
