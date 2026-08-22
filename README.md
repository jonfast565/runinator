# runinator

Runinator is a Rust workspace for scheduling and executing tasks across a small local/distributed runtime. The local development path uses `runinator-supervisor` to run the broker, web service (which embeds the engine), waker, and worker, plus a one-shot `runinatorctl` pack import.

## Prerequisites

- Rust toolchain with Cargo.
- Docker with Compose if using the local observability helper.
- kubectl if deploying to Kubernetes or launching the UI against a K8s stack.
- pnpm if you want to build or run the Tauri `runinator-command-center` app.

## Run Locally

The quickest path on macOS/Linux is:

```bash
bash scripts/run-local.sh start
```

To start the same supervisor stack with local OTLP export, Jaeger, and
Prometheus already wired up:

```bash
bash scripts/run-local.sh observe
```

That command starts the checked-in Docker Compose observability stack, sets
`OTEL_EXPORTER_OTLP_ENDPOINT=http://127.0.0.1:4318` for the supervisor daemon
and its child services, then starts the normal local Runinator processes.

That checked-in supervisor config defaults to SQLite, but the same local loop can
target a server database without editing JSON:

```bash
RUNINATOR_DATABASE=mysql \
RUNINATOR_DATABASE_URL='mysql://runinator:runinator@127.0.0.1:3306/runinator' \
bash scripts/run-local.sh start
```

Use `RUNINATOR_DATABASE=postgres` with a `postgres://` or `postgresql://` URL to
point the same loop at Postgres instead.

That script runs `cargo build --workspace`, starts the supervisor in daemon mode, and prints process status. The web API listens at:

```text
http://127.0.0.1:8080/
```

### API reference (OpenAPI)

The web service generates an OpenAPI 3.1 document automatically from `utoipa`
annotations on its handlers and serves it at:

- `http://127.0.0.1:8080/openapi.json` — the raw spec
- `http://127.0.0.1:8080/docs` — an interactive Scalar reference

Both are public (reachable without a credential). To document an endpoint, add a
`#[utoipa::path(...)]` attribute to its handler and list the handler in the
`paths(...)` set in `runinator-ws/src/openapi/mod.rs`; derive `ToSchema` on any struct
referenced by `body = ...`. Endpoints without an annotation still work — they are
simply absent from the spec until annotated, so coverage can grow incrementally.

Useful local commands:

```bash
bash scripts/run-local.sh start
bash scripts/run-local.sh foreground
bash scripts/run-local.sh status
bash scripts/run-local.sh watch
bash scripts/run-local.sh logs
bash scripts/run-local.sh logs --process web-service
bash scripts/run-local.sh logs-watch --lines 40
bash scripts/run-local.sh observe
bash scripts/run-local.sh observe-foreground
bash scripts/run-local.sh observability-start
bash scripts/run-local.sh observability-status
bash scripts/run-local.sh observability-logs
bash scripts/run-local.sh observability-stop
bash scripts/run-local.sh sync
bash scripts/run-local.sh dev
bash scripts/run-local.sh smoke-sync
bash scripts/run-local.sh ui
bash scripts/run-local.sh stop
bash scripts/run-local.sh restart
```

The supervisor runs `runinatorctl workflows apply` once per pack configured in `runinator-supervisor.json`, so those workflow packs are pushed into the API after the web service starts. The checked-in local config imports all three packs under `packs/` — `packs/sdlc/sdlc.rrx`, `packs/hello-world/hello-world.rrx`, and the `packs/creds-sync` directory — compiling the referenced `.rrx` files before sending each bundle to the API. The `creds-sync` workflows require a `runner=creds-sync` worker, so on the local stack their scheduled runs park then fail unless you start such a worker (see `packs/creds-sync/README.md`). It also advertises `127.0.0.1` for the web service, waker, and worker, and gives the waker and worker stable local instance ids so the replicas list shows host/IP/version data instead of blank fields on restart. Built-in provider metadata is seeded by the web service from the provider catalog on startup. If the stack is already running and you want another sync, run:

```bash
bash scripts/run-local.sh sync
```

The checked-in local supervisor config also seeds a bootstrap admin user into an empty database on first start:

```text
username: admin
password: admin
```

That seed happens even while HTTP auth is still disabled by default, so the usual local stack keeps working unchanged. If you later enable `RUNINATOR_AUTH_ENABLED=true` for the web service, you can immediately log in with that account and rotate it.

The same bootstrap step also seeds a dev-only service API key and feeds it to
the supervisor-managed development worker, waker, one-shot `runinatorctl workflows apply`,
and the `bash scripts/run-local.sh sync|dev|smoke-sync` helpers. That means the
default local stack continues to work unchanged with auth off, and starts
working against an auth-enabled local web service without hand-editing
`runinator-supervisor.json` or exporting extra env vars.

When auth is enabled, store a local CLI session with:

```bash
runinatorctl login
```

That prompts for a username and password, or takes them from the global `--username` option and
the `RUNINATOR_USERNAME`/`RUNINATOR_PASSWORD` environment variables. Those globals work on any
command, so a one-shot invocation or the console signs in on demand without a separate `login`
step:

```bash
RUNINATOR_PASSWORD=… runinatorctl --username admin status
```

Keep the password in the environment rather than on the command line so it stays out of shell
history and the process listing.

`runinatorctl` will refresh that session automatically on later commands and will ask you to log in before calling an auth-enabled server when no valid local session, credentials, or `--api-key` is available. Remove the stored session with:

```bash
runinatorctl logout
```

The local supervisor path runs `runinator-bootstrap` before `runinator-ws`, so
schema/auth bootstrap stays outside the web-service binary even in local
development.

Once authenticated, requests are authorized on two axes, both enforced
backend-side: named **capabilities** (a documented catalog of platform/org
privileges) gate privileged handlers, and **resource grants** (View/Run/Edit/Own)
gate individual workflows and pipelines — list responses are scoped to the
workflows the caller can see, and creators are stamped as owners. The command
center hides nav/panels and disables actions the caller lacks (via `GET /auth/me`),
but that never replaces backend enforcement. See [`docs/permissions.md`](docs/permissions.md)
for the full model.

For rapid REXRAP development, keep a pack compiling and re-importing on every save:

```bash
bash scripts/run-local.sh dev
```

Pass `--run` to create and watch a workflow run after each successful import:

```bash
bash scripts/run-local.sh dev --run "SDLC: Development"
```

When you only need to prove the local ws/waker/worker wiring with a tiny import
and one console action, use the hello-world smoke pack:

