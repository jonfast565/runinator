# Local overlay

Targets a single-node local cluster (Docker Desktop, k3d, minikube, kind).
The overlay includes development-only Secrets for Postgres, RabbitMQ, and the
app credential store. It also seeds a dev-only bootstrap admin plus a shared
service API key so the local `runinator-ws`, `runinator-worker`, `runinator-waker`,
and pack-import job can all run unchanged with `RUNINATOR_AUTH_ENABLED=true`.

Expects a default `StorageClass` (k3d ships `local-path`; kind needs the
local-path provisioner installed; minikube enables it by default).

## Scaling

The local overlay runs every service (ws, worker, waker, command-center) at 2
replicas with HorizontalPodAutoscalers (capped low for laptop capacity) and
PodDisruptionBudgets — the same multi-replica topology as prod, just smaller.
Every service runs as a Deployment; workers are stateless broker
competing-consumers with no persistent volumes, so they need no stable identity.
Install
[metrics-server](https://github.com/kubernetes-sigs/metrics-server) for the HPAs
to actually autoscale (minikube: `minikube addons enable metrics-server`).
Without it the services simply hold their replica count, and you can still scale
by hand:

```sh
kubectl -n runinator scale deploy/runinator-ws --replicas=3
kubectl -n runinator scale deploy/runinator-worker --replicas=3
```

Each worker pod also mounts a node-local AWS config directory at
`/var/runinator/aws`, which is exposed inside the container as
`/home/runinator/.aws` so AWS SSO profile and cache files are available to the
worker process. Point that node path at your real `~/.aws` using your local
cluster driver's mount or symlink support before applying the overlay.

The ws, waker, and worker pods also mount a node-local Claude config directory
at `/var/runinator/claude`, exposed inside each container as
`/home/runinator/.claude`, so the `claude` binary invoked by the AI command
(`runinator-provider-ai`) finds its config and credentials. It is mounted
read-write because Claude Code writes session/project state and refreshes its
OAuth token there. Point that node path at your real `~/.claude` using your
local cluster driver's mount or symlink support before applying the overlay.

The preferred end-to-end command is:

```sh
pwsh ./build.ps1 -DeployKube
```

For clusters that cannot see Docker Desktop's local image store, push through a
registry reachable from the cluster:

```sh
pwsh ./build.ps1 -DeployKube -LocalRegistry localhost:5000
```

To refresh only the command-center web interface, use:

```sh
pwsh ./build.ps1 -DeployKube -CommandCenterOnly
```

Open the Tauri command center against the deployed local cluster with:

```sh
bash scripts/run-k8s.sh ui
```

To inspect the raw API docs directly in a browser instead of launching the UI:

```sh
bash scripts/port-forward-ws.sh
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
