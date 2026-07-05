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

The archiver pod mounts a node-local archive directory at `/var/runinator/archive`,
exposed inside the container at `/var/lib/runinator/archive`, so the compressed
`jsonl.gz` archives it writes land on the real filesystem instead of an ephemeral
PVC (the base `runinator-archive-data` claim is dropped by this overlay). Point
that node path at your real `~/.runinator` using your local cluster driver's mount
or symlink support before applying the overlay.

The preferred end-to-end command is:

```sh
cargo run -p xtask -- k8s deploy
```

For clusters that cannot see Docker Desktop's local image store, push through a
registry reachable from the cluster:

```sh
cargo run -p xtask -- k8s deploy --local-registry localhost:5000
```

To refresh only the command-center web interface, use:

```sh
cargo run -p xtask -- k8s deploy --command-center-only
```

Open the Tauri command center against the deployed local cluster with:

```sh
bash scripts/run-k8s.sh ui
```

To inspect the raw API docs directly in a browser instead of launching the UI:

```sh
bash scripts/port-forward-ws.sh
```

## Flutter command center (testing)

`runinator-command-center-flutter` is the in-progress Flutter web port. It is
deployed only by this overlay (`command-center-flutter.yaml`), single-replica,
with no Ingress, so it never reaches prod and never competes with the Vue
command center's catch-all `/` route. `xtask k8s deploy` builds and
deploys it automatically alongside the rest of the stack (it is skipped for
overlays, like prod, whose `kustomization.yaml` doesn't declare the image). To
build and load it by hand instead:

```sh
docker build -t runinator-command-center-flutter:dev runinator-command-center-flutter
kubectl apply -k deploy/k8s/overlays/local
```

If your cluster driver can't see Docker Desktop's local image store (k3d,
kind), import the image first, e.g.:

```sh
k3d image import runinator-command-center-flutter:dev -c <cluster-name>
# or: kind load docker-image runinator-command-center-flutter:dev
```

Its nginx proxies `/api/*` and `/ws/*` to `runinator-ws` exactly like the Vue
command center, so no extra wiring is needed for it to talk to the backend.
Reach it with:

```sh
bash scripts/port-forward-command-center-flutter.sh
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