```bash
bash scripts/run-local.sh smoke-sync
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
- `runinator-blob`
- `runinator-ws`
- `runinator-waker`
- `runinator-worker`
- `runinatorctl workflows apply` (one-shot pack import)

The default worker configuration processes up to four actions concurrently. Tune
`--max-concurrent-actions` when long-running actions should not block unrelated
workflow action pickup.

### On-demand nodes

Provisionable node kinds can be spun up and scaled down on demand through the web
service's pluggable provisioner. Two backends are available: `supervisor` (adds
dynamic local processes through the running `runinator-supervisor` control queue)
and `kubernetes` (scales the backing Deployments via kube-rs; the ws image
must be built with `--features kubernetes` and the `runinator-ws-provisioner`
RBAC role applied). Enable a backend with `RUNINATOR_PROVISIONER_SUPERVISOR_ENABLED`
or `RUNINATOR_PROVISIONER_K8S_ENABLED`.

Each backend is configured per node kind, and the Node Pools panel lists the
provisionable kinds (`worker`, `waker`, `webservice`, and `postgres`);
kinds without a template/deployment on a backend show as non-manageable rows so a
newly added kind is always visible and becomes scalable the moment it is wired up.
The supervisor backend reads a spawn template per kind from
`RUNINATOR_PROVISIONER_SUPERVISOR_<KIND>` (e.g. `..._WORKER`, `..._WAKER`,
JSON `{ "command", "args", "env", "cwd" }`). The kubernetes
backend reads a deployment name per kind from
`RUNINATOR_PROVISIONER_K8S_<KIND>_DEPLOYMENT` (`worker`/`waker`/`ws` default to
`runinator-worker`/`-waker`/`-ws`, other kinds are opt-in).

Drive it from the CLI or the command center's Node Pools panel (Replicas view):

```bash
runinatorctl nodes list
runinatorctl nodes spin-up --backend supervisor --kind worker --count 2
runinatorctl nodes scale --backend kubernetes --kind worker --desired 5
runinatorctl nodes scale --backend kubernetes --kind webservice --desired 3
runinatorctl nodes stop --backend supervisor --node prov-worker-<id>
```

The Kubernetes provisioner can also observe and scale `postgres` when
`RUNINATOR_PROVISIONER_K8S_POSTGRES_STATEFULSET` is set, but scale-out above
one replica is intentionally blocked unless
`RUNINATOR_PROVISIONER_K8S_POSTGRES_SCALE_OUT_ENABLED=true` is also set. Only
enable that for a replication-aware Postgres topology with safe connection
routing, such as an operator-managed primary/replica cluster fronted by
PgBouncer. The checked-in `runinator-postgres` manifest is a single-primary
development StatefulSet and should not be scaled out as-is.

The continuation-driven graph interpreter lives in `runinator-runtime`; its
`WorkflowMachine` treats each durable cursor as a schedulable fiber and delegates
bookkeeping to a `WorkflowHost`. `runinator-engine` drives it over the broker.
The engine publishes scheduled work on the `wake` channel, and the
`runinator-waker` (a small, broker-only timer/relay) sleeps until each ready
node is due and then publishes a `drive` on the `ingress` channel for the
engine to consume.

The durable orchestration engine — the workflow VM driver and effect dispatcher,
the effect-result consumer and infrastructure-effect host, trigger and agent-directive
publishers, plus replica/usage/metrics/notification maintenance — lives in the
`runinator-engine` library crate. `runinator-engine-worker` is only an
optional host for that engine; it does not execute provider actions and it no
longer runs the removed legacy reducer/ingress loops. The engine can run in either
of two topologies. By default `runinator-ws` embeds it in-process
(`RUNINATOR_WS_RUN_ENGINE=true`), so the single-process local/dev/supervisor
stack needs no engine-worker process. Setting `RUNINATOR_WS_RUN_ENGINE=false`
makes ws serve HTTP/WebSocket only and offloads the engine to one or more standalone
`runinator-engine-worker` processes that talk to the same database and broker
directly (not the ws HTTP API), so HTTP replicas and engine replicas scale
independently. The engine is multi-replica safe: durable claims/leases
(`FOR UPDATE SKIP LOCKED`), shared-group result/ingress consumers, broker-deduped
wakes, and an idempotent per-window usage sampler let any number of
`runinator-engine-worker` (and/or engine-embedding ws) replicas run
active/active. Engine workers register as `background` replicas and appear in
the fleet/replica view. The Kubernetes base (`deploy/k8s/base`) and
`deploy/docker-compose.yml` ship the split topology; flip ws back to
`RUNINATOR_WS_RUN_ENGINE=true` and drop the engine-worker Deployment to fold it back
in-process. The waker holds no state and reaches the engine only over the
broker, so multiple waker replicas can run active/active. SQLite remains
the default for simple local development and single-process stacks. MariaDB and
Postgres are also supported for local development when you want a server-backed
database, and Postgres remains the intended path for multi-replica deployments.
All three run the same persistence test suite; see the dialect-parity section of
`AGENTS.md` for how to exercise the server-backed engines locally.

The local stack uses the built-in broker over raw TCP by default. The standalone
broker can also serve the same broker contract over HTTP by setting
`RUNINATOR_BROKER_TRANSPORT=http`; HTTP clients must use an endpoint like
`http://127.0.0.1:7070/`, while TCP clients use `127.0.0.1:7070`.
Kafka and RabbitMQ are available as feature-gated direct backends for the
waker, worker, web service, and engine worker. Build those binaries with `--features kafka`
or `--features rabbitmq`, set `--broker-backend kafka|rabbitmq`, use
`--broker-endpoint` for Kafka bootstrap servers or the RabbitMQ AMQP URI, and
override `--broker-action-topic`, `--broker-control-topic`,
`--broker-result-topic`, `--broker-wake-topic`, or `--broker-ingress-topic` when
not using the default `runinator.*` topics/queues.
Do not scale the built-in `runinator-broker` process horizontally: each instance
has its own in-memory queue. For multi-broker high availability, run Kafka or
RabbitMQ and point every web-service/engine-worker, waker, and worker instance at the same
shared broker topics or queues.

Worker-originated control requests travel to the engine over the broker
`ingress` channel; API-originated `cancel`/`pause`/`resume` requests pass through
the engine and reach workers over the `control` channel. There is no direct
worker-to-waker channel.

Local runtime files are written under `~/.runinator/` by default. When using
SQLite, this includes the database at `~/.runinator/runinator.db` (which also
holds config and secrets in the `settings` table, with each value encrypted at rest by
`RUNINATOR_CREDENTIAL_KEY`), application logs under `~/.runinator/logs/`, and
supervisor state under `~/.runinator/supervisor/`.
The local supervisor runs `runinatorctl workflows apply` against the pack at
`packs/sdlc/sdlc.rrx`.
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

## Cross-platform Local Run (xtask)

`xtask` is a plain Rust binary (`cargo run -p xtask -- <subcommand>`) that builds the
workspace and starts the local stack against the same checked-in
`runinator-supervisor.json` that `bash scripts/run-local.sh` uses, identically on Windows,
macOS, or Linux, with no PowerShell or Bash dependency:

```bash
cargo run -p xtask -- local up
```

This builds the workspace (unless `--skip-build`), makes sure the console plugin is
copied into `~/.runinator/plugins/` where the worker looks for it by default, then runs
`runinator-supervisor --config runinator-supervisor.json start --foreground` against the
`target/debug` binaries in place. There is only one local supervisor config either way you
start it. Stop it with `Ctrl+C`.

To run that same local stack against MariaDB, select the backend and pass a
MySQL-compatible URL (these become `RUNINATOR_DATABASE`/`RUNINATOR_DATABASE_URL`
environment variables for the web-service process, the same convention `bash
scripts/run-local.sh` documents above):

```bash
cargo run -p xtask -- local up \
  --database mariadb \
  --database-url 'mysql://runinator:runinator@127.0.0.1:3306/runinator'
```

