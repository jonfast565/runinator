# Kubernetes deployment

Use this guide to deploy Runinator to a Kubernetes cluster, configure shared object storage and images, operate the local or production overlays, and rotate deployment keys safely.

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
as the broker (via the `rabbitmq` Cargo feature, baked into the ws/waker/
worker images). The standalone `runinator-broker` binary is not deployed in K8s;
it is built as `deploy/Dockerfile --target broker` for the single-host
`deploy/docker-compose.yml` topology, which swaps RabbitMQ for the broker's
built-in tcp transport.

### Object storage

`runinator-blob` is an S3-compatible object store that keeps function-package
artifacts and workflow run artifacts. It is deployed as its own service because
artifact bytes must be readable by every ws and engine replica: a path on
one replica's filesystem is invisible to the others, so a download routed to a
different pod would 404.

It speaks enough of the S3 REST API for the AWS CLI and SDKs to drive it —
path-style addressing, Signature V4 (header and presigned-query), object
PUT/GET/HEAD/DELETE with ranged reads, `ListObjectsV2`, and multipart upload —
so pointing `RUNINATOR_BLOB_ENDPOINT` at real S3, MinIO, or Ceph instead is a
configuration change rather than a code change. Storage is the container
filesystem, backed by a PVC.

| Variable | Meaning |
| --- | --- |
| `RUNINATOR_BLOB_ENDPOINT` | Where clients (ws and engine-worker) find the store. **Unset means "use a local directory"**, which is right for a workstation and wrong for a multi-replica deployment. |
| `RUNINATOR_BLOB_ADDR` | Listen address for the service itself (default `0.0.0.0:9000`). |
| `RUNINATOR_BLOB_DATA_DIR` | Where the service stores objects (default `/var/lib/runinator/blobs`). |
| `RUNINATOR_BLOB_ACCESS_KEY_ID` / `RUNINATOR_BLOB_SECRET_ACCESS_KEY` | The key pair. The service verifies signatures against it and clients sign with it, so the two must match or every artifact call is a 403. |
| `RUNINATOR_BLOB_CREDENTIALS` | A JSON array of `{access_key_id, secret_access_key}` for more than one key. |
| `RUNINATOR_BLOB_REGION` | Signing region (default `us-east-1`). A mismatch is a signature failure. |
| `RUNINATOR_BLOB_ALLOW_ANONYMOUS` | Accept unsigned requests. The local supervisor stack sets this; never set it on a reachable deployment. |
| `RUNINATOR_BLOB_MAX_OBJECT_BYTES` | Largest single-part upload (default 256 MiB). Larger objects go through multipart. |

To poke at it with the AWS CLI:

```bash
export AWS_ENDPOINT_URL=http://127.0.0.1:9100
export AWS_ACCESS_KEY_ID=... AWS_SECRET_ACCESS_KEY=... AWS_DEFAULT_REGION=us-east-1
aws s3 ls
aws s3 cp ./artifact.zip s3://runinator-function-artifacts/sha256/<digest>.zip
```

Two deliberate divergences from real S3: an ETag here is a quoted SHA-256 rather
than an MD5, and `STREAMING-AWS4-HMAC-SHA256-PAYLOAD` (per-chunk signing) is
refused rather than accepted without verification — send an unsigned payload
instead (`AWS_REQUEST_CHECKSUM_CALCULATION=when_required` for the CLI).

**Artifact storage.** Workflow run artifacts are stored here too. An artifact row's
`uri` is either a `blob://bucket/key` (everything written since the object store
landed) or an absolute path (everything before it). Both are readable; only the
first is written. The older form is why a download could 404 from a second ws
replica — the file was real, just not on that pod.

Workers relocate provider-produced artifacts through `POST /artifacts/content`
before publishing the artifact event, so the bytes outlive the worker that made
them. A failed relocation is not fatal: the local path is reported as before,
which keeps a completed node from failing over an artifact copy.

### Container images and plugins

Every rust service is one `--target` of the shared `deploy/Dockerfile`, so the
whole dependency graph compiles once for the entire set:

```bash
for t in ws engine-worker waker worker archiver blob ctl bootstrap broker; do
  docker build -f deploy/Dockerfile --target "$t" -t "runinator-$t:dev" .
done
```

`cargo run -p xtask -- k8s deploy` does this for you and pushes when
`--image-repository` is set. The builder mounts cargo's registry, git checkouts,
and the workspace `target/` as BuildKit caches, so an image rebuild after a
source edit is an *incremental* cargo build rather than a cold one. That syntax
requires BuildKit; xtask sets `DOCKER_BUILDKIT=1` on every invocation.

