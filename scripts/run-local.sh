#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
SUPERVISOR_ARGS=(-p runinator-supervisor --)
COMMAND="${1:-start}"
WORKFLOWS_FILE="${RUNINATOR_WORKFLOWS_FILE:-$ROOT_DIR/packs/sdlc/sdlc.wdlp}"
SMOKE_WORKFLOWS_FILE="${RUNINATOR_SMOKE_WORKFLOWS_FILE:-$ROOT_DIR/packs/hello-world/hello-world.wdlp}"
SMOKE_WORKFLOW="${RUNINATOR_SMOKE_WORKFLOW:-Hello World Test}"
LOG_PROCESS=""
LOG_LINES="${RUNINATOR_LOG_LINES:-80}"
API_BASE_URL="${RUNINATOR_API_BASE_URL:-http://127.0.0.1:8080/}"
LOCAL_SERVICE_API_KEY_DEFAULT="${RUNINATOR_LOCAL_SERVICE_API_KEY:-localdev.runinator-local-dev-service-key}"
DEV_ARGS=()

if [[ $# -gt 0 ]]; then
  shift
fi

while [[ $# -gt 0 ]]; do
  case "$1" in
    --workflows-file)
      WORKFLOWS_FILE="${2:?--workflows-file requires a value}"
      shift 2
      ;;
    --smoke-workflows-file)
      SMOKE_WORKFLOWS_FILE="${2:?--smoke-workflows-file requires a value}"
      shift 2
      ;;
    --smoke-workflow)
      SMOKE_WORKFLOW="${2:?--smoke-workflow requires a value}"
      shift 2
      ;;
    --process)
      LOG_PROCESS="${2:?--process requires a value}"
      shift 2
      ;;
    --lines)
      LOG_LINES="${2:?--lines requires a value}"
      shift 2
      ;;
    *)
      if [[ "$COMMAND" == "dev" ]]; then
        DEV_ARGS+=("$1")
        shift
        continue
      fi
      echo "unknown option: $1" >&2
      echo "usage: bash scripts/run-local.sh [start|foreground|status|watch|logs|logs-watch|sync|dev|smoke-sync|ui|stop|restart] [--workflows-file PATH] [--smoke-workflows-file PATH] [--smoke-workflow NAME] [--process NAME] [--lines N]" >&2
      exit 2
      ;;
  esac
done

cd "$ROOT_DIR"

ensure_workflow_dir() {
  mkdir -p "$(dirname "$WORKFLOWS_FILE")"
}

sync_import() {
  ensure_workflow_dir
  local ctl_api_key="${RUNINATOR_API_KEY:-}"
  if [[ -z "$ctl_api_key" && "$API_BASE_URL" == "http://127.0.0.1:8080/" ]]; then
    ctl_api_key="$LOCAL_SERVICE_API_KEY_DEFAULT"
  fi
  RUNINATOR_API_KEY="$ctl_api_key" cargo run -p runinator-ctl -- \
    --api-base-url "$API_BASE_URL" \
    workflows apply "$WORKFLOWS_FILE"
}

smoke_sync() {
  WORKFLOWS_FILE="$SMOKE_WORKFLOWS_FILE"
  sync_import

  local output
  local ctl_api_key="${RUNINATOR_API_KEY:-}"
  if [[ -z "$ctl_api_key" && "$API_BASE_URL" == "http://127.0.0.1:8080/" ]]; then
    ctl_api_key="$LOCAL_SERVICE_API_KEY_DEFAULT"
  fi

  output="$(RUNINATOR_API_KEY="$ctl_api_key" cargo run -p runinator-ctl -- \
    --api-base-url "$API_BASE_URL" \
    workflows run "$SMOKE_WORKFLOW" \
    --name "hello-world-smoke")"
  printf '%s\n' "$output"

  local run_id
  run_id="$(printf '%s\n' "$output" | sed -n 's/.*workflow_run id=\([0-9][0-9]*\).*/\1/p' | tail -n 1)"
  if [[ -z "$run_id" ]]; then
    echo "failed to parse workflow run id from runinatorctl output" >&2
    exit 1
  fi

  RUNINATOR_API_KEY="$ctl_api_key" cargo run -p runinator-ctl -- \
    --api-base-url "$API_BASE_URL" \
    runs watch "$run_id" \
    --interval-seconds 1

  local summary
  summary="$(RUNINATOR_API_KEY="$ctl_api_key" cargo run -p runinator-ctl -- \
    --api-base-url "$API_BASE_URL" \
    runs show "$run_id")"
  printf '%s\n' "$summary"
  if ! grep -q 'status=succeeded' <<<"$summary"; then
    echo "hello-world smoke workflow run $run_id did not succeed" >&2
    exit 1
  fi
}