`--database postgres` works the same way with a Postgres URL. SQLite continues
to use `--database-path` (defaults to `~/.runinator/runinator.db`). `cargo run
-p xtask -- build` on its own just builds the workspace plus the host-only
credential tools (`tools/keychain-export`, `tools/runinator-secret-sync`)
without starting anything.

## Packaged Functions

A *packaged function* is code published to the platform as an immutable, content-addressed
package and then invoked like any other action. A package directory holds a
`runinator-function.json` manifest plus whatever code it ships:

```json
{
  "name": "image-tools",
  "runtime": { "runtime": "python3.13" },
  "exports": [
    {
      "name": "resize",
      "handler": "src.images.resize",
      "input": [{ "name": "source", "type": "string", "required": true }],
      "output": [{ "name": "uri", "type": "string" }]
    }
  ]
}
```

`packs/image-tools` is a working example. The function manifest is JSON rather than TOML because
its schemas deserialize directly into the provider `ParameterMetadata` shape.

```bash
runinatorctl functions validate packs/image-tools   # offline: archive, check, print the digest
runinatorctl functions publish  packs/image-tools   # upload if needed, then publish a version
runinatorctl functions list
runinatorctl functions show     image-tools
runinatorctl functions alias    image-tools production --version 3
```

The archive is **deterministic**: entry order, timestamps, permissions, and the compression
method are all fixed, so the same tree always yields the same sha-256 on any machine and from any
checkout. That is what makes `publish` cheap — the digest is computed client-side, so an unchanged
package skips the upload entirely — and it is what lets a compiled workflow pin to *exact* bytes.
Build output, VCS state, and editor droppings are excluded by default; the manifest's `exclude`
list adds more (`*`, `?`, `**`, and a trailing `/` for a whole directory).

Versions are immutable and numbered per package. **Aliases are the only mutable part**: moving
`production` from version 3 to 4 changes what *new* calls resolve to and nothing else, because a
compiled workflow records the version and artifact digest it was compiled against. Publishing and
promotion are gated on the `functions:manage` capability.

Artifact bytes live in the object store (see `runinator-blob`), reachable by every ws replica and
every worker, and are addressed by digest at `/function_artifacts/{digest}`.

### Calling a function from a workflow

```
functions.image-tools.resize(source: params.url, width: 320)
```

No grammar change was needed — the REXRAP call syntax already reads every segment but the last as the
provider name. What compilation does is rewrite it: the runtime has one provider (`functions`) with
one action (`invoke`), and the *export* is named by a `FunctionBinding` attached to the action. That
binding records the exact version and artifact digest resolved at compile time, which is what makes
a later `alias` movement unable to reach into a workflow that already compiled. An unversioned call
takes the newest published version and pins it; nothing re-resolves it afterwards.

The call also gets `runner: functions` by default, so it only lands on a worker that can run
containers — an explicit `@runner(...)` wins, since an operator who pinned a pool meant it. Retry,
timeout, transitions, compensation, and idempotency all behave exactly as they do for any other
action, because after lowering it *is* one.

Publishing mirrors each package into the provider catalog as `functions.<pkg>` metadata, so
author-time typing and workflow validation keep working with the worker pool scaled to zero. Saving
a workflow — from the UI, a pack import, or a revision rollback — re-checks every binding against a
version that still exists, in an org the workflow may reach.

### Invoking a function over HTTP

```bash
curl -X POST .../functions/image-tools/resize \
     -H 'content-type: application/json' \
     -d '{"source":"a.png","width":320}'
```

This does **not** execute a container directly. Publishing generates one hidden single-node
*adapter workflow* per export, and the endpoint starts a run of it. Retry, timeout, cancellation,
logs, artifacts, tracing, and the debugger are all properties of a run; a second execution path
would need its own copy of every one and would drift from the workflow path immediately. So "invoke
over HTTP" and "call it from a workflow" are the same machinery reached two ways — which is what
makes them behave identically.

Adapters are generated by *compiling REXRAP*, not by assembling graph JSON, so the compiler guarantees
the graph is well-formed and the call carries exactly the binding an authored call would. They are
marked `metadata.managed_by = "functions"` and filtered out of the workflow list.

One deliberate difference from a workflow call: the HTTP path resolves its alias **now**
(`?alias=production`, or `?version=3` for an exact one), because an HTTP caller asking for
`production` means whatever `production` is today. A compiled workflow pinned its version when it
compiled.

By default the request waits briefly for a terminal status and returns the output inline, falling
back to `202` with a run id if the run is still going; `Prefer: respond-async` skips the wait
entirely. `Idempotency-Key` replays the run an earlier identical request started, scoped per org and
export. `GET /function_invocations/{run_id}` and `POST /function_invocations/{run_id}/cancel` read
and cancel one.

### In the command center

**Functions** lists published packages and, for the selected one, its aliases, exports, and version
history. Moving an alias is the one mutable act available there — publishing happens through
`runinatorctl`, because a publish uploads bytes and the digest is computed client-side.

**Console** is the notebook. Both tabs gate their controls on the caller's capabilities
(`functions:manage`, `console:use`), which is UX only — the backend enforces both.

### Functions in a pack

A pack directory may hold function packages beside its `.rrx` files — each is a subdirectory with
its own `runinator-function.json`. `runinatorctl workflows apply` discovers them, publishes them as
part of the same apply, and imports them **before** the workflows, for the same reason secrets go
first: a workflow in the pack may bind to one, and binding validation would reject it against a
catalog that did not yet know the package.

Artifacts are uploaded by digest *first*, and only the ones the server reports missing ride inside
the pack zip. A pack that carried every artifact every time would push megabytes through the 10 MB
request limit to re-send bytes the server already holds — and since the digest is computed
client-side, asking is nearly free. Discovery is one level deep only: a package's own
subdirectories are its code, and recursing would treat a vendored dependency that happened to carry
a manifest as a second package to publish.

### Retention

Deleting a package leaves its artifacts behind, because they are addressed by content and a
*different* package may have published the same bytes.
`repository::functions::sweep_unreferenced_artifacts` removes the ones no version references and
that are older than a 24-hour grace window. The grace period is deliberate: republishing identical
bytes reuses the artifact, so something unreferenced right now may be referenced again minutes later
by a re-apply — deleting immediately would turn a free no-op upload into a real one, and would race
a publish that has stored its bytes but not yet written its version row.

### How a function executes

A worker that picks up a packaged-function action stages the code before running it: it downloads
the artifact by digest, **re-verifies the sha-256 against the bytes that arrived**, and unpacks it
into a digest-keyed cache under the worker's app-data directory. The unpack refuses any archive
entry that would write outside the package directory. The provider is then handed a local path and
makes no control-plane calls of its own.

Execution goes through `runinator-sandbox`, which runs the container under a bounded envelope: no
network, a read-only root with a small tmpfs, `--cap-drop=ALL`, `no-new-privileges`, an unprivileged
uid, and caps on memory, CPU, and process count. The manifest's `limits` supply the numbers; it
cannot opt out of the parts that are not its decision. The deadline is the smaller of the export's
declared timeout and the node's, enforced by the host — a payload that ignores its own timeout does
not get to run on — and both output streams are captured with a size cap, streaming to the run's
logs as they arrive.

