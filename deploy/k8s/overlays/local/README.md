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

AWS and Claude credentials reach the worker pod via the `components/rotated-creds`
component (enabled by this overlay), not a hostPath mount — Docker Desktop's local
cluster schedules replicas across several nodes, so a node-local hostPath is only
ever populated on whichever one you touched. `rotated-creds` instead mounts the
`aws-sso-cache` and `claude-credentials` Secrets, which `tools/runinator-secret-sync`
(run on your workstation, see `scripts/sync-secrets.sh` and `packs/creds-sync`) pushes
into the cluster from your local AWS SSO cache and Claude Code Keychain login. Run
that sync at least once after a fresh deploy (both Secrets are optional, so pods start
without them — the AWS/Claude actions just fail until the sync has run). Set your real
IAM Identity Center values in `deploy/k8s/components/rotated-creds/aws-config-configmap.yaml`
if you need working AWS SSO locally.

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
