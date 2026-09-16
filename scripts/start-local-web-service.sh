#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
BOOTSTRAP_BIN="${RUNINATOR_BOOTSTRAP_BIN:-runinator-bootstrap}"
WS_BIN="${RUNINATOR_WS_BIN:-runinator-ws}"

for executable in "$BOOTSTRAP_BIN" "$WS_BIN"; do
  if ! command -v "$executable" >/dev/null 2>&1; then
    echo "required executable is not on PATH: $executable" >&2
    echo "install it, set the matching RUNINATOR_*_BIN override, or use cargo run." >&2
    exit 1
  fi
done

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
if [[ -n "${RUNINATOR_AUTH_BOOTSTRAP_SERVICE_API_KEY:-}" ]]; then
  bootstrap_args+=(--auth-bootstrap-service-api-key "$RUNINATOR_AUTH_BOOTSTRAP_SERVICE_API_KEY")
fi
if [[ -n "${RUNINATOR_AUTH_BOOTSTRAP_SERVICE_API_KEY_NAME:-}" ]]; then
  bootstrap_args+=(--auth-bootstrap-service-api-key-name "$RUNINATOR_AUTH_BOOTSTRAP_SERVICE_API_KEY_NAME")
fi

"$BOOTSTRAP_BIN" "${bootstrap_args[@]}"
exec "$WS_BIN" "$@"
