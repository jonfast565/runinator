#!/usr/bin/env bash
# Deploy the runinator stack to a Kubernetes cluster using kustomize.
#
# Usage:
#   scripts/deploy-k8s.sh [--overlay local|prod] [--context <kubectl-ctx>] [--pack-import-timeout 600s] [--recreate-infra] [--delete]
#
# By default the postgres and rabbitmq StatefulSets are only applied when they
# do not already exist in the target namespace. Pass --recreate-infra to apply
# them again (which may roll the pods). The flag has no effect with --delete.
#
# Assumes images are already built and pushed or visible to the local cluster.
# For an end-to-end build and deploy, prefer:
#   pwsh ./build.ps1 -DeployKube
#
# Manual local images use the overlay's default dev tag. all rust services share
# deploy/Dockerfile and are selected with --target; BuildKit caches the common
# builder stage so the cargo compile runs once for the whole set:
#   docker build -f deploy/Dockerfile --target ws        -t runinator-ws:dev        .
#   docker build -f deploy/Dockerfile --target waker     -t runinator-waker:dev     .
#   docker build -f deploy/Dockerfile --target worker    -t runinator-worker:dev    .
#   docker build -f deploy/Dockerfile --target ctl       -t runinator-ctl:dev       .
#   docker build -f deploy/Dockerfile --target migration -t runinator-migration:dev .

set -euo pipefail

overlay="local"
context=""
delete=0
recreate_infra=0
pack_import_timeout="600s"

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
        --pack-import-timeout)
            pack_import_timeout="$2"
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

kube() {
    if [[ "${#kubectl_ctx_args[@]}" -eq 0 ]]; then
        kubectl "$@"
        return
    fi

    kubectl "${kubectl_ctx_args[@]}" "$@"
}

print_rollout_diagnostics() {
    local target="$1"
    local workload_name="${target#*/}"
    local selector="app=$workload_name"
    local pod_name
    local pods=()

    echo "==> rollout diagnostics for $target" >&2
    echo "==> pod selector: $selector" >&2

    echo "==> selected pods" >&2
    kube get pods --namespace runinator --selector "$selector" --output wide --sort-by=.metadata.creationTimestamp >&2 || true

    while IFS= read -r pod_name; do
        if [[ -n "$pod_name" ]]; then
            pods+=("$pod_name")
        fi
    done < <(kube get pods --namespace runinator --selector "$selector" --output jsonpath='{range .items[*]}{.metadata.name}{"\n"}{end}' 2>/dev/null || true)

    echo "==> describe $target" >&2
    kube describe "$target" --namespace runinator >&2 || true

    for pod_name in "${pods[@]}"; do
        echo "==> describe pod/$pod_name" >&2
        kube describe "pod/$pod_name" --namespace runinator >&2 || true
    done

    echo "==> recent namespace events" >&2
    kube get events --namespace runinator --sort-by=.metadata.creationTimestamp 2>&1 | tail -n 40 >&2 || true

    if [[ "${#pods[@]}" -eq 0 ]]; then
        echo "==> no pods found for selector $selector; skipping container logs" >&2
        return
    fi

    for pod_name in "${pods[@]}"; do
        echo "==> recent logs for pod/$pod_name" >&2
        kube logs "pod/$pod_name" --namespace runinator --all-containers=true --tail=120 --prefix >&2 || true

        echo "==> recent previous logs for pod/$pod_name" >&2
        kube logs "pod/$pod_name" --namespace runinator --all-containers=true --tail=120 --prefix --previous >&2 || true
    done
}

echo "==> kubectl ${kubectl_ctx_args[*]:-} $verb -k $overlay_dir"
for stale_resource in deployment/runinator-importer job/runinator-importer job/runinator-pack-import service/runinator-gossip; do
    kube delete "$stale_resource" --namespace runinator --ignore-not-found=true >/dev/null 2>&1 || true
done

# decide which infra StatefulSets to skip on apply. existing ones are preserved
# unless --recreate-infra was passed, so re-deploys never disturb the running
# database or broker.
skip_pg=0
skip_mq=0
if [[ "$delete" -eq 0 && "$recreate_infra" -eq 0 ]]; then
    if kube get statefulset runinator-postgres --namespace runinator >/dev/null 2>&1; then
        skip_pg=1
        echo "==> preserving existing statefulset/runinator-postgres (pass --recreate-infra to override)"
    fi
    if kube get statefulset runinator-rabbitmq --namespace runinator >/dev/null 2>&1; then
        skip_mq=1
        echo "==> preserving existing statefulset/runinator-rabbitmq (pass --recreate-infra to override)"
    fi
fi

if [[ "$delete" -eq 1 ]]; then
    kube delete -k "$overlay_dir" --ignore-not-found=true
elif [[ "$skip_pg" -eq 0 && "$skip_mq" -eq 0 ]]; then
    kube apply -k "$overlay_dir"
else
    # render the overlay and drop the StatefulSet docs we want to preserve.
    # filter is by doc kind + metadata.name match within the same document so
    # the matching Services and Secrets are not affected.
    kube kustomize "$overlay_dir" | awk -v skip_pg="$skip_pg" -v skip_mq="$skip_mq" '
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
    ' | kube apply -f -
fi

if [[ "$verb" == "apply" ]]; then
    rollout_targets=(deployment/runinator-ws deployment/runinator-waker deployment/runinator-worker)
    if [[ "$skip_pg" -eq 0 ]]; then
        rollout_targets=(statefulset/runinator-postgres "${rollout_targets[@]}")
    fi
    if [[ "$skip_mq" -eq 0 ]]; then
        rollout_targets+=(statefulset/runinator-rabbitmq)
    fi
    rollout_failed=0
    for target in "${rollout_targets[@]}"; do
        if ! kube rollout status "$target" --namespace runinator --timeout 120s; then
            echo "error: rollout check failed for $target" >&2
            print_rollout_diagnostics "$target"
            rollout_failed=1
        fi
    done

    if [[ "$rollout_failed" -ne 0 ]]; then
        exit 1
    fi

    if ! kube wait --for=condition=complete job/runinator-pack-import --namespace runinator --timeout "$pack_import_timeout"; then
        echo "warn: pack-import job did not complete within timeout" >&2
    fi
fi