Kubernetes image builds compile only the chosen backend drivers. The defaults
are Postgres and RabbitMQ; choose `--database-backend sqlite|postgres|mysql|mariadb`
and `--broker-backend http|tcp|kafka|rabbitmq` when building a deployment. The
selected values must match the database and broker configured by the selected
Kustomize manifest. The bundled overlays provision only Postgres and RabbitMQ,
so other combinations need an overlay that points at the corresponding
external services.

Image binaries are **statically linked** (`+crt-static` is the musl default).
A static binary has no dynamic loader, so containerized workers cannot `dlopen`
a `.so` and ship no plugin directory — they run only the providers compiled into
them. Dynamic plugins are a host capability and are unaffected: `cargo run -p
xtask -- local up` still stages the console plugin into `~/.runinator/plugins/`,
and `runinator-desktop-agent` still loads plugins normally.

Schema is applied by the `runinator-bootstrap` image, which runs the embedded
SQL bootstrap from `runinator-database/migrations/` and can also seed the first
admin account when `RUNINATOR_AUTH_BOOTSTRAP_ADMIN` is provided. By default this
only seeds into an empty user table; set `RUNINATOR_AUTH_BOOTSTRAP_ADMIN_FORCE=true`
as a break-glass to reset that admin's password on the next bootstrap even when
users already exist (recovers a locked-out admin), then unset it. The
`runinator-ws` Deployment runs bootstrap from an initContainer on every pod
start. `deploy/k8s/base/db-bootstrap-job.yaml` is kept as an optional
out-of-band ops manifest; it is not part of the default kustomize base because
Kubernetes Job pod templates are immutable across image tag changes.

The bundled pack-import Job now logs in with the bootstrap-admin credentials
before it runs `workflows apply`, so `runinator-app-secret` must carry
`RUNINATOR_BOOTSTRAP_ADMIN_USERNAME` and `RUNINATOR_BOOTSTRAP_ADMIN_PASSWORD`
alongside `RUNINATOR_AUTH_BOOTSTRAP_ADMIN`.

For non-Kubernetes environments, `runinator-bootstrap` also supports
`--database mysql` / `--database mariadb` with a `mysql://...` connection string,
in addition to the existing SQLite and Postgres modes.

#### Key rotation (two-key overlap)

Both at-rest keys support a primary + previous overlap so a key can be rotated
without invalidating live tokens or stranding stored secrets:

- **JWT signing secret.** New access tokens are always signed with
  `RUNINATOR_AUTH_JWT_SECRET` (the primary); the web service also accepts tokens
  signed with `RUNINATOR_AUTH_JWT_SECRET_PREVIOUS` on verify. To rotate: set the
  new secret as the primary and the old one as `*_PREVIOUS`, redeploy so bootstrap
  persists both, wait past the access-token TTL, then clear `*_PREVIOUS` (bootstrap
  deletes the slot) and redeploy to retire the old key.
- **Credential encryption key.** Stored settings — including the JWT signing
  secret — are encrypted at rest with `RUNINATOR_CREDENTIAL_KEY` (the primary)
  and tagged with a short key id; `RUNINATOR_CREDENTIAL_KEY_PREVIOUS`
  (comma-separated) lists prior keys still accepted on decrypt. To rotate: set the
  new key as the primary and the old one as `*_PREVIOUS`, redeploy ws,
  `POST /credentials/reencrypt` (admin) to re-tag every stored value with the new
  key, then clear `*_PREVIOUS` and redeploy. A signing secret persisted before
  encryption was added is migrated to the encrypted form on the next bootstrap.
- **Rate limiting.** On by default; set `RUNINATOR_RATE_LIMIT_ENABLED=false` to
  disable. It gates the HTTP API with an in-memory token bucket keyed by the
  authenticated principal (falling back to the connection IP). Tune it with
  `RUNINATOR_RATE_LIMIT_RPS` (sustained requests per second, default `50`) and
  `RUNINATOR_RATE_LIMIT_BURST` (bucket size, default `100`). Each ws replica limits
  independently; `/health`, `/ready`, and `/metrics` are exempt. Over-limit
  requests get `429` with a `Retry-After` header. Independently, the unauthenticated
  `/auth/login` endpoint carries an always-on per-IP brute-force throttle (a small
  burst, then ~1 attempt every 5s) that cannot be disabled.
- **Overload protection.** On by default; set
  `RUNINATOR_OVERLOAD_PROTECTION_ENABLED=false` to disable. A global cap of
  `RUNINATOR_MAX_CONCURRENT_REQUESTS` (default `512`) in-flight requests sheds excess
  load with `503` + `Retry-After` instead of queueing it without bound, and
  `RUNINATOR_REQUEST_TIMEOUT_SECONDS` (default `30`) aborts a stuck handler with
  `408`. Each ws replica protects itself independently. This is the aggregate backstop
  the per-principal rate limiter above does not provide.
