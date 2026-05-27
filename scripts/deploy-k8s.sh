#!/usr/bin/env bash
# Deploy the runinator stack to a Kubernetes cluster using kustomize.
#
# Usage:
#   scripts/deploy-k8s.sh [--overlay local|prod] [--context <kubectl-ctx>] [--delete]
#
# Assumes images are already built and pushed or visible to the local cluster.
# For an end-to-end build and deploy, prefer:
#   pwsh ./build.ps1 -DeployKube
#
# Manual local images use the overlay's default dev tag:
#   docker build -t runinator-ws:dev       -f runinator-ws/Dockerfile       .
#   docker build -t runinator-scheduler:dev -f runinator-scheduler/Dockerfile .
#   docker build -t runinator-worker:dev    -f runinator-worker/Dockerfile    .
#   docker build -t runinator-importer:dev  -f runinator-importer/Dockerfile  .
#   docker build -t runinator-migration:dev -f runinator-migration/Dockerfile .

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
            sed -n '2,16p' "$0"
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
if [[ "$delete" -eq 1 ]]; then
    kubectl_args+=(--ignore-not-found=true)
fi

echo "==> kubectl ${kubectl_args[*]}"
for stale_resource in deployment/runinator-importer job/runinator-importer; do
    cleanup_args=()
    if [[ -n "$context" ]]; then
        cleanup_args+=(--context "$context")
    fi
    cleanup_args+=(delete "$stale_resource" --namespace runinator --ignore-not-found=true)
    kubectl "${cleanup_args[@]}" >/dev/null 2>&1 || true
done
kubectl "${kubectl_args[@]}"

if [[ "$verb" == "apply" ]]; then
    for target in statefulset/runinator-postgres statefulset/runinator-rabbitmq \
        deployment/runinator-ws deployment/runinator-scheduler \
        deployment/runinator-worker; do
        rollout_args=()
        if [[ -n "$context" ]]; then
            rollout_args+=(--context "$context")
        fi
        rollout_args+=(rollout status "$target" --namespace runinator --timeout 120s)
        if ! kubectl "${rollout_args[@]}"; then
            echo "warn: rollout check failed for $target" >&2
        fi
    done

    wait_args=()
    if [[ -n "$context" ]]; then
        wait_args+=(--context "$context")
    fi
    wait_args+=(wait --for=condition=complete job/runinator-importer --namespace runinator --timeout 120s)
    if ! kubectl "${wait_args[@]}"; then
        echo "warn: importer job did not complete within timeout" >&2
    fi
fi
