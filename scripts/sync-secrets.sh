#!/usr/bin/env bash
# Build (if needed) and run the config-driven credential sync engine. It compiles
# the Swift Keychain helper (tools/keychain-export, macOS-only) and the Go engine
# (tools/runinator-secret-sync), puts the helper on PATH so a config can invoke
# `keychain-export` by name, then runs the engine against a JSON job spec.
#
# The engine authenticates through your local kubeconfig (so EKS exec-auth works)
# and reconciles each job's credential into the sinks named in the spec (namespaced
# Secrets the worker mounts via deploy/k8s/components/rotated-creds, and/or files).
#
# Usage:
#   scripts/sync-secrets.sh [--config <path>] [--no-build]
#                           [-- <extra runinator-secret-sync flags>]
#
# Defaults: --config tools/runinator-secret-sync/secret-sync.json
#
# Examples:
#   scripts/sync-secrets.sh --once --dry-run            # preview, write nothing
#   scripts/sync-secrets.sh --interval 5m               # watch and sync
#   scripts/sync-secrets.sh --config my-spec.json --once
#
# Any unrecognized flag (or anything after `--`) is forwarded verbatim to the
# engine; run `tools/runinator-secret-sync/bin/runinator-secret-sync -h` (or see
# its README) for the full flag and config surface.

set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
workspace="$(cd "${script_dir}/.." && pwd)"
swift_dir="${workspace}/tools/keychain-export"
go_dir="${workspace}/tools/runinator-secret-sync"
swift_bin_dir="${swift_dir}/.build/release"
go_bin="${go_dir}/bin/runinator-secret-sync"
default_config="${go_dir}/secret-sync.json"

build=1
config=""
engine_args=()

while [[ $# -gt 0 ]]; do
  case "$1" in
    --no-build) build=0; shift ;;
    --config)   config="$2"; shift 2 ;;
    -h|--help)
      sed -n '2,24p' "$0"
      exit 0
      ;;
    --)
      shift
      engine_args+=("$@")
      break
      ;;
    *)
      engine_args+=("$1"); shift
      ;;
  esac
done

if [[ -z "$config" ]]; then
  config="$default_config"
fi
if [[ ! -f "$config" ]]; then
  echo "error: config not found: $config" >&2
  echo "       copy the example: cp ${go_dir}/secret-sync.example.json ${default_config}" >&2
  exit 1
fi

# build the Swift Keychain helper (macOS only); a config that does not use it
# (no keychain-export command) still works on other platforms.
if [[ $build -eq 1 && "$(uname -s)" == "Darwin" ]]; then
  if command -v swift >/dev/null 2>&1; then
    echo "==> building keychain-export (release)"
    ( cd "$swift_dir" && swift build -c release )
  else
    echo "warning: swift not on PATH; skipping keychain-export build." >&2
  fi
fi

# build the Go engine.
if ! command -v go >/dev/null 2>&1; then
  echo "error: go toolchain not on PATH (needed to build the engine)." >&2
  exit 1
fi
if [[ $build -eq 1 || ! -x "$go_bin" ]]; then
  echo "==> building runinator-secret-sync"
  ( cd "$go_dir" && go build -o "$go_bin" ./... )
fi

# expose the freshly built keychain-export so configs can call it by bare name.
if [[ -d "$swift_bin_dir" ]]; then
  export PATH="${swift_bin_dir}:${PATH}"
fi

echo "==> runinator-secret-sync --config ${config} ${engine_args[*]+${engine_args[*]}}"
exec "$go_bin" --config "$config" "${engine_args[@]+"${engine_args[@]}"}"