- **Database pool.** The Postgres/MySQL pool is bounded by
  `RUNINATOR_DB_MAX_CONNECTIONS` (default `20`) so a request flood cannot open
  unbounded server connections, and `RUNINATOR_DB_ACQUIRE_TIMEOUT_SECONDS`
  (default `30`) fails a checkout fast on a saturated pool rather than parking the
  caller. SQLite applies only the acquire timeout (its writes serialize, so more
  connections just add lock contention). Outbound API-client calls
  (`runinator-api`) carry their own `RUNINATOR_API_TIMEOUT_SECONDS` (default `60`)
  and `RUNINATOR_API_CONNECT_TIMEOUT_SECONDS` (default `10`).

### Quick start (local cluster)

```bash
# Builds the K8s images, renders a temporary local overlay with matching
# image tags, applies it, and waits for Postgres, RabbitMQ, and app rollouts.
cargo run -p xtask -- k8s deploy
```

For example, an overlay configured for an external MySQL database and Kafka
broker can build the matching runtime images with:

```bash
cargo run -p xtask -- k8s deploy \
  --manifest deploy/k8s/overlays/my-mysql-kafka \
  --database-backend mysql \
  --broker-backend kafka
```

The deploy waits up to 10 minutes for the pack-import Job to complete. Override
that when importing larger workflow packs:

```bash
cargo run -p xtask -- k8s deploy --pack-import-timeout-secs 900
```

The local overlay includes development-only Postgres, RabbitMQ, and app
Secrets. For k3d/kind clusters that do not share Docker Desktop's image store,
configure a local registry and pass it as `--local-registry localhost:5000` (or
use `--image-repository` for any registry reachable by the cluster).

Re-running `k8s deploy` against a cluster that already has the stack up
preserves the existing `runinator-postgres` and `runinator-rabbitmq`
StatefulSets by default, so redeploys don't roll your data stores. Pass
`--recreate-infra` when you actually want those StatefulSets re-applied (e.g.
after editing their manifests):

```bash
cargo run -p xtask -- k8s deploy --recreate-infra
```

To redeploy only the web interface, rebuild and apply just the
`runinator-command-center-web` resources with:

```bash
cargo run -p xtask -- k8s deploy --command-center-only
```

To refresh only Grafana after changing the dashboard or datasource manifests,
render the selected overlay, apply Grafana's ConfigMaps/Deployment/Service, and
wait for its rollout:

```bash
cargo run -p xtask -- k8s redeploy-grafana
```

To re-apply only PostgreSQL's Service and StatefulSet, without touching
RabbitMQ or the application workloads:

```bash
cargo run -p xtask -- k8s redeploy-database
```

To replace PostgreSQL with a completely empty database, use the explicit
destructive mode below. It scales only PostgreSQL down, deletes its generated
data PVC, recreates PostgreSQL, restarts the web service so its bootstrap
init-container applies the schema, and re-runs the bundled pack import. It does
not redeploy RabbitMQ or the other application workloads.

```bash
cargo run -p xtask -- k8s redeploy-database --from-scratch
```

Add `--skip-pack-import` when a schema-only empty database is intentional.

By default only the command-center is reachable from outside the cluster (it
proxies `/api` and `/ws` to the web service). To additionally expose the web
service API/websocket directly and open a debugging-only NodePort to Postgres,
pass `--expose-direct-ingress`:

```bash
cargo run -p xtask -- k8s deploy --expose-direct-ingress
```

This injects the `deploy/k8s/components/direct-ingress` component at render time
(it is never wired into a base/overlay, so prod stays closed unless you opt in).
It adds a host-based ingress for the web service at `api.runinator.local` and a
`NodePort` Service reaching Postgres on `<node-ip>:30432`. Leave the flag off for
any environment where the database must not be externally reachable.

Tear the stack back down with `cargo run -p xtask -- k8s delete` (same
`--manifest`/`--kube-context`/`--command-center-only` flags apply).

### Production

Edit `deploy/k8s/overlays/prod/storage-class-patch.yaml` to set your cluster's
`storageClassName`, create the three Secrets from
`deploy/k8s/base/secrets.example.yaml`, then build, push, render, and apply the
prod overlay:

```bash
cargo run -p xtask -- k8s deploy \
  --manifest deploy/k8s/overlays/prod \
  --kube-context my-prod-context \
  --image-repository registry.example.com/runinator \
  --image-tag 1.0.0
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

To open the raw web-service API or Scalar docs directly in a browser, forward
the `runinator-ws` Service on a separate local port:

```bash
bash scripts/port-forward-ws.sh
```

That exposes:

- `http://127.0.0.1:8081/docs`
- `http://127.0.0.1:8081/openapi.json`
