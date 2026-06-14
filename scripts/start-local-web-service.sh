#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
BOOTSTRAP_BIN="$ROOT_DIR/target/debug/runinator-bootstrap"
WS_BIN="$ROOT_DIR/target/debug/runinator-ws"

database="${RUNINATOR_DATABASE:-sqlite}"
sqlite_path="${RUNINATOR_SQLITE_PATH:-}"
database_url="${RUNINATOR_DATABASE_URL:-}"

args=("$@")
index=0
while [[ $index -lt ${#args[@]} ]]; do
  arg="${args[$index]}"
  case "$arg" in
    --database)
      index=$((index + 1))
      database="${args[$index]}"
      ;;
    --sqlite-path)
      index=$((index + 1))
      sqlite_path="${args[$index]}"
      ;;
    --database-url)
      index=$((index + 1))
      database_url="${args[$index]}"
      ;;
  esac
  index=$((index + 1))
done

if [[ "$database" == "sqlite" ]]; then
  if [[ -z "$database_url" ]]; then
    if [[ -n "$sqlite_path" ]]; then
      database_url="$sqlite_path"
    else
      runinator_home="${RUNINATOR_HOME:-${HOME:-${USERPROFILE:-}}/.runinator}"
      database_url="$runinator_home/runinator.db"
    fi
  fi
  mkdir -p "$(dirname "$database_url")"
elif [[ -z "$database_url" ]]; then
  echo "missing connection string for bootstrap: pass --database-url or set RUNINATOR_DATABASE_URL" >&2
  exit 1
fi

bootstrap_args=(
  --database
  "$database"
  --database-url
  "$database_url"
)

if [[ -n "${RUNINATOR_AUTH_JWT_SECRET:-}" ]]; then
  bootstrap_args+=(--auth-jwt-secret "$RUNINATOR_AUTH_JWT_SECRET")
fi
if [[ -n "${RUNINATOR_AUTH_BOOTSTRAP_ADMIN:-}" ]]; then
  bootstrap_args+=(--auth-bootstrap-admin "$RUNINATOR_AUTH_BOOTSTRAP_ADMIN")
fi

"$BOOTSTRAP_BIN" "${bootstrap_args[@]}"
exec "$WS_BIN" "$@"