Packaged code is ordinary code: it imports no Runinator library. A small shim inside the container
reads the input, resolves the declared handler, calls it, and writes the result. `python` and `node`
are supported; `runtime` names a family and version (`python3.13`), which resolves to an image an
operator can repoint with `RUNINATOR_FUNCTION_IMAGE_PYTHON` without republishing anything.

Running a function needs a container runtime, which the Kubernetes worker pods do not have — they
report `FUNC008` rather than failing obscurely. Use a host worker or the desktop agent.

## The REXRAP Console

A notebook of cells sharing one scope, for working out what a workflow should say.

```
POST /console/sessions                 # start a notebook
POST /console/sessions/{id}/cells      # append a cell
POST /console/cells/{id}/run           # run it
```

A cell is a fragment of the same REXRAP a workflow is written in, and it is answered one of two ways.
A **pure** cell — an expression or a `compute` block — is evaluated in process and has already
settled when the request returns. **Anything else** becomes a hidden scratch workflow and goes
through the ordinary graph-runtime path. Classification is conservative and the workflow fallback is
unconditional: a cell wrongly treated as pure would run a provider action inside an HTTP handler,
with no run to record it and no retry, timeout, or cancellation.

Cells share a scope. A cell's result binds under its label (or `cell_<n>` if unlabelled) and a later
cell reads it as `params.<name>`. `params` rather than a console-only root because a bare dotted
path in REXRAP already means *node output* — `cells.load` would be a reference to a node called
`cells`. It is still a namespace, so a cell labelled `config` binds to `params.config` and cannot
shadow the real `config` root. The scope is also what a scratch run receives as its parameters, so a
name means one thing however the cell ran.

The scope lives in the database, not in a replica's memory: a session outlives any one request, and
an in-process scope would give different answers depending on which replica served the cell.

Editing a cell clears its previous result, and a failing cell drops its binding — in both cases
because a stale value shown as a current one is worse than an absent one.

### The two consoles

The console has two front ends, and they are deliberately the same thing: a scrollback of what has
been run, a prompt at the bottom, and the session's scope beside it.

`runinatorctl console` opens it in the terminal, as a full-screen ratatui interface: a status line
naming the session and the service, a scrollable pane holding everything commands have printed, the
input, a completion menu, and a key legend.

The output pane is the console's own scrollback, not the terminal's. `PgUp`/`PgDn` page through it,
`Shift+↑`/`Shift+↓` move a line, `Shift+Home`/`Shift+End` jump to the oldest line or back to
following, and `Shift+←`/`Shift+→` scroll sideways for output wider than the pane (tables are
truncated rather than wrapped, the way `less -S` does it). The wheel scrolls whichever pane the
pointer is over — the output, or the input when a multi-line cell is taller than the four rows it
gets. `↑`/`↓` remain history recall, and typing anything puts the input pane back under the caret.

A pane that has been scrolled back stays where it was put while a command keeps printing, and says
so in its header rather than looking live. Output arriving during a long run reaches the pane as it
happens, so a run can be read while it is still going, and `Ctrl+C` interrupts the wait without
leaving the console. Quitting replays the session's output to the terminal, so the shell's own
scrollback ends up holding what it would have held anyway.

`--plain` falls back to the single-line reedline prompt, which is also what a pipe gets
automatically. The Console tab in the command center is the same layout in the browser.

The console runs on Linux, macOS, and Windows. Taking stdout away from the command modules is the
one per-platform part — `dup2` on a descriptor, `SetStdHandle` on a std handle — and crossterm is
what makes the Windows half work: it reaches the terminal through `CONOUT$` and `CONIN$`, opened by
name, so the size query, raw mode, the alternate screen, and the event source cannot see the
redirection at all. The console also puts the Windows console into UTF-8 for the duration and puts
the code page back on the way out, since it draws through a handle that carries bytes rather than
through Rust's own `Stdout`.

In both, a **bare line is REXRAP** and becomes a durable cell; a **`:` line is a command**. Every
`runinatorctl` command works with a `:` in front of it — `:runs list --open`, `:settings get aws
key`, `:agents drain <replica>` — because the terminal console hands the line to the same clap
parser the process uses rather than keeping a second table of verbs. The web console implements the
same vocabulary over the HTTP API; the handful of commands that read or write a working tree
(`workflows apply`, `functions publish`, `settings import`, `artifacts download`) stay listed in
`:help` and say to run them with `runinatorctl`.

`:help` prints one table of every command against what it does, and `:help <command>` narrows to a
prefix or expands one command into its call shape and each argument. The list is *derived* — the
terminal console walks the same clap tree the process parses with, so a verb added to the CLI is
listed, completed, and explained the day it is added.

Both consoles read a line the same way: tokenize, take the longest command path that prefixes the
tokens, then split what is left into positionals and flags (`--name value`, `--name=value`, and a
bare switch all work). A flag the command does not take is an error naming the ones it does, and an
unknown verb suggests the nearest one rather than only refusing.

`Tab` completes verbs, subcommands, long flags, and the **values** a flag accepts when they are a
closed set (`:replicas list --status ` offers `live`, `stale`, `offline`). When there is nothing to
insert, the band under the prompt says what belongs there instead — `<workflow>`, `--kind <KIND>`.
`Enter` runs a finished line and opens a new one while a brace, bracket, paren, or quote is still
open, so a multi-line workflow can be typed straight into the prompt and executed as a scratch run.

A few verbs exist only inside a session, since they have no command-line counterpart: `:sessions`,
`:new`, `:use`, `:history`, `:bindings`, `:cancel`, `:replay`, `:run workflow|pipeline`, `:invoke`,
and `:clear`. `:run` and `:invoke` take their payload either way — `--param KEY=VALUE` or a
`… with {"a": 1}` tail — so a line copied between the two consoles keeps working.

### The MCP server

`runinatorctl mcp` is the same control surface again, for a model rather than a person: a Model
Context Protocol server speaking json-rpc on stdin and stdout. It is meant to be launched by an MCP
client, not run by hand.

```jsonc
// claude_desktop_config.json, .mcp.json, or whatever your client reads
{
  "mcpServers": {
    "runinator": {
      "command": "runinatorctl",
      "args": ["mcp", "--api-base-url", "http://127.0.0.1:8080"],
      "env": { "RUNINATOR_API_KEY": "…" }
    }
  }
}
```

**Every `runinatorctl` command is a tool**, named `runinator_<command>_<subcommand>` —
`runinator_workflows_apply`, `runinator_runs_show`, `runinator_settings_set`, and eighty-odd more.
Each one's description, arguments, types, closed sets, and defaults are read out of the same clap
tree the process parses its own argv with, so a verb added to the CLI is a tool with a correct
schema the day it is added; nothing is written down twice. A call is turned back into argv and run
through the ordinary parser and dispatch, so there is one execution path, not two.

Two tools sit in front of that set. `runinator_help` is the index — the `:help` table, for finding a
verb without pulling ninety schemas into the conversation. `runinator_exec` runs a raw command line,
which is the escape hatch for a longer timeout, for output as a table rather than json, or for
anything a schema does not express.

