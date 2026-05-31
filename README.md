# runinator

Runinator is a Rust workspace for scheduling and executing tasks across a small local/distributed runtime. The local development path uses `runinator-supervisor` to run the broker, web service, scheduler, worker, and importer together.

## Prerequisites

- Rust toolchain with Cargo.
- PowerShell 7+ if using `build.ps1`.
- kubectl if deploying to Kubernetes or launching the UI against a K8s stack.
- pnpm if you want to build or run the Tauri `runinator-command-center` app.

## Run Locally

The quickest path on macOS/Linux is:

```bash
bash scripts/run-local.sh start
```

That script runs `cargo build --workspace`, starts the supervisor in daemon mode, and prints process status. The web API listens at:

```text
http://127.0.0.1:8080/
```

Useful local commands:

```bash
bash scripts/run-local.sh status
bash scripts/run-local.sh watch
bash scripts/run-local.sh logs
bash scripts/run-local.sh logs --process web-service
bash scripts/run-local.sh logs-watch --lines 40
bash scripts/run-local.sh sync
bash scripts/run-local.sh ui
bash scripts/run-local.sh stop
bash scripts/run-local.sh restart
```

The supervisor starts the importer in one-shot mode, so the workflow file configured in `runinator-supervisor.json` is pushed into the API once after the web service is discovered. The checked-in local config imports `packs/sdlc/workflow-pack.json`. If the stack is already running and you want another sync, run:

```bash
bash scripts/run-local.sh sync
```

You can also run the supervisor directly:

```bash
cargo build --workspace
cargo run -p runinator-supervisor -- start
cargo run -p runinator-supervisor -- status
cargo run -p runinator-supervisor -- restart
cargo run -p runinator-supervisor -- stop
```

This uses `runinator-supervisor.json` to start:

- `runinator-broker`
- `runinator-ws`
- `runinator-scheduler`
- `runinator-worker`
- `runinator-importer`

The default worker configuration processes up to four actions concurrently. Tune
`--max-concurrent-actions` when long-running actions should not block unrelated
workflow action pickup.

The scheduler supports active/active scale-out when the web service is backed by
Postgres. Each scheduler claims workflow runs through the web service before
advancing them, so multiple scheduler replicas can run at the same time. SQLite
remains intended for local development and single-process stacks.

The local stack uses the built-in broker over raw TCP by default. The standalone
broker can also serve the same broker contract over HTTP by setting
`RUNINATOR_BROKER_TRANSPORT=http`; HTTP clients must use an endpoint like
`http://127.0.0.1:7070/`, while TCP clients use `127.0.0.1:7070`.
Kafka and RabbitMQ are available as feature-gated direct backends for the
scheduler, worker, and web service. Build those binaries with `--features kafka`
or `--features rabbitmq`, set `--broker-backend kafka|rabbitmq`, use
`--broker-endpoint` for Kafka bootstrap servers or the RabbitMQ AMQP URI, and
override `--broker-action-topic`, `--broker-control-topic`, or
`--broker-result-topic` when not using the default `runinator.*` topics/queues.
Do not scale the built-in `runinator-broker` process horizontally: each instance
has its own in-memory queue. For multi-broker high availability, run Kafka or
RabbitMQ and point every web-service, scheduler, and worker instance at the same
shared broker topics or queues.

The optional direct worker-to-scheduler control-event channel is disabled by
default. Enable it on the scheduler with `--worker-control-transport http|tcp`
plus bind/port flags, and on workers with `--scheduler-control-transport
http|tcp` plus the matching endpoint.

Local runtime files are written under `~/.runinator/` by default. This includes
the SQLite database at `~/.runinator/runinator.db`, credentials at
`~/.runinator/credentials.enc.json`, application logs under
`~/.runinator/logs/`, and supervisor state under `~/.runinator/supervisor/`.
When the importer is started without `--workflows-file`, it reads
`~/.runinator/workflows/workflow-pack.json`.
Child process stdout and stderr are collected under
`~/.runinator/supervisor/logs/` with one file per process start:

```text
YYYY-MM-DDTHH-MM-SS.mmmZ__process-name__attempt-N.log
```

Each file includes a supervisor start marker with the exact configured process name, command, and working directory, then the app's normal stdout/stderr output.

`watch` refreshes the status table. Use `logs-watch` or `logs --watch` to refresh log tails.

Use the supervisor log tail command to inspect the latest active log files:

```bash
cargo run -p runinator-supervisor -- logs
cargo run -p runinator-supervisor -- logs --process web-service --lines 100
cargo run -p runinator-supervisor -- logs --watch --lines 40
```

## PowerShell Local Run

PowerShell can build and run a local artifact layout:

```powershell
./build.ps1 -Mode Local -Run
```

This publishes binaries under `target/artifacts/`, writes `target/artifacts/runinator-supervisor.local.json`, then starts the stack in the foreground. Runtime state and the default workflow import file still go under `~/.runinator/` unless you pass explicit paths. Stop it with `Ctrl+C`.

## Workflow Import

The importer binary reads `~/.runinator/workflows/workflow-pack.json` by default
when `--workflows-file` is not set. The local supervisor config passes `--once`
and `--workflows-file ./packs/sdlc/workflow-pack.json`. Put a secret bundle at
`~/.runinator/secrets.json` to load local credentials during importer startup.
You can seed the app-data workflow bundle from the repository sample pack if
needed:

