#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
SUPERVISOR_ARGS=(-p runinator-supervisor --)
COMMAND="${1:-start}"
WORKFLOWS_FILE="${RUNINATOR_WORKFLOWS_FILE:-$ROOT_DIR/packs/sdlc/sdlc.wdlp}"
LOG_PROCESS=""
LOG_LINES="${RUNINATOR_LOG_LINES:-80}"
API_BASE_URL="${RUNINATOR_API_BASE_URL:-http://127.0.0.1:8080/}"

if [[ $# -gt 0 ]]; then
  shift
fi

while [[ $# -gt 0 ]]; do
  case "$1" in
    --workflows-file)
      WORKFLOWS_FILE="${2:?--workflows-file requires a value}"
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
      echo "unknown option: $1" >&2
      echo "usage: bash scripts/run-local.sh [start|foreground|status|watch|logs|logs-watch|sync|ui|stop|restart] [--workflows-file PATH] [--process NAME] [--lines N]" >&2
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
  cargo run -p runinator-ctl -- \
    --api-base-url "$API_BASE_URL" \
    workflows apply "$WORKFLOWS_FILE"
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
  bash scripts/run-local.sh ui
  bash scripts/run-local.sh stop

Command-center:
  Run the Tauri UI with bash scripts/run-local.sh ui.
  The supervisor runs runinatorctl once on startup to import the workflow pack configured in runinator-supervisor.json.
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
    echo "usage: bash scripts/run-local.sh [start|foreground|status|watch|logs|logs-watch|sync|ui|stop|restart] [--workflows-file PATH] [--process NAME] [--lines N]" >&2
    exit 2
    ;;
esac