Runs, their logs, and their artifacts are also readable as **resources** (`runinator://runs/{id}`,
`runinator://node_runs/{id}/chunks`, …), so a client can attach what a run left behind to the
conversation without spending a tool call on it. `--workflow-tools` additionally exposes every
enabled workflow as a tool that starts a run of it, typed by the workflow's own declared input; it
is off by default, because a fleet of workflows would bury the commands that author them.

The verbs that never return or read the terminal are refused rather than left to hang the call —
`console`, `mcp`, `login`/`logout`, `workflows dev`, `runs watch` — each naming what to do instead.
Two commands that need no server (`workflows test`, `functions validate`) run offline, which is when
a dry run is most useful. The server also starts against a web service that is not up yet: a client
launches it before the stack is running, so an unreachable service becomes an error on the first
tool call rather than a process that exits at startup.

Command output is captured underneath the command modules, the way the terminal console captures it,
because they print with plain `println!` and a table written into the middle of a json-rpc frame
would desynchronise the client. Moving a standard stream is the one per-platform part —
`dup2` on a descriptor, `SetStdHandle` on a console handle — and it is the whole of
`capture/unix.rs` and `capture/windows.rs`; everything above that line is shared. The server runs on
Linux, macOS, and Windows alike.

When the web service lives in kubernetes rather than on localhost, `scripts/start-runinatorctl.sh
--mcp` is the launcher to point the client at: it brings up the same port-forward the console uses,
signs in if the service enforces auth, and then runs `runinatorctl mcp` against it, tearing the
forward down when the client disconnects. Every message the script prints moves to stderr under
`--mcp`, since stdout carries the protocol. Arguments after the script's own flags go to the
subcommand, so `scripts/start-runinatorctl.sh --mcp --workflow-tools` works as expected.

## Workflow Import

`runinatorctl workflows apply <path>` imports a workflow pack in one shot. The
path can be a `.rrx` source file or directory (each source is a unified REXRAP
container with workflow, pipeline, settings, package-manifest, and test blocks),
or a workflow/bundle JSON file. The local supervisor config applies
`./packs/sdlc/sdlc.rrx`. To load local credentials and config, import an `.rrx`
source containing a `settings` block with `runinatorctl settings import <file>`. Each entry carries a `kind`
(`secret` — the default — or `config`) and a `value`; secret values stay
encrypted and resolve late at the worker, while config values are arbitrary JSON
read by the web service. You can seed the app-data workflow pack from the
repository sample if needed:

```bash
mkdir -p ~/.runinator/workflows
cp -R packs/sdlc ~/.runinator/workflows/sdlc
```

Compiled JSON workflow packs are no longer checked in. Use the unified `sdlc.rrx`
source and its referenced `.rrx` sources for imports.

Because `workflows apply` overwrites stored definitions wholesale, every accepted
definition is also captured as an immutable revision. `runinatorctl workflows
revisions <workflow>` lists that history (revision number, version, source —
`ui`/`pack`/`api`/`duplicate`/`rollback` — author, and timestamp), `workflows
revision <workflow> <n>` prints one revision with the definition it captured, and
`workflows rollback <workflow> <n>` restores it. A rollback re-validates the old
definition against the current provider catalog and saves it as a *new* revision,
so nothing is overwritten and the rollback itself stays in the history. The same
history is available per workflow in the command center's Workflow Settings
dialog, with a diff between any two revisions. An unchanged re-apply records
nothing, so a pack imported on a schedule does not bury real edits.

`runinatorctl workflows dev <path>` runs the same client-side pack compile and
compiled zip upload in a watch loop. It watches the selected `.rrx` source or
directory, adjacent sources, and an optional `--json-file`. When `--run` is
provided, it starts that workflow after each successful import and refreshes the
run detail until the run reaches a terminal state.

`runinatorctl workflows test <path>` dry-runs a pack against `tests { ... }`
blocks in `.rrx` sources
entirely client-side — no server or broker. It compiles the pack, then walks each
workflow's state machine with the graph runtime's own condition/switch/toggle/percentage
evaluators, stubbing task nodes with mocked outputs, and asserts on the branch
taken and final outputs. Test cases live in an RRX `tests` block and provide a name,
input, config, mocked task outputs, and assertions on status, reached nodes, branches,
and final output. Every `.rrx` source in the pack is considered automatically, or pass
additional sources with `--tests`. The command
exits non-zero when any case fails, so it drops straight into CI.

The same walker also backs a server-side dry-run: `POST /workflows/simulate`
(`WorkflowSimulateRequest`: `{ workflow, inputs?, replay_run? }`) walks a workflow
against live config — publishing no actions — and returns the routed path, per-node
status, branch targets, and final output. Pass `replay_run` to replay a prior run's
recorded node outputs so the walk follows the branches that run actually took. A
saved workflow requires `Run` permission; an unsaved draft only an authenticated
caller; `replay_run` is additionally gated on that run's workflow. The command
center surfaces it as a **Dry run** button in the workflow editor toolbar.

For the checked-in local stack only, `bash scripts/run-local.sh sync`, `dev`,
and `smoke-sync` will default `RUNINATOR_API_KEY` to the same seeded dev
service key when talking to `http://127.0.0.1:8080/` and no explicit
`RUNINATOR_API_KEY` is already set. Pointing those helpers at another stack
still requires that stack's own credentials.

For a minimal smoke import, use `./packs/hello-world/hello-world.rrx`. It
contains one REXRAP workflow that runs a single built-in console action and is wired
into `bash scripts/run-local.sh smoke-sync` for an import-and-run check against
an already running local stack.

### Editor integration (language server)

`runinator-lsp` is an editor-agnostic Language Server for `.rrx` files: live diagnostics,
provider/action completion (from live service metadata), hover, formatting, and an optional
apply-on-save that imports the pack into a running web service — the editor-native counterpart of
`runinatorctl workflows dev`. Build it with `cargo build -p runinator-lsp --release` and point your
editor at the binary. See [`runinator-lsp/README.md`](runinator-lsp/README.md) for the VS Code
extension and Neovim/Zed setup. The pack compile-to-bundle logic shared by the CLI and the server
lives in the `runinator-pack` crate.

Workflow syntax now includes richer declarative control-flow nodes:

- `switch` routes by ordered cases and an optional default target.
- `parallel` starts branch roots, with branch nodes returning to a `join`.
- `join` waits for named upstream nodes using `all`, `any`, or `first_success`.
- `try` runs a body, optional catch, and optional finally node; those nodes transition back to the `try` controller.
- `map` runs one target node for each resolved item and exposes the current item under `workflow.state.map`.
- `race` starts branch roots until one satisfies the winner policy, then cancels the still-running losing continuations and effects.
- `emit` records structured node output without calling a provider.
- `reentry` allows explicit bounded cycles back to a node and can route to `on_exhausted`.

#### Retry and compensation

An action node carries two failure-handling policies, both editable in the step editor and both
previewed there so the effect is visible before saving:

- **Retry** (`@retry(attempts, backoff: 2s, max: 60s, jitter: true, on: failure)`) re-runs the node
  itself. The delay before attempt *n+1* is `clamp(backoff * 2^(n-1), backoff, max)`, optionally
  jittered into the lower half to spread a retry storm. `on` narrows which terminals are eligible
  (`any`, `failure`, `timeout`) so a long expensive action is not blindly re-run on a timeout. The
  editor renders the resulting schedule, since exponential backoff under a cap is hard to eyeball.
