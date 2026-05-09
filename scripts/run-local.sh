#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
SUPERVISOR_ARGS=(-p runinator-supervisor --)
COMMAND="${1:-start}"
TASKS_FILE="${RUNINATOR_TASKS_FILE:-runinator-importer/tasks/tasks.json}"
LOG_PROCESS=""
LOG_LINES="${RUNINATOR_LOG_LINES:-80}"
IMPORTER_GOSSIP_PORT="${RUNINATOR_IMPORTER_ONCE_GOSSIP_PORT:-5513}"
IMPORTER_GOSSIP_TARGETS="${RUNINATOR_IMPORTER_ONCE_GOSSIP_TARGETS:-127.0.0.1:5510,127.0.0.1:5511,127.0.0.1:5512}"

if [[ $# -gt 0 ]]; then
  shift
fi

while [[ $# -gt 0 ]]; do
  case "$1" in
    --tasks-file)
      TASKS_FILE="${2:?--tasks-file requires a value}"
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
      echo "usage: bash scripts/run-local.sh [start|foreground|status|watch|logs|logs-watch|sync|stop|restart] [--tasks-file PATH] [--process NAME] [--lines N]" >&2
      exit 2
      ;;
  esac
done

cd "$ROOT_DIR"

sync_import() {
  cargo run -p runinator-importer -- \
    --once \
    --tasks-file "$TASKS_FILE" \
    --gossip-bind 127.0.0.1 \
    --gossip-port "$IMPORTER_GOSSIP_PORT" \
    --gossip-targets "$IMPORTER_GOSSIP_TARGETS"
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
    cargo build --workspace
    cargo run "${SUPERVISOR_ARGS[@]}" start
    cargo run "${SUPERVISOR_ARGS[@]}" status
    cat <<'MSG'

Runinator local stack is starting.

Web API:
  http://127.0.0.1:8080/

Useful commands:
  bash scripts/run-local.sh status
  bash scripts/run-local.sh watch
  bash scripts/run-local.sh logs
  bash scripts/run-local.sh logs --process web-service
  bash scripts/run-local.sh sync
  bash scripts/run-local.sh stop

Command-center:
  Build it with CMake/Qt from command-center/, then connect to the discovered local service.
  The importer seeds workflow "Mock SDLC: Feature Delivery" and mock Console tasks 101-106.
MSG
    ;;
  foreground)
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
  stop)
    cargo run "${SUPERVISOR_ARGS[@]}" stop
    ;;
  restart)
    cargo run "${SUPERVISOR_ARGS[@]}" stop || true
    cargo build --workspace
    cargo run "${SUPERVISOR_ARGS[@]}" start
    cargo run "${SUPERVISOR_ARGS[@]}" status
    ;;
  *)
    echo "usage: bash scripts/run-local.sh [start|foreground|status|watch|logs|logs-watch|sync|stop|restart] [--tasks-file PATH] [--process NAME] [--lines N]" >&2
    exit 2
    ;;
esac
