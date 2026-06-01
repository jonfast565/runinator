#!/usr/bin/env bash
# Deploy the runinator stack to a Kubernetes cluster using kustomize.
#
# Usage:
#   scripts/deploy-k8s.sh [--overlay local|prod] [--context <kubectl-ctx>] [--importer-timeout 600s] [--recreate-infra] [--delete]
#
# By default the postgres and rabbitmq StatefulSets are only applied when they
# do not already exist in the target namespace. Pass --recreate-infra to apply
# them again (which may roll the pods). The flag has no effect with --delete.
#
# Assumes images are already built and pushed or visible to the local cluster.
# For an end-to-end build and deploy, prefer:
#   pwsh ./build.ps1 -DeployKube
#
# Manual local images use the overlay's default dev tag:
#   docker build -t runinator-ws:dev       -f runinator-ws/Dockerfile       .
#   docker build -t runinator-waker:dev     -f runinator-waker/Dockerfile     .
#   docker build -t runinator-worker:dev    -f runinator-worker/Dockerfile    .
#   docker build -t runinator-importer:dev  -f runinator-importer/Dockerfile  .
#   docker build -t runinator-migration:dev -f runinator-migration/Dockerfile .

set -euo pipefail

overlay="local"
context=""
delete=0
recreate_infra=0
importer_timeout="600s"

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
        --importer-timeout)
            importer_timeout="$2"
            shift 2
            ;;
        --recreate-infra)
            recreate_infra=1
            shift
            ;;
        --delete)
            delete=1
            shift
            ;;
        -h|--help)
            sed -n '2,20p' "$0"
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

kubectl_ctx_args=()
if [[ -n "$context" ]]; then
    kubectl_ctx_args+=(--context "$context")
fi

echo "==> kubectl ${kubectl_ctx_args[*]:-} $verb -k $overlay_dir"
for stale_resource in deployment/runinator-importer job/runinator-importer service/runinator-gossip; do
    kubectl ${kubectl_ctx_args[@]+"${kubectl_ctx_args[@]}"} delete "$stale_resource" --namespace runinator --ignore-not-found=true >/dev/null 2>&1 || true
done

# decide which infra StatefulSets to skip on apply. existing ones are preserved
# unless --recreate-infra was passed, so re-deploys never disturb the running
# database or broker.
skip_pg=0
skip_mq=0
if [[ "$delete" -eq 0 && "$recreate_infra" -eq 0 ]]; then
    if kubectl ${kubectl_ctx_args[@]+"${kubectl_ctx_args[@]}"} get statefulset runinator-postgres --namespace runinator >/dev/null 2>&1; then
        skip_pg=1
        echo "==> preserving existing statefulset/runinator-postgres (pass --recreate-infra to override)"
    fi
    if kubectl ${kubectl_ctx_args[@]+"${kubectl_ctx_args[@]}"} get statefulset runinator-rabbitmq --namespace runinator >/dev/null 2>&1; then
        skip_mq=1
        echo "==> preserving existing statefulset/runinator-rabbitmq (pass --recreate-infra to override)"
    fi
fi

if [[ "$delete" -eq 1 ]]; then
    kubectl ${kubectl_ctx_args[@]+"${kubectl_ctx_args[@]}"} delete -k "$overlay_dir" --ignore-not-found=true
elif [[ "$skip_pg" -eq 0 && "$skip_mq" -eq 0 ]]; then
    kubectl ${kubectl_ctx_args[@]+"${kubectl_ctx_args[@]}"} apply -k "$overlay_dir"
else
    # render the overlay and drop the StatefulSet docs we want to preserve.
    # filter is by doc kind + metadata.name match within the same document so
    # the matching Services and Secrets are not affected.
    kubectl ${kubectl_ctx_args[@]+"${kubectl_ctx_args[@]}"} kustomize "$overlay_dir" | awk -v skip_pg="$skip_pg" -v skip_mq="$skip_mq" '
        function flush(d) {
            if (d == "") return
            is_sts = (index(d, "kind: StatefulSet\n") > 0)
            if (skip_pg && is_sts && index(d, "  name: runinator-postgres\n") > 0) return
            if (skip_mq && is_sts && index(d, "  name: runinator-rabbitmq\n") > 0) return
            print "---"
            printf "%s", d
        }
        /^---$/ { flush(doc); doc = ""; next }
        { doc = doc $0 "\n" }
        END { flush(doc) }
    ' | kubectl ${kubectl_ctx_args[@]+"${kubectl_ctx_args[@]}"} apply -f -
fi

if [[ "$verb" == "apply" ]]; then
    rollout_targets=(deployment/runinator-ws deployment/runinator-waker deployment/runinator-worker)
    if [[ "$skip_pg" -eq 0 ]]; then
        rollout_targets=(statefulset/runinator-postgres "${rollout_targets[@]}")
    fi
    if [[ "$skip_mq" -eq 0 ]]; then
        rollout_targets+=(statefulset/runinator-rabbitmq)
    fi
    for target in "${rollout_targets[@]}"; do
        if ! kubectl ${kubectl_ctx_args[@]+"${kubectl_ctx_args[@]}"} rollout status "$target" --namespace runinator --timeout 120s; then
            echo "warn: rollout check failed for $target" >&2
        fi
    done

    if ! kubectl ${kubectl_ctx_args[@]+"${kubectl_ctx_args[@]}"} wait --for=condition=complete job/runinator-importer --namespace runinator --timeout "$importer_timeout"; then
        echo "warn: importer job did not complete within timeout" >&2
    fi
fi