- **Compensation** (`compensate provider.fn(args)`) is a saga rollback. Once the node has succeeded,
  a run that later reaches `fail` calls the compensating action; compensations unwind in reverse
  order and are best-effort, so one that fails does not stop the unwind. The clause is the same call
  form as the forward action and carries its own attributes.

```rexrap
@retry(3, backoff: 5s, on: failure)
let deploy = k8s.apply(manifest: params.manifest)
    compensate @timeout(300s) k8s.rollback(release: deploy.release)
```

#### Triggers and workflow chaining

Workflows declare triggers in the REXRAP header, materialized from `metadata.triggers`
on import (pack-managed, `metadata.managed_by = "rexrap"`):

- `trigger cron "<expr>"` schedules the workflow.
- `trigger on_success | on_failure | on_complete workflow "<name>"` chains another
  workflow: when this run reaches that terminal state, the named target starts a new
  run. `on_failure` matches `Failed`/`TimedOut` (not a manual `Cancel`).

```rexrap
trigger cron "0 * * * *"
trigger on_success workflow "Downstream Report"
```

Chaining is event-driven from the graph runtime's terminal settle (not the best-effort
`events` channel), fired exactly once per (trigger, source-run) via a durable
dedupe table, and cycle-bounded by a `chain_depth` cap. Only top-level runs fan out
chains — subflow/map children do not. Chaining does **not** replace `subflow`: a
subflow is a synchronous child with a return path, while a chain starts an
independent downstream run.

The command center's top-level **Pipelines** tab visualizes chains as a DAG — one
node per workflow, one edge per chained trigger — and lets you author them by
dragging between workflows, editing an edge's `on` selector, or enabling/disabling
and deleting chains through the normal trigger CRUD.

#### Failure alerting

Workflows declare alerting policies in the REXRAP header, materialized from
`metadata.notifications` on import the same pack-managed way triggers are:

- `notify on failure -> <channel> "<target>"` fires when a run ends `Failed`/`TimedOut`.
- `notify on retry_exhausted -> ...` fires for the node that used up its retry budget.
- `notify on sla -> ... after <duration>` fires when a run stays open past the threshold.
- `notify on parked -> ... after <duration>` fires when a run sits waiting/approval/input/blocked
  past the threshold.

`<channel>` is `slack`, `email`, or `app` (in-app only). `severity info|warning|critical`
defaults to `warning`; `with { ... }` overrides the generated provider configuration
(notably the credential reference); `disabled` imports the policy switched off. The two
duration events require `after`, and the compiler rejects them without it — a policy the
scanner could never match is a silent failure during an incident.

```rexrap
notify on failure -> slack "#oncall" severity critical
notify on sla -> email "ops@example.com" after 30m
```

Policies can equally be managed from the command center's **Notifications** tab
(capability `notifications:manage`); pack-managed rows are read-only there because an
import reconciles them. Emission happens in `runinator-engine` at the terminal
transition, plus a periodic scanner for the duration events. The engine never speaks a
vendor protocol: an in-app policy writes the notifications row, and every other channel
is enqueued on the normal action outbox so a worker delivers it through the
`runinator-provider-slack` / `-email` provider like any other action. Delivery attempts
are tracked per notification and readable at `GET /notifications/{id}/deliveries`.

#### Schedule policy: concurrency, catch-up, and freeze windows

A cron trigger fires without asking whether the last run finished. Two REXRAP header
clauses change that, and both are evaluated at the claim point — the loop declines to
create the run rather than creating one that immediately parks:

```rexrap
trigger cron "0 * * * *" catchup fire_all max 10
concurrency 1 on_conflict queue
```

`concurrency <n> on_conflict <policy>` caps overlapping runs of the workflow. The
policies:

- `skip` (the default when the clause is omitted) — drop the slot, record the firing so
  it is never retried, and move the schedule on.
- `queue` — leave the slot due and re-evaluate next tick, so it fires as soon as capacity
  frees up. Nothing is created while blocked: the schedule itself is the queue, so a
  backed-up workflow costs no runs and no wake-queue entries.
- `cancel_previous` — cancel the workflow's in-flight runs, then start this one.
- `allow` — the historical behavior, and what an absent header means.

`catchup <policy>` decides what happens to slots that came due while nothing was firing
them (engine downtime, a freeze window, or a `queue` policy holding the schedule back):

- `fire_once` (default) — collapse the whole backlog into one run and re-anchor.
- `fire_all [max <n>]` — replay each missed slot as its own run, at most `max` per pass
  (25 by default) so a week of downtime cannot flood the run table in one tick.
- `skip [grace <duration>]` — abandon slots more than `grace` late (60s by default) and
  re-anchor. The grace matters: every firing is slightly late, so without it `skip` would
  drop every run.

The compiler rejects `grace` on a non-`skip` policy and `max` on a non-`fire_all` one
rather than storing a knob the runtime never reads.

**Disabling a workflow** stops every path that starts it automatically, not just its own
schedule: its cron triggers drop out of the due claim, and a chained trigger pointing at
it declines to start it. Both leave the link and the due slot in place, so re-enabling
resumes on the same terms a freeze window does. A trigger keeps its own `enabled` flag on
top of this — turning off one schedule is not the same as turning off the workflow. Only
an explicit run (`runinatorctl triggers run`, the Run button, a backfill) still starts a
disabled workflow, because that is somebody asking for it by hand.

**Freeze windows** suspend firing over a date range, independently of any pack. A window
scopes to one workflow, one org, or the whole platform (and platform/org windows also
hold pipeline cron triggers). Frozen triggers are excluded from the claim in SQL, and
their due slot is deliberately left in place — when the window lifts, the trigger's
catch-up policy decides whether the missed slots replay. Manage them under the command
center's **Schedules** tab or from the CLI, both gated on the `schedules:manage`
capability:

```bash
runinatorctl freeze create "December freeze" --from 2026-12-20T00:00:00Z --to 2027-01-02T00:00:00Z
runinatorctl freeze list --active
```

**Backfill** replays a cron trigger's slots across a past range. Slots the loop already
fired keep their original run — the firing row is the same uniqueness gate the loop
claims through — so an overlapping range is safe to re-issue:

```bash
runinatorctl triggers backfill <trigger-id> --from 2026-08-01T00:00:00Z --dry-run
```

REXRAP references resolve runtime values into action arguments. Alongside `params.*`,
`prev.*`, `run.*`, and bare node-output names, two roots read from the unified
settings store:

- `config.<scope>.<name>` — non-sensitive JSON, resolved eagerly by the web
  service. It interpolates freely (e.g. `"${config.api.base}/v2"`) and can drill
  into stored JSON (`config.api.settings.url`).
- `secret.<scope>.<name>` — sensitive values, lowered to the `secret://scope/name`
  form and resolved late at the worker so plaintext never reaches the web
  service, database, or broker. A secret must be passed as a whole argument value
  (it cannot be interpolated mid-string).

Stored settings are typed. Config values are validated on write against a declared
JSON-schema (required once per `scope/name`, then reused for value-only updates);
a value that does not match the schema is rejected. Secrets are validated as
non-empty strings. Manage them with `runinatorctl`:

