#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'USAGE'
usage: scripts/package-macos-backend-apps.sh [--release] [--profile <profile>] [--skip-build] [--out-dir <path>] [--verbose]

Creates macOS .app bundles for the Runinator runtime binaries and desktop agent using cargo-packager.

Options:
  --release            Package target/release binaries.
  --profile <profile>  Package a custom cargo profile. Defaults to dev/debug.
  --skip-build         Do not build before packaging.
  --out-dir <path>     Output directory. Defaults to target/macos-apps.
  --verbose            Enable verbose cargo-packager logging.
  -h, --help           Show this help.
USAGE
}

profile="dev"
target_profile_dir="debug"
skip_build=0
out_dir=""
verbose=0

while [[ $# -gt 0 ]]; do
  case "$1" in
    --release)
      profile="release"
      target_profile_dir="release"
      shift
      ;;
    --profile)
      if [[ $# -lt 2 ]]; then
        echo "--profile requires a value." >&2
        exit 2
      fi
      profile="$2"
      target_profile_dir="$2"
      shift 2
      ;;
    --skip-build)
      skip_build=1
      shift
      ;;
    --out-dir)
      if [[ $# -lt 2 ]]; then
        echo "--out-dir requires a value." >&2
        exit 2
      fi
      out_dir="$2"
      shift 2
      ;;
    --verbose)
      verbose=1
      shift
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      echo "unknown argument: $1" >&2
      usage >&2
      exit 2
      ;;
  esac
done

if [[ "$(uname -s)" != "Darwin" ]]; then
  echo "macOS app bundles can only be created on macOS." >&2
  exit 1
fi

if ! cargo packager --version >/dev/null 2>&1; then
  echo "cargo-packager is required. Install it with: cargo install cargo-packager --version 0.11.8 --locked" >&2
  exit 1
fi

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
workspace_dir="$(cd "$script_dir/.." && pwd)"
cd "$workspace_dir"

if [[ -z "$out_dir" ]]; then
  out_dir="$workspace_dir/target/macos-apps"
elif [[ "$out_dir" != /* ]]; then
  out_dir="$workspace_dir/$out_dir"
fi

target_dir="$workspace_dir/target/$target_profile_dir"
icon_path="$workspace_dir/runinator-command-center/src-tauri/icons/icon.icns"

if [[ ! -f "$icon_path" ]]; then
  echo "missing icon: $icon_path" >&2
  exit 1
fi

if [[ "$skip_build" -eq 0 ]]; then
  cargo build \
    --profile "$profile" \
    -p runinator-broker \
    -p runinator-ws \
    -p runinator-waker \
    -p runinator-worker \
    -p runinator-desktop-agent \
    -p runinator-ctl \
    -p runinator-supervisor
fi

mkdir -p "$out_dir"
config_dir="$(mktemp -d "${TMPDIR:-/tmp}/runinator-packager.XXXXXX")"
trap 'rm -rf "$config_dir"' EXIT

apps=(
  "runinator-broker|Runinator Broker|dev.runinator.broker|Runinator broker service."
  "runinator-ws|Runinator Web Service|dev.runinator.web-service|Runinator HTTP API service."
  "runinator-waker|Runinator Waker|dev.runinator.waker|Runinator waker service."
  "runinator-worker|Runinator Worker|dev.runinator.worker|Runinator worker service."
  "runinator-desktop-agent|Runinator Desktop Agent|dev.runinator.desktop-agent|Runinator exclusive desktop worker and tray application."
  "runinatorctl|Runinator Control|dev.runinator.ctl|Runinator control and pack-import CLI."
  "runinator-supervisor|Runinator Supervisor|dev.runinator.supervisor|Runinator local stack supervisor."
)

for app in "${apps[@]}"; do
  IFS="|" read -r binary product_name identifier description <<< "$app"
  binary_path="$target_dir/$binary"
  app_out_dir="$out_dir/$binary"
  config_path="$config_dir/$binary.toml"

  if [[ ! -x "$binary_path" ]]; then
    echo "missing executable: $binary_path" >&2
    exit 1
  fi

  mkdir -p "$app_out_dir"

  cat > "$config_path" <<EOF
name = "$binary"
product-name = "$product_name"
version = "0.1.0"
identifier = "$identifier"
description = "$description"
formats = ["app"]
out-dir = "$app_out_dir"
binaries-dir = "$target_dir"
icons = ["$icon_path"]

[[binaries]]
path = "$binary"
main = true
EOF

  if [[ "$verbose" -eq 1 ]]; then
    cargo packager -vv --config "$config_path"
  else
    cargo packager --config "$config_path"
  fi
done