```bash
mkdir -p ~/.runinator/workflows
cp packs/sdlc/workflow-pack.json ~/.runinator/workflows/workflow-pack.json
```

Workflow syntax now includes richer declarative control-flow nodes:

- `switch` routes by ordered cases and an optional default target.
- `parallel` starts branch roots, with branch nodes returning to a `join`.
- `join` waits for named upstream nodes using `all`, `any`, or `first_success`.
- `try` runs a body, optional catch, and optional finally node; those nodes transition back to the `try` controller.
- `map` runs one target node for each resolved item and exposes the current item under `workflow.state.map`.
- `race` starts branch roots until one satisfies the winner policy; v1 does not cancel already dispatched work.
- `emit` records structured node output without calling a provider.
- `reentry` allows explicit bounded cycles back to a node and can route to `on_exhausted`.

The v1 control-flow runtime is controller-driven and still uses one `active_node_id`.
`parallel` and `race` advance branch roots sequentially through persisted workflow state,
and `map.concurrency` is reserved for a future multi-active-node runtime. Branch/body/item
nodes should transition back to their owning `join`, `try`, `map`, or `race` controller.

## Kubernetes

The Kubernetes manifests live under `deploy/k8s/` and are organized as a
kustomize base with two overlays:

```
deploy/k8s/
  base/                     # core manifests (namespace, services, postgres, rabbitmq, app deployments)
  overlays/local/           # k3d/minikube/kind — light replicas, default StorageClass
  overlays/prod/            # real registry + StorageClass + production resource sizing
```

The K8s stack uses **Postgres** in-cluster (StatefulSet + PVC) and **RabbitMQ**
as the broker (via the `rabbitmq` Cargo feature, baked into the ws/scheduler/
worker images). The standalone `runinator-broker` binary is not deployed in K8s.

Schema is applied by the `runinator-migration` image, which uses sqlx's built-in
migrator to run versioned SQL files from `runinator-database/migrations/`. The
`runinator-ws` Deployment runs migrations from an initContainer on every pod
start. `deploy/k8s/base/db-migrate-job.yaml` is kept as an optional
out-of-band ops manifest; it is not part of the default kustomize base because
Kubernetes Job pod templates are immutable across image tag changes.

### Quick start (local cluster)

```bash
# Builds the five K8s images, renders a temporary local overlay with matching
# image tags, applies it, and waits for Postgres, RabbitMQ, and app rollouts.
pwsh ./build.ps1 -DeployKube
```

The deploy waits up to 10 minutes for the importer Job to complete. Override
that when importing larger workflow packs:

```bash
pwsh ./build.ps1 -DeployKube -KubeImporterTimeoutSeconds 900
```

The local overlay includes development-only Postgres, RabbitMQ, and app
Secrets. For k3d/kind clusters that do not share Docker Desktop's image store,
configure a local registry and pass it as `-LocalRegistry localhost:5000` (or
use `-ImageRepository` for any registry reachable by the cluster).

### Production

Edit `deploy/k8s/overlays/prod/storage-class-patch.yaml` to set your cluster's
`storageClassName`, create the three Secrets from
`deploy/k8s/base/secrets.example.yaml`, then build, push, render, and apply the
prod overlay:

```bash
pwsh ./build.ps1 -DeployKube \
  -KubeManifest deploy/k8s/overlays/prod \
  -KubeContext my-prod-context \
  -ImageRepository registry.example.com/runinator \
  -ImageTag 1.0.0
```

See `deploy/k8s/overlays/{local,prod}/README.md` for details.

Launch the Tauri command center against the deployed K8s stack with one
command. The script starts a local port-forward to the `runinator-ws` Service,
waits for the API, passes the forwarded service URL to the app, and stops the
forward when the UI exits:

```bash
bash scripts/run-k8s.sh ui
```

Use `--context` or `--namespace` when the stack is not in the current kubectl
context's `runinator` namespace:

```bash
bash scripts/run-k8s.sh ui --context my-prod-context --namespace runinator
```

## Build Command Center

`runinator-command-center` is a Tauri client. Run it against the local stack with:

```bash
bash scripts/run-local.sh ui
```

The default local stack advertises and serves the API on `127.0.0.1:8080`.
For Kubernetes, gossip is disabled and the web service is available through the
`runinator-ws` Service instead. Use the K8s UI launcher to create the
port-forward and pass the concrete API URL:

```bash
bash scripts/run-k8s.sh ui
```

The command center checks `RUNINATOR_COMMAND_CENTER_SERVICE_URL`,
`RUNINATOR_SERVICE_URL`, then `WS_API_BASE_URL` before falling back to gossip.

## Package macOS Backend Apps

The Rust backend services remain normal command-line binaries. On macOS, you can
also package the runtime services as `.app` bundles with the Runinator icon:

```bash
cargo install cargo-packager --version 0.11.8 --locked
scripts/package-macos-backend-apps.sh --release
```

The script creates `.app` bundles for broker, web service, scheduler, worker,
importer, and supervisor under `target/macos-apps`.

## Verification

For importer workflow import changes, run:

```bash
jq empty packs/sdlc/workflow-pack.json
cargo test -p runinator-importer
```

To sync the seed file manually against a running local API:

```bash
bash scripts/run-local.sh sync
```

To verify rich workflow execution end-to-end against an isolated local stack:

```bash
RUNINATOR_E2E=1 cargo test -p runinator-e2e -- --ignored
```