```bash
# declare + store a config value (schema required on first write)
runinatorctl settings set api base '"https://api.example.com"' \
  --kind config --schema '{ "type": "string" }'
# later value-only update reuses the stored schema
runinatorctl settings set api base '"https://api.example.com/v2"' --kind config

# store a secret (string)
runinatorctl settings set github token "ghp_xxx"

# read a value from a file instead of passing it inline
runinatorctl settings set github deploy-key --value-file ./id_ed25519

# bulk import secrets and config from a bundle file
runinatorctl settings import ./secrets.json

runinatorctl settings list            # all settings, no values
runinatorctl settings get api base --kind config
```

The import file is a `{ "secrets": [...] }` document; each entry carries
`scope`, `name`, and `value`, plus optional `kind` (`secret` or `config`) and
`schema`. Existing entries are only overwritten when an incoming `updated_at` is
strictly newer.

The control-flow runtime uses persisted cursors: linear runs retain one primary cursor, while
`parallel` and `race` fork one cursor per branch. The engine claims and drives runnable
continuations with bounded concurrency (default 16 per instance), so independent branches and
concurrent map children can reach workers without waiting for a serial interpreter loop.
`active_node_id` remains a compatibility mirror of the primary cursor for run detail and older consumers. Branch/body/item
nodes should transition back to their owning `join`, `try`, `map`, or `race` controller.

## Observability

Every service binary (`ws`, `engine-worker`, `worker`, `waker`) emits structured logs to stdout and
a log file via `tracing`, filtered by `RUNINATOR_LOG` (an `EnvFilter` directive,
default `info`). The web service additionally exposes Prometheus metrics at
`/metrics`.

The `runinator-desktop-agent` tray app honors the same `RUNINATOR_LOG` directive at
startup and additionally renders those `tracing` records into its in-app log console,
where a **Log → Level** dropdown changes the level live (no restart) and persists it.

OpenTelemetry export is **off by default and turns on purely from the standard
`OTEL_*` environment variables** — no CLI flags or config-file options. When
`OTEL_EXPORTER_OTLP_ENDPOINT` (or a signal-specific
`OTEL_EXPORTER_OTLP_{TRACES,METRICS,LOGS}_ENDPOINT`) is set, each binary stands up
OTLP exporters for **traces, metrics, and logs** over OTLP HTTP/protobuf;
`OTEL_SDK_DISABLED=true` forces it off. The service name defaults to the binary
(e.g. `Runinator Web Service`) and is overridable with `OTEL_SERVICE_NAME` /
`OTEL_RESOURCE_ATTRIBUTES`.

Trace context propagates across hops using W3C `traceparent`: inbound HTTP requests
to the web service continue the caller's trace, and the graph runtime stamps the active
context onto each `ActionCommand` so a worker's execution span links back to the
dispatching trace. Prometheus `/metrics` remains available alongside OTLP metrics.

Each service and the broker emit runtime metrics over OTLP (and, for the web
service, also on Prometheus `/metrics`):

- **Engine** (`runinator_engine_*`, emitted by `ws` when embedded or by `engine-worker`):
  effect/result processing, trigger, maintenance, and VM-drive metrics. The VM's
  `runinator_vm_drive_duration_ms` histogram measures continuation claim-and-advance batches.
- **Worker** (`runinator_worker_*`): `actions_received_total`, `actions_completed_total`
  and the `action_duration_ms` histogram (both split by `outcome`),
  `actions_duplicate_total`, `actions_in_flight` (gauge), `control_commands_total`
  (by `kind`), and `secret_resolution_failures_total`.
- **Waker** (`runinator_waker_*`): `wakes_{received,driven,requeued}_total`,
  `drive_failures_total`, and the `wake_lead_ms` histogram (scheduling lead/lag at
  receipt).
- **VM** (`runinator_vm_*`): `continuations_driven_total` split by the bounded
  `outcome` label (`yielded`, `forked`, `joined`, `completed`, `failed`), the
  `drive_duration_ms` histogram for claim-and-advance batches, and
  `driver_failures_total`.
- **Broker** (`runinator_broker_*`, emitted by every service): `operations_total` and
  the `operation_duration_ms` histogram, tagged with `backend` (in-memory/http/tcp/
  kafka/rabbitmq), `channel`, `op`, and (for the counter) `outcome`.

```bash
# point all binaries at a local OpenTelemetry Collector (OTLP/HTTP on :4318)
export OTEL_EXPORTER_OTLP_ENDPOINT=http://localhost:4318
cargo run -p runinator-supervisor -- start
```

For the checked-in local supervisor flow, prefer the one-command helper:

```bash
bash scripts/run-local.sh observe
```

It starts `deploy/local-observability/compose.yaml` with:

- OpenTelemetry Collector receiving OTLP HTTP on `http://127.0.0.1:4318`
  and OTLP gRPC on `127.0.0.1:4317`.
- Jaeger at `http://127.0.0.1:16686` for traces.
- Prometheus at `http://127.0.0.1:9090` scraping the collector's re-exported
  OTLP metrics on `otel-collector:8889` plus collector self-metrics on `:8888`.
- Loki at `http://127.0.0.1:3100` receiving the collector's logs signal via
  OTLP, so structured fields the binaries set (`trace_id`, `run_id`,
  `error_code`, ...) are queryable with LogQL instead of only living in
  stdout/log files.
- Grafana at `http://127.0.0.1:3000` (anonymous admin) with Loki, Prometheus,
  and Jaeger pre-provisioned as datasources — the natural place to query logs
  and click a `trace_id` through to its Jaeger trace.

After the stack starts, run `bash scripts/run-local.sh smoke-sync` or drive a
workflow through the UI/CLI, then inspect traces in Jaeger, metrics in
Prometheus, and logs in Grafana (or `loki`, e.g. via `logcli`). Use
`bash scripts/run-local.sh observability-logs --lines 120` to inspect
collector/exporter output, and use `bash scripts/run-local.sh
observability-stop` to stop the local observability containers.

**In Kubernetes**, the `components/observability` kustomize component deploys an
OpenTelemetry Collector, Jaeger (trace UI), Prometheus (scrapes the collector),
Loki (durable/queryable log store), and Grafana (dashboards over Prometheus +
Jaeger + Loki), and points the services at the collector. It is enabled in the
`local` overlay by default; add `../../components/observability` to another
overlay's `components:` list to turn it on there (and remove it to turn otel
back off). After deploying:

```bash
# dashboards + logs — open Grafana at http://localhost:3000 (anonymous admin; "Runinator
# Overview" dashboard is provisioned, with Loki + Prometheus + Jaeger datasources wired up)
bash scripts/port-forward-grafana.sh   # or: kubectl -n runinator port-forward svc/runinator-grafana 3000:3000
# traces — open the Jaeger UI at http://localhost:16686
kubectl -n runinator port-forward svc/runinator-jaeger 16686:16686
# raw metrics — the Prometheus UI / API at http://localhost:9090
kubectl -n runinator port-forward svc/runinator-prometheus 9090:9090
# raw logql — the Loki API at http://localhost:3100
kubectl -n runinator port-forward svc/runinator-loki 3100:3100
# a copy of every signal — the collector's debug exporter
kubectl -n runinator logs deploy/runinator-otel-collector
```

