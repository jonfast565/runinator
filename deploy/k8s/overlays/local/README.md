# Local overlay

Targets a single-node local cluster (Docker Desktop, k3d, minikube, kind).
The overlay includes development-only Secrets for Postgres, RabbitMQ, and the
app credential store.

Expects a default `StorageClass` (k3d ships `local-path`; kind needs the
local-path provisioner installed; minikube enables it by default).

## Scaling

The local overlay runs every service (ws, worker, waker, command-center) at 2
replicas with HorizontalPodAutoscalers (capped low for laptop capacity) and
PodDisruptionBudgets — the same multi-replica topology as prod, just smaller.
The worker runs as a StatefulSet behind a headless Service so each pod keeps a
stable ordinal identity and DNS name across restarts; ws/waker/command-center
stay Deployments. Install
[metrics-server](https://github.com/kubernetes-sigs/metrics-server) for the HPAs
to actually autoscale (minikube: `minikube addons enable metrics-server`).
Without it the services simply hold their replica count, and you can still scale
by hand:

```sh
kubectl -n runinator scale deploy/runinator-ws --replicas=3
kubectl -n runinator scale statefulset/runinator-worker --replicas=3
```

Each worker pod also mounts the host's `~/.aws` directory at
`/home/runinator/.aws` so AWS SSO profile and cache files are available to the
worker process.

The preferred end-to-end command is:

```sh
pwsh ./build.ps1 -DeployKube
```

For clusters that cannot see Docker Desktop's local image store, push through a
registry reachable from the cluster:

```sh
pwsh ./build.ps1 -DeployKube -LocalRegistry localhost:5000
```

Open the Tauri command center against the deployed local cluster with:

```sh
bash scripts/run-k8s.sh ui
```

If images are already available and you only need to apply the overlay:

```sh
kubectl apply -k deploy/k8s/overlays/local
```

## hostPath fallback

If your local cluster has no storage provisioner, replace the StatefulSets'
`volumeClaimTemplates` with hostPath volumes by adding this JSON patch to
`kustomization.yaml`:

```yaml
patches:
  - target: { kind: StatefulSet, name: runinator-postgres }
    patch: |-
      - op: remove
        path: /spec/volumeClaimTemplates
      - op: add
        path: /spec/template/spec/volumes
        value:
          - name: postgres-data
            hostPath:
              path: /var/runinator/postgres
              type: DirectoryOrCreate
```

(Repeat for `runinator-rabbitmq` with `rabbitmq-data` and `/var/runinator/rabbitmq`.)
