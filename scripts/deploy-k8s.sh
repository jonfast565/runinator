#!/usr/bin/env bash
# Deploy the runinator stack to a Kubernetes cluster using kustomize.
#
# Usage:
#   scripts/deploy-k8s.sh [--overlay local|prod] [--context <kubectl-ctx>] [--delete]
#
# Assumes images are already built and pushed (or loaded into the local cluster).
# For local clusters, build images first:
#   docker build -t runinator-ws:dev       -f runinator-ws/Dockerfile       .
#   docker build -t runinator-scheduler:dev -f runinator-scheduler/Dockerfile .
#   docker build -t runinator-worker:dev    -f runinator-worker/Dockerfile    .
#   docker build -t runinator-importer:dev  -f runinator-importer/Dockerfile  .

set -euo pipefail

overlay="local"
context=""
delete=0

while [[ $# -gt 0 ]]; do
    case "$1" in
        --overlay)
            overlay="$2"
            shift 2
            ;;
        --context)
            context="$2"
            shift 2
            ;;
        --delete)
            delete=1
            shift
            ;;
        -h|--help)
            sed -n '2,11p' "$0"
            exit 0
            ;;
        *)
            echo "unknown arg: $1" >&2
            exit 2
            ;;
    esac
done

repo_root="$(cd "$(dirname "$0")/.." && pwd)"
overlay_dir="$repo_root/deploy/k8s/overlays/$overlay"

if [[ ! -d "$overlay_dir" ]]; then
    echo "overlay not found: $overlay_dir" >&2
    exit 1
fi

command -v kubectl >/dev/null || { echo "kubectl not on PATH" >&2; exit 1; }

verb="apply"
if [[ "$delete" -eq 1 ]]; then
    verb="delete"
fi

kubectl_args=()
if [[ -n "$context" ]]; then
    kubectl_args+=(--context "$context")
fi
kubectl_args+=("$verb" -k "$overlay_dir")

echo "==> kubectl ${kubectl_args[*]}"
kubectl "${kubectl_args[@]}"

if [[ "$verb" == "apply" ]]; then
    for dep in runinator-ws runinator-scheduler runinator-worker runinator-importer; do
        rollout_args=()
        if [[ -n "$context" ]]; then
            rollout_args+=(--context "$context")
        fi
        rollout_args+=(rollout status "deployment/$dep" --namespace runinator --timeout 120s)
        if ! kubectl "${rollout_args[@]}"; then
            echo "warn: rollout check failed for $dep" >&2
        fi
    done
fi