Grafana's anonymous-admin login is for convenient local viewing; lock it down (set
a real admin password and disable anonymous access) before using it on a shared
cluster.

### Dead letters and audit log

Poison messages are no longer dropped silently. When a result or ingress event
cannot be applied and is given up on, the engine persists a `dead_letters`
row before acking, so failed messages have a durable record. Auth and sensitive
operations (login success/failure, authorization denials) are recorded to an
`audit_log` table. Both are exposed as admin-only endpoints (`GET /dead_letters`,
`GET /audit_log`, in the OpenAPI spec) and surfaced in the command center as
admin-gated **Dead Letters** and **Audit Log** views.

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

## Versioning

Runinator uses one `major.minor.build` version across the Cargo workspace, command center, VS Code
extension, packaged backend apps, release archives, and container images. Major and minor are
release decisions and are set in the root `Cargo.toml`; the build is the full-history Git commit
count. Release CI runs `node scripts/set-workspace-version.mjs` after a full checkout to resolve the
build number and synchronize the non-Cargo manifests. Container builds receive the same version as
an OCI label, and the default Kubernetes tag is `<version>-kube-<timestamp>`.

To prepare a manual major or minor bump, edit the root workspace version and run:

```bash
node scripts/set-workspace-version.mjs
cargo metadata --no-deps --format-version 1 >/dev/null
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
It is a pure client and does not execute workflow actions itself; use the
desktop agent below to run actions on your own machine.

## Desktop Agent

`runinator-desktop-agent` is a standalone tray application that runs the shared
`runinator-worker` action loop on an operator machine. It publishes the built-in
provider catalog plus the sandboxed local-files provider, and registers as an
exclusive `desktop`-pool replica. It therefore runs only work explicitly pinned
to that replica or targeted to one of its labels; it never picks up unlabeled
general-pool workloads. The Tauri command center is a separate API client and
does not start, stop, embed, or communicate directly with this worker runtime.

```bash
cargo run -p runinator-desktop-agent
```

For an unattended machine, run the same binary without a desktop session:

```bash
cargo run -p runinator-desktop-agent -- \
  --headless \
  --service-url https://runinator.example/ \
  --api-key "$RUNINATOR_API_KEY" \
  --sandbox-root /srv/runinator-agent \
  --labels zone=home \
  --liveness-file /tmp/runinator-desktop-agent-liveness
```

Command-line values take precedence over environment variables, which take precedence over the
saved GUI configuration. The corresponding environment variables are `RUNINATOR_SERVICE_URL`,
`RUNINATOR_API_KEY`, `RUNINATOR_WORKER_LABELS`, `RUNINATOR_BROKER_MODE`,
`RUNINATOR_MAX_CONCURRENT_ACTIONS`, `RUNINATOR_SHUTDOWN_GRACE_SECONDS`,
`RUNINATOR_RECONNECT_MAX_ATTEMPTS`, and `RUNINATOR_LIVENESS_FILE`.

If the web service or broker becomes unreachable, the agent retries with backoff and shows an
amber **reconnecting** dot (in the window header and the tray icon) carrying the attempt number.
After `--reconnect-max-attempts` consecutive failures — 10 by default, roughly seven minutes of
capped backoff — it gives up: the dot turns red **disconnected**, a desktop notification fires, the
replica is marked offline, and the agent stops rather than heartbeating a worker that can never take
an action. Pressing **Start agent** (or restarting the process) tries again. The count is
consecutive, so a connection that stays up clears it; `--reconnect-max-attempts 0` restores retrying
forever. `runinator-worker` takes the same flag but defaults to `0`, since an in-cluster pod's
orchestrator is what decides whether to restart it.

While the agent is coming up — enrolling, registering, or parked waiting for re-enrollment — the
window offers **Cancel startup** in place of **Start agent**. It aborts the attempt wherever it is
waiting, stops anything it already brought up, and returns to the configuration form, so a start
pointed at an unreachable service does not have to be ended by killing the process. "Exit" in the
tray cancels a startup the same way it stops a running agent.

For LAN/local development, `--discover --enroll <token>` (or `RUNINATOR_DISCOVER=true`) listens
for web-service gossip and selects only an announcement whose `cluster_id` matches the identity
bound into that enrollment token. Discovery merely finds an address; the token authorizes the
cluster. Without a bound token, candidates must be chosen explicitly with `--service-url` and are
never auto-enrolled. Gossip is IPv4 UDP broadcast and normally stays within one subnet; it is not
a Kubernetes discovery mechanism. Kubernetes keeps `--disable-gossip` and uses stable service DNS.

Web-service announcements include their `http`/`https` scheme, relay path, version, enrollment
availability, and optional SPKI pin. Set `RUNINATOR_CLUSTER_ID` to the same stable UUID on every web
replica when its public enrollment URL differs from the address advertised on the LAN.

`runinatorctl replicas` reads the fleet: `list` (with `--kind`, `--status`, or `--live`), `ids` for
just the identifiers one per line, `show <id>` for one replica and the attributes it heartbeats,
`providers <id>` for what a worker advertises, and `samples <id>` for its recent cpu/memory
telemetry. The same verbs are in both consoles as `:replicas …`. They are read-only on purpose: a
replica row is a report *from* a runtime, so the way to change one is to scale a node group
(`nodes`), direct an agent (`agents`), or stop the process itself.

The service reaps silent replicas after 10 minutes and deletes offline rows after 60 minutes by
default. Set `RUNINATOR_REPLICA_REAP_SECONDS` and `RUNINATOR_REPLICA_DELETE_SECONDS` to tune those
retention windows. Remote agents advertise a separate live/stale window in their heartbeat status,
so normal home-network jitter does not make a connected agent flicker stale.

The control window lets you set the service URL, sandbox folder, optional direct
broker connection, routing labels, and startup behavior. Closing the window
hides it in the tray; use "Exit" from the tray menu to actually quit.

By default the agent relays broker traffic through the web service's
`/ws/desktop-worker` endpoint rather than dialing the broker directly, so a
machine behind NAT needs only outbound access to the web service — no inbound
ports and no route to RabbitMQ. The relay URL is derived from the service URL
(`https://` becomes `wss://`), so pointing the agent at a TLS ingress works as-is.
Use direct broker mode only for a machine actually on the broker's network.

## Package macOS Runtime Apps

The Rust services and desktop agent remain normal binaries. On macOS, you can
also package them as `.app` bundles with the Runinator icon:

```bash
cargo install cargo-packager --version 0.11.8 --locked
scripts/package-macos-backend-apps.sh --release
```

The script creates `.app` bundles for broker, web service, waker, headless
worker, desktop agent, the control CLI (`runinatorctl`), and supervisor under
`target/macos-apps`.

## Verification

For workflow pack import changes, run:

```bash
runinatorctl rexrap check packs/sdlc/sdlc.rrx
runinatorctl rexrap check packs/hello-world/hello-world.rrx
cargo test -p runinator-ctl
```

To sync the seed file manually against a running local API:

```bash
bash scripts/run-local.sh sync
```

To run the tiny smoke pack against a running local stack:

```bash
bash scripts/run-local.sh smoke-sync
```

To verify rich workflow execution end-to-end against an isolated local stack:

```bash
RUNINATOR_E2E=1 cargo test -p runinator-e2e -- --ignored
```