show_logs() {
  local watch_flag="${1:-}"
  local args=(logs --lines "$LOG_LINES")
  if [[ -n "$LOG_PROCESS" ]]; then
    args+=(--process "$LOG_PROCESS")
  fi
  if [[ "$watch_flag" == "watch" ]]; then
    args+=(--watch)
  fi
  cargo run "${SUPERVISOR_ARGS[@]}" "${args[@]}"
}

case "$COMMAND" in
  start)
    ensure_workflow_dir
    cargo build --workspace
    cargo run "${SUPERVISOR_ARGS[@]}" start
    cargo run "${SUPERVISOR_ARGS[@]}" status
    cat <<MSG

Runinator local stack is starting.

Web API:
  http://127.0.0.1:8080/

Useful commands:
  bash scripts/run-local.sh status
  bash scripts/run-local.sh watch
  bash scripts/run-local.sh logs
  bash scripts/run-local.sh logs --process web-service
  bash scripts/run-local.sh sync
  bash scripts/run-local.sh dev
  bash scripts/run-local.sh smoke-sync
  bash scripts/run-local.sh ui
  bash scripts/run-local.sh stop

Command-center:
  Run the Tauri UI with bash scripts/run-local.sh ui.
  The supervisor runs runinatorctl once on startup to import the workflow pack configured in runinator-supervisor.json.
  Use smoke-sync to import and run the tiny hello-world pack against an already running stack.
MSG
    ;;
  foreground)
    ensure_workflow_dir
    cargo build --workspace
    cargo run "${SUPERVISOR_ARGS[@]}" start --foreground
    ;;
  status)
    cargo run "${SUPERVISOR_ARGS[@]}" status
    ;;
  watch)
    cargo run "${SUPERVISOR_ARGS[@]}" status --watch
    ;;
  logs)
    show_logs
    ;;
  logs-watch)
    show_logs watch
    ;;
  sync)
    sync_import
    ;;
  dev)
    ensure_workflow_dir
    ctl_api_key="${RUNINATOR_API_KEY:-}"
    if [[ -z "$ctl_api_key" && "$API_BASE_URL" == "http://127.0.0.1:8080/" ]]; then
      ctl_api_key="$LOCAL_SERVICE_API_KEY_DEFAULT"
    fi
    RUNINATOR_API_KEY="$ctl_api_key" cargo run -p runinator-ctl -- \
      --api-base-url "$API_BASE_URL" \
      workflows dev "$WORKFLOWS_FILE" \
      "${DEV_ARGS[@]}"
    ;;
  smoke-sync)
    smoke_sync
    ;;
  ui)
    pnpm --dir runinator-command-center tauri dev
    ;;
  stop)
    cargo run "${SUPERVISOR_ARGS[@]}" stop
    ;;
  restart)
    ensure_workflow_dir
    cargo build --workspace
    cargo run "${SUPERVISOR_ARGS[@]}" restart
    cargo run "${SUPERVISOR_ARGS[@]}" status
    ;;
  *)
    echo "usage: bash scripts/run-local.sh [start|foreground|status|watch|logs|logs-watch|sync|dev|smoke-sync|ui|stop|restart] [--workflows-file PATH] [--smoke-workflows-file PATH] [--smoke-workflow NAME] [--process NAME] [--lines N]" >&2
    exit 2
    ;;
esac
