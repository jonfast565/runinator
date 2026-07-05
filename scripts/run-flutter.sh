#!/usr/bin/env bash
# Build and run the runinator-command-center-flutter app.
#
# Only the web/ platform directory is checked in today, so the default
# target is Chrome. Pass --device to target something else (e.g. macos),
# but note that platforms with no scaffolding (macos/ios/android/linux/
# windows) need `flutter create --platforms=<name> .` run once first.
#
# Usage:
#   scripts/run-flutter.sh [--device chrome] [--release] [--build-only] [--clean] [-- <extra flutter run args>]
#
# Examples:
#   scripts/run-flutter.sh                       # flutter run -d chrome
#   scripts/run-flutter.sh --release              # release build, same device
#   scripts/run-flutter.sh --build-only            # flutter build web only, no run
#   scripts/run-flutter.sh --device macos          # run on macOS desktop (needs macos/ scaffold)

set -euo pipefail

app_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)/runinator-command-center-flutter"

device="chrome"
release=false
build_only=false
clean=false
extra_args=()

while [[ $# -gt 0 ]]; do
  case "$1" in
    --device)     device="$2"; shift 2 ;;
    --release)    release=true; shift ;;
    --build-only) build_only=true; shift ;;
    --clean)      clean=true; shift ;;
    -h|--help)
      sed -n '2,17p' "$0"
      exit 0
      ;;
    --)
      shift
      extra_args=("$@")
      break
      ;;
    *)
      echo "Unknown argument: $1" >&2
      exit 2
      ;;
  esac
done

if ! command -v flutter >/dev/null 2>&1; then
  echo "flutter was not found on PATH. Install Flutter and re-run this script." >&2
  exit 1
fi

cd "$app_dir"

if [[ "$clean" == true ]]; then
  echo "==> flutter clean"
  flutter clean
fi

echo "==> flutter pub get"
flutter pub get

if [[ "$build_only" == true ]]; then
  build_mode=("build" "web")
  if [[ "$release" == true ]]; then
    build_mode+=("--release")
  fi
  echo "==> flutter ${build_mode[*]}"
  exec flutter "${build_mode[@]}"
fi

run_args=("run" "-d" "$device")
if [[ "$release" == true ]]; then
  run_args+=("--release")
fi
if [[ ${#extra_args[@]} -gt 0 ]]; then
  run_args+=("${extra_args[@]}")
fi

echo "==> flutter ${run_args[*]}"
exec flutter "${run_args[@]}"
