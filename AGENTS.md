# AGENTS.md

Guidance for agents working in this repository. Keep changes aligned with the existing architecture before adding new abstractions or cross-crate dependencies.

## Project Shape

Runinator is a Rust workspace for scheduling and executing tasks across a small distributed runtime using a resumable state-machine orchestrator.

Primary runtime flow:

1. `runinator-ws` owns the HTTP API, authentication/authorization, WebSockets, and transport adapters. It embeds `runinator-engine` by default, but does not own reducer or repository implementation code. The surface is spread over six crates — `runinator-ws` itself only assembles them; see "The web service crates" below.
2. `runinator-engine` owns persistence orchestration and the background loops that consume broker ingress/results, fire triggers, publish wakes/actions, and reconcile durable work. It calls the pure state-machine transition logic in `runinator-reducer`. The engine can run inside `runinator-ws` or in the standalone `runinator-background-worker`; do not fork those execution paths.
3. `runinator-waker` is a small, horizontally scalable, broker-only timer/relay: it consumes the `wake` channel, sleeps until each ready node is due, then publishes a drive on the `ingress` channel. It has no database, no HTTP client to the web service, and shares no channel with the worker.
4. `runinator-worker` polls the broker action channel, resolves a provider/plugin, executes the task, and publishes results on the broker result channel (records also reachable through `runinator-api` compatibility endpoints). It self-publishes its built-in provider metadata to the web service on startup.
5. `runinator-desktop-agent` is the standalone, exclusive desktop worker. It reuses `runinator-worker`'s runtime, exposes built-in providers plus the sandboxed local-files provider, and can relay broker traffic through `runinator-ws`. It is a separate tray application, not a Tauri sidecar or command-center service.
6. `runinator-ctl` is the control CLI (`runinatorctl`). Among other commands, `workflows apply` is the one-shot pack importer: it compiles a `.wdl`/`.wdlm`/directory (including any `.wdls` secrets and `.wdlp` pipelines) **client-side**, zips the compiled artifacts (`workflows.json` + optional `secrets.json` + optional `pipelines.json`), and uploads a single `application/zip` to the web service's `/packs/import` endpoint. Compilation never happens on the backend; `/packs/import` only reads the compiled JSON. There is no long-running importer service. Pack zip read/write lives in `runinator-utilities::pack`; the entry-name layout is the wire contract shared by ctl/api/command-center/mcp (writers) and ws (reader). File extensions: `.wdl` workflow, `.wdls` settings, `.wdlm` pack manifest (JSON — lists `workflows`/`pipelines`/`settings`), `.wdlp` pipeline (WDL pipeline grammar), `.wdlt` tests.
7. `runinator-supervisor` runs the local stack from `runinator-supervisor.json`.

There is also a Tauri `runinator-command-center` client. It discovers and calls the web service, compiles/edits packs, and presents runtime state; it never hosts a worker or executes provider actions. Keep frontend UI changes separate from runtime crates unless the change explicitly touches the desktop UI.

### Command center layering (`runinator-command-center`)

Layout:

- `src/core/` — portable domain logic: `domain/`, `api/`, `services/`, `realtime/`, `navigation/`, `workflow/`, `utils/`, `platform/`. Must not import Vue, Pinia, Vue Flow, CodeMirror, Tauri, or `ui/`.
- `src/ui/` — Vue presentation: `views/`, `components/`, `composables/`, `adapters/` (pinia, vue-flow, codemirror, browser, tauri).
- Bootstrap (`src/bootstrap.ts`) selects the platform adapter and registers the CodeMirror text-editor factory before the app mounts.

Import conventions (Phase 5 — shims removed):

- Pinia stores: `ui/adapters/pinia/*`
- Wire models: `core/domain/models`
- Navigation types: `core/navigation/app`
- Pure helpers: `core/utils/*`
- CodeMirror adapters: `ui/adapters/codemirror/*`
- Services (from views/components): `core/services` singletons exported by `core/services/index.ts`

Verification:

```bash
cd runinator-command-center
npm test
npm run build
npm run lint
```

## Crate Boundaries

Keep dependency direction boring and predictable, structured with domains in mind:

- `runinator-models`: shared domain and wire structs only. Avoid service logic, database details, HTTP clients, broker behavior, or runtime configuration here.
- `runinator-comm`: shared communication contracts and gossip/discovery types. It can depend on models, but should not know about concrete services, databases, providers, or broker backends.
- `runinator-api`: HTTP client facade for talking to the web service. Keep URL discovery behind locator types; do not spread raw web-service endpoint construction through worker or ctl code.
- `runinator-store`: the persistence **contract** — trait definitions and the plain types they exchange, with no sqlx and no backend. The surface is split two ways:
  - `roles/` holds one trait per domain (`RunStore`, `ScheduleStore`, `AuthStore`, `OrgStore`, `DefinitionStore`, `DispatchStore`, `NotificationStore`, `ReplicaStore`, `AutomationStore`, `TaskRunStore`, `SettingStore`, `ArchiveStore`). `DatabaseImpl` composes all of them and keeps only `run_init_scripts` of its own.
  - `ReducerStore` is a **use-case** trait, cut to exactly what the state machine calls. It deliberately spans several domains — keeping it small is what makes the reducer's in-memory fake practical.

  Add a new operation to the role that owns it (or `ReducerStore` if the reducer calls it), never to `DatabaseImpl`. Bound as narrowly as the caller allows: `runinator-archiver` bounds on `ArchiveStore`, not `DatabaseImpl`. Because the roles are separate traits, a caller using several must import each — glob `runinator_database::interfaces::prelude::*` when that list would be long and uninformative.
- `runinator-database`: the concrete SQLite/Postgres/MySQL implementation of `runinator-store`'s traits, plus row mapping. Database-specific mapping belongs here, not in `runinator-ws`. Method bodies are written **once**, generically over `SqlBackend`, and implemented on the local `SqlStore<B>` wrapper — the traits are foreign now, so the orphan rule forbids a blanket impl on a bare type parameter. `SqliteDb`/`PostgresDb`/`MySqlDb` are aliases for `SqlStore<…Backend>`, so callers name them as before. It re-exports `runinator_store::{archive, interfaces}` at their historical paths.

  `operations/` mirrors the role split one file per trait, with shared helpers and the SQL-dialect plumbing in `operations/mod.rs`. Each role impl repeats the same thirty-line sqlx `where` block; that is deliberate, not an oversight — a macro would make every type error inside the query bodies point at an expansion instead of a real line. Rust does **not** elaborate trait `where` clauses into implied bounds, so a "bundle the bounds in one trait" shortcut does not compile.
- `runinator-reducer`: state-machine transitions and node-kind orchestration, bounded on `ReducerStore` rather than the whole store. Keep HTTP, concrete broker transports, and service hosting out of it — and keep sqlx out: the crate deliberately does not depend on `runinator-database`, which is what makes `test_support::FakeStore` (behind the `test-support` feature) viable for testing handlers without a database. Prefer adding node-handler tests there over reaching for the web service's sqlite-backed suite.
- `runinator-engine`: durable repository orchestration and background runtime loops shared by the web service and `runinator-background-worker`.
- `runinator-ws`: HTTP/WebSocket server and auth/API adapters over `runinator-engine`. It should not grow duplicate reducer, repository, or worker implementations. It is the assembly crate for the five below; see "The web service crates".
- `runinator-ws-core`: wire payloads (`models`), the json response envelope (`responses`), the ui event bus (`events`), the openapi documentation vocabulary (`openapi::{docs,examples}`), and small json helpers. No routes, no middleware, and no knowledge of any endpoint.
- `runinator-ws-middleware`: the request-gating layers — `auth` (credential resolution and the gating middleware), `authz` (capabilities and resource grants), `rate_limit`, `overload`. It depends on `runinator-ws-core` for the envelope it replies with and registers no routes.
- `runinator-ws-identity`, `runinator-ws-authoring`, `runinator-ws-runtime`: the handler modules, one `src/handlers/<domain>.rs` per domain. Each owns its handler fns, its `routes()` registrations, and its `DOCS` entries, exactly as before the split.
- `runinator-broker-core`: the broker **contract** — the `Broker` trait, the per-channel message/delivery types, `BrokerError`, the channel-capability checks, the otel `instrument` wrapper, and the in-memory backend. It depends on no transport and no external system. A crate that only publishes and receives through a `dyn Broker` (the ws handler crates, `runinator-engine`) depends on **this** crate, not `runinator-broker` — that is what keeps the axum/reqwest/kafka/rabbitmq dependency surface confined to the binaries that actually build a backend.
- `runinator-broker`: the concrete transports and adapters over `runinator-broker-core` — HTTP backend/client/server, tcp, ws, kafka, rabbitmq, and the `factory` that builds one from configuration. It re-exports the core surface at its historical `runinator_broker::…` paths, so a binary that builds a backend needs only this crate. Backend selection stays a feature of the crate that builds the broker (`runinator-ws`, `-worker`, `-waker`, `-background-worker`); do not add a `kafka`/`rabbitmq` forward to a crate with no `cfg(feature)` code of its own. Channels are `action`, `control` (ws→worker), `result` (worker→ws), `wake` (ws→waker), `ingress` (waker/worker→ws), and `events` (ws→every ws replica). All channels except `events` are competing-consumer (one delivery per consumer group); `events` is **fan-out** — every subscriber receives every message (rabbitmq fanout exchange, per-replica kafka group, per-consumer in-memory/wire receiver), so ws replicas can fan UI events to all connected WebSocket clients. The `action` and `control` channels are additionally **target-routed**: commands carry an `ActionTarget` and consumers use `receive_for`/`receive_control_for` with a `ConsumerProfile`, so a pinned action or a cancel stamped with the executor-holding replica reaches only a matching worker (backends without native routing bounce mismatches via nack; a targeted control nobody can match is dropped after `STALE_CONTROL_TTL_SECONDS`). Waker, worker, and web service should talk to the `Broker` trait where practical. A new channel must be implemented across every backend (in-memory/http/tcp/kafka/rabbitmq) and both wire transports.
- `runinator-waker`: broker-only timer/relay. It consumes the `wake` channel, sleeps until due, and publishes a drive on the `ingress` channel. It must not execute task providers, must not write to the database, and must not depend on `runinator-api` or the worker.
- `runinator-worker`: task execution loop and provider resolution. It should not calculate schedules or mutate state except through API calls intended for worker results.
- `runinator-desktop-agent`: standalone GUI host for an exclusive desktop `WorkerRuntime`. Desktop-only configuration and tray UX live here; reusable execution behavior stays in `runinator-worker`. Never add this lifecycle to `runinator-command-center`.
- `runinator-compute`: the expression and compute language — `$ref`/`$template` resolution, the declarative condition form, the compute-program interpreter, the `std` intrinsic library, user-defined function tables, and argument-dependent intrinsic result typing. It knows nothing about workflow graphs: no nodes, no transitions, no definition validation. Depend on **this** crate when you only need to evaluate a value; `runinator-wdl-sema`, `-wdl-ide`, `-wdl-codegen`, and `runinator-provider-std` do exactly that rather than linking the graph layer for it.

  The `WORKFLOW` error dictionary lives here, not in `runinator-workflows`, because both crates emit the same `WorkflowValidationError`. This is the same arrangement as the `WDL` dictionary in `runinator-wdl-syntax` — see "Error Dictionaries".
- `runinator-workflows`: the graph layer over `runinator-compute` — workflow validation, cycle detection, node-kind registry, type checking, and simulation. It re-exports the compute surface at its historical `runinator_workflows::…` paths, so a graph-layer consumer need not name both crates; a consumer that only evaluates values should depend on `runinator-compute` directly instead.

  Per-kind knowledge lives in `node_kinds/`, one `NodeKindSpec` per `WorkflowNodeKind`, in its own
  file under the kind's catalog category (`terminal`/`task`/`control_flow`/`concurrency`/`io`/
  `sync`). A spec owns the kind's palette metadata, its `GraphRole`, the node targets its
  parameters carry (`TargetSlot`), its parameter shape check, and its statically-known output type.
  `catalog.rs`, `parameters.rs`, `validation.rs`, `typing.rs`, and `simulate.rs` read those facts
  from `spec_for(kind)` rather than each keeping a parallel `match`. Adding a node kind is a new
  file, a `mod`/`pub(super) use` pair in the category's `mod.rs`, and one arm in `spec_for`, which
  is exhaustive.

  Two things deliberately stay outside the registry: `typing.rs`'s per-kind type checks need the
  private inference context, and `simulate.rs`'s per-kind evaluation needs the simulator's private
  outcome type and its `&mut dyn SimulationEnv`. Both are single-sited and exhaustively matched, so
  neither can silently disagree with anything; they read the *facts* they used to re-derive
  (`GraphRole`, `NodeKindSpec::output_type`) from the registry. Do not widen those private types to
  move the bodies in — the coupling costs more than the colocation buys.

  A kind's catalog `edge_slots` and its `target_slots` describe the same edges from two angles
  (where the ui writes a target vs. where the graph walkers read one). `node_kinds/tests.rs` pins
  them together in both directions; that is what keeps the palette from advertising an edge the
  runtime ignores.
- `runinator-wdl`: the WDL surface language (grammar, parser, lowering to the JSON workflow model, and decompiling back), plus the `.wdls` secrets front end and the `.wdlp` pipeline front end (`parse_pipeline_str` → `PipelineBundle`, `pipeline_to_wdlp` back). It must round-trip every node kind's parameters, but its grammar must only express well-formed graphs. Do not add WDL syntax for degenerate or malformed graphs (e.g. a parallel with no matching join, a condition with no branches, a missing start node); the decompiler may error on such JSON instead. Keep the grammar a description of valid programs, not a serializer for every possible JSON shape. Header `trigger cron "..."` declarations and input-field defaults are carried in `definition.metadata.triggers` / the field's `default`; the web service materializes pack-managed triggers (`metadata.managed_by = "wdl"`) on import. A `.wdlp` pipeline lowers to a portable `PipelineBundle` (members + links by workflow name); on import the web service resolves names to ids, upserts the `Pipeline`, and materializes each link as a managed `chained` trigger carrying `configuration.pipeline_id` (reconciled by pipeline id; header-trigger reconciliation skips triggers that carry a `pipeline_id`). The pipeline itself never runs — its chained triggers are the runtime linkage.

  The language core is four crates split by compile stage; see "The WDL crates" below. `runinator-wdl` itself is the assembly crate and the only one consumers link.
- `runinator-wdl-syntax`: text ↔ ast. The pest grammar, the ast, comment attachment, the canonical formatter, `file(...)`/include resolution, and `WdlError`/`Span` with the shared `WDL` error dictionary. It depends on no runinator crate but `runinator-models` and knows nothing of diagnostics or the workflow JSON model.
- `runinator-wdl-sema`: ast → diagnostics. Namespace resolution, alias desugaring, the callable registry (intrinsics + user `fn`s), purity classification, named-type resolution, and the four semantic passes. `CompileOptions`/`TypePolicy`/`WorkflowSignature` live here because it is the lowest crate that reads them.
- `runinator-wdl-codegen`: ast ↔ JSON model. `lower` (ast → `WorkflowDefinition`) and `decompile` (`WorkflowDefinition` → text). They share no code but share the round-trip contract, so they share a crate.
- `runinator-wdl-ide`: the editor surface over the language core — completion and hover. It answers "what can go here" and "what is this" for a cursor in a buffer; it never affects what a compiled workflow means. It reads the core through `parse_document`, `ast`, and the `analysis` seam (`runinator-wdl/src/analysis.rs`), which is the whole reason `runinator-wdl-sema`'s `types` and `namespace` modules are public. An editor feature needing a new item from the core gets it added to `analysis` deliberately — do not reach into a core crate to get it. `runinator-lsp`, `runinator-ws`'s `/wdl/complete` and `/wdl/hover` handlers, and the command center's Tauri commands depend on this crate; ctl, the worker, and every compile path depend only on the core.
- `runinator-plugin`: dynamic plugin loading and `Provider` trait integration. Keep FFI details contained here.
- `runinator-provider-*`: provider implementations. Always implement a new library for a new provider. Keep provider-specific configuration and external system behavior out of core crates.
- `runinator-utilities`: small cross-cutting helpers such as startup/logging, credential store trait, and data export. Do not turn this into a dumping ground for domain logic.

If a change requires a dependency from a lower-level/shared crate back into a service crate, stop and redesign the boundary.

### The WDL crates

The language is four crates, layered by compile stage so nothing depends back up:

```
runinator-wdl                    public api, `.wdlp`/`.wdls` front ends,
                                 `analysis` seam, cross-stage test suite
  ├── runinator-wdl-codegen      lower/ (ast -> json), decompile/ (json -> text)
  │     └── runinator-wdl-sema
  ├── runinator-wdl-sema         namespace, desugar, registry, purity, types,
  │                              sema/, options
  │     └── runinator-wdl-syntax
  └── runinator-wdl-syntax       errors, ast, comments, parser, format, includes
```

Pick a crate by compile stage. Syntax must never name sema; sema must never name codegen. If a
pass appears to need the reverse direction, the pass is in the wrong crate.

`runinator-wdl` re-exports `ast`, `comments`, `errors`, `sema`, `CompileOptions`,
`WorkflowSignature`, and the rest at their historical `runinator_wdl::…` paths, so a consumer
should never need to name a core crate directly. Nine crates depend on the facade; only
`runinator-wdl-ide` reads a narrower seam, and it does so through `analysis`.

The **round-trip and format-idempotence assertions live in `runinator-wdl`'s test suite** — it is
the first crate that can see parse, lower, decompile, and format at once, and those contracts are
cross-stage by nature. Do not try to pin them from inside one stage.

The **`WDL` error dictionary is shared, not per-crate**: all four emit the same `WdlError`, so
`DICTIONARY` is defined once in `runinator-wdl-syntax`'s `errors.rs` and re-exported. This is the
one documented exception to the per-crate rule in "Error Dictionaries" below.

## Coding Standards

- Favor guard clauses over deep nesting to keep logic flow flat and readable.
- If a functionality can have different implementations, always use traits to define the interface.
- Favor comments as appropriate for Rust but make them lower case, single line, with a period at the end.
- Use RustDoc comments (`///`) where necessary on public methods, but keep them short, succinct, dense, and dispassionate.
- Do not put all the code for a library in `lib.rs`; break it out into smaller, focused files.
- Do not put tests in the same files as code; break them out into a `tests.rs` file (or a `tests` module in a separate file). Once that file covers several unrelated subjects, make it a `tests/` directory: `tests/mod.rs` holds the imports, the shared fixtures, and the `mod` list, and each submodule owns one subject and opens with `use super::*;` plus a `//!` line saying what it covers. `runinator-wdl`, `runinator-ws`, `runinator-database`, and `runinator-workflows` are laid out this way. A per-module suite that pairs with exactly one source file may instead stay beside it as `<module>_tests.rs`.

## Runtime Contracts

Preserve the command lifecycle:

- Workflows are executed as state-machines with nodes like `task`, `wait`, `condition`, `approval`, `loop`, and `subflow`.
- `runinator-reducer` owns state transitions. `runinator-engine` persists them and publishes `ActionCommand` values through `runinator-broker` for `task` nodes by draining the durable `workflow_action_dispatches` outbox. The engine normally runs in-process with the web service, but may instead run in `runinator-background-worker`. The waker never publishes `ActionCommand`s.
- The engine publishes a `WsIngressCommand::Drive` on the `ingress` channel for every already-due ready node, and a `WakeCommand` on the `wake` channel for future-dated ones (the wake-publisher loop doubles as the durable reconcile backstop; the broker dedupes wakes/drives already in flight). The waker relays a due wake to a `WsIngressCommand::Drive` on the `ingress` channel, which an engine host consumes to run the reducer.
- Workflow run states (`queued`, `running`, `waiting`, etc.) are persisted separately from individual task run statuses.
- Workers acknowledge broker deliveries only after processing and any required result logging has completed.
- Worker outputs, logs, artifacts, and node-run status/results may be delivered as broker result events consumed by `runinator-engine`, or through compatibility endpoints hosted by `runinator-ws`; only the engine/API persistence path writes them through `runinator-database`, and workers must not write directly to the database.
- Broker messages should remain serializable and backend-neutral.
- Any command or control payload that crosses the broker/waker/worker boundary must use the shared contracts in `runinator-comm` end to end. Do not add broker-local, waker-local, or worker-local duplicates for the same path; extend `ActionCommand`, `ControlCommand`/`ControlKind`, `WakeCommand`, `WsIngressCommand`, or `UiEvent` (the fan-out UI event on the `events` channel) and thread that type through every relevant backend and delivery wrapper.
- Do not add direct waker-to-worker or worker-to-waker channels. Worker-originated control requests travel worker→`ingress`→engine (`WsIngressCommand::Control`); API-originated control travels ws→engine→`control`→worker (`ControlCommand`). The two directions use distinct channels so neither consumes its own messages.
- Discovery/gossip types in `runinator-comm` should stay transport-friendly and serde-compatible.

When adding fields to shared structs, check every boundary that serializes, persists, or maps that type:

- `runinator-models`
- `runinator-comm`
- `runinator-store` (the trait declaration: `roles/<domain>.rs`, or `reducer_store.rs` if the state machine calls it)
- `runinator-database/src/mappers.rs`
- SQLite/Postgres implementations (`operations/<same file name as the role>.rs`)
- `runinator-reducer/src/test_support.rs`, if the operation lands on `ReducerStore`
- `runinator-api`
- ctl task/pack import (WDL compile + `workflows apply` compiled-pack zip)
- command center models, if the field is user-facing

## Provider And Plugin Guidance

Providers execute task actions; they are not schedulers, API clients, or persistence layers.

- Keep provider resolution in `runinator-worker`.
- Keep dynamic library loading and FFI safety wrappers in `runinator-plugin`.
- Treat plugin ABI names (`runinator_marker`, `name`, `call_service`) as public contracts.
- Dynamic plugins are a **host-only** capability. `deploy/Dockerfile` links statically (`+crt-static`
  is the musl default), and a static binary has no dynamic loader, so containerized workers cannot
  `dlopen` a `.so`; they run only the providers compiled into them. Do not add a plugin directory,
  a `--dll-path` argument, or a plugin-staging init container to the container images or the k8s
  manifests. `runinator-desktop-agent`, `xtask local up`, and the release bundles are the paths that
  ship plugins. A new provider that must work in k8s has to be a compiled-in `runinator-provider-*`
  crate, not a plugin.
- Provider/action metadata belongs next to the executable provider: built-ins expose it through `Provider::metadata()`, and plugins expose it through the `metadata` ABI function. Do not duplicate provider metadata in workflow or provider packs.
- For third-party integrations, look for a well-maintained client library before hand-rolling HTTP payloads and API semantics.
- Always add a new provider as a separate crate: `runinator-provider-<name>`.
- Keep `action_name`, `action_function`, and `action_configuration` semantics compatible with existing task import and execution paths.

## Database And API Guidance

The database crate owns persistence behavior. The web service owns HTTP behavior.

- Add a new persistence operation to the `runinator-store` role trait that owns its domain (`roles/<domain>.rs`), then write the body in the matching `runinator-database/src/operations/<domain>.rs`. One generic body covers SQLite, Postgres, and MySQL together. Do not add methods to `DatabaseImpl` itself — it only composes the roles.
- Keep SQLx row mapping centralized in `runinator-database`, especially `mappers.rs`.
- Keep repository functions in `runinator-engine/src/repository/` focused on persistence orchestration. HTTP response mapping belongs in a handler crate's `src/handlers/`, and SQL belongs in `runinator-database`.
- Keep public API payloads in shared model/API crates when they must be consumed by multiple binaries or the command center.

### When a ws handler may call the store directly

A handler may call `db.*` itself **only** when the endpoint is thin CRUD over a row the runtime does
not orchestrate: authentication, orgs/memberships, and billing (`runinator-ws-identity`'s
`handlers/{auth,orgs,billing}.rs`) plus `runinator-ws-authoring`'s `handlers/{credentials,catalog}.rs`.
Those have no engine counterpart on purpose — routing them through a repository module would add a
call layer and no behavior. The one other direct call is `runinator-ws-runtime`'s
`handlers/health.rs` readiness probe, which reads one row to test database connectivity and does not
care what the row says.

Anything carrying orchestration semantics goes through `runinator-engine/src/repository/`, even
when the body is a single delegating call: runs, node runs, triggers/firings, action dispatches,
notifications and their policies, pipelines, replicas. The test is not how short the function is —
it is whether a future version of the operation would need to touch run state, the dispatch outbox,
the ready-node queue, or emit an event. Those all belong to the engine, and a handler that reached
past it would be the place the next rule gets broken. `create_notification` is the worked example:
it looks like a one-line insert, but it must also resolve the run's owning org so the ui event is
scoped, so it lives in `repository/notifications.rs` and returns `CreatedNotification`.

When in doubt, put it in the engine — that direction is always safe, and the CRUD exemption above
is a closed list, not a pattern to extend.

### The web service crates

The http surface is six crates, layered so nothing depends back up:

```
runinator-ws                    router/server/websocket/openapi assembly, config, binary
  ├── runinator-ws-identity     handlers/{auth,orgs,billing}
  ├── runinator-ws-authoring    handlers/{workflows,wdl,packs,pipelines,credentials,catalog,providers}
  ├── runinator-ws-runtime      handlers/{runs,node_runs,artifacts,triggers,schedules,
  │                                       action_dispatches,replicas,notifications,debug,automation,
  │                                       observability,health,supervisor,provisioning,
  │                                       catalog_metadata,webhook}
  ├── runinator-ws-middleware   auth, authz, rate_limit, overload
  └── runinator-ws-core         models, responses, events, json, openapi::{docs,examples}
```

The domain line is what an endpoint *is about*, not its url prefix: `runinator-ws-authoring`
describes what can run, `runinator-ws-runtime` drives and observes what is running, and
`runinator-ws-identity` is who may do either. A handler crate depends on core and middleware and on
nothing else in this list.

`runinator-ws` keeps only assembly and the pieces that need the whole surface at once: `router.rs`,
`server.rs`, `websocket.rs`, `openapi/`, `config.rs`, `main.rs`, and the behavior test suite (which
boots a real `SqliteDb` and drives handler + reducer + persistence together, so it cannot live in any
one domain crate). Its `lib.rs` re-exports `handlers`, `models`, `events`, `auth`, `authz`, and the
engine's `repository`/`stability` at their historical `crate::…` paths, so assembly code and tests
read the same as before.

### Adding an endpoint

An endpoint lives in exactly one file. Each handler crate's `handlers/<domain>.rs` — plus
`runinator-ws`'s `websocket.rs` and `openapi/mod.rs`, which serve their own routes — owns three
things side by side: the handler fns, a `pub fn routes<T: DatabaseImpl>(pool: Arc<T>) -> Router`
holding that domain's `.route()` registrations, and a `pub const DOCS: &[EndpointDoc]` holding its
openapi entries. Adding an endpoint is one file, three additions.

Pick the file by domain, and if the domain is new, add the module to that crate's `handlers/mod.rs`
— not a new crate. `router.rs` only merges the fragments and applies the middleware stack;
`openapi/mod.rs` only concatenates the `DOCS` slices through `DOC_SETS` and enriches the generated
document. Do not add a `.route()` call or an endpoint doc to either — they hold no per-endpoint
knowledge, which is what keeps them ~190 and ~400 lines against 166 routes.

The shared doc vocabulary stays central in `runinator-ws-core`'s `openapi/docs.rs`: the
`EndpointDoc`/`RequestDoc`/`ParamDoc` model, the `endpoint()`/`json_body()` constructors, and the
reusable query-param consts (`CURSOR`, `WORKFLOW_FILTERS`, …), several of which are used by more than
one domain. Examples live in `openapi/examples.rs`, one `Example` arm plus one match arm each.

Two source lints guard the layout. Both live in `runinator-ws` because they are the only checks that
see the merged surface, both read the sibling crates by path, and both are ratchets in each
direction:

- `openapi/route_parity.rs` diffs the registered routes against the documented ones. Because
  registration is per-module and the modules are in three crates, it reads `ROUTER_SOURCES` — an
  `include_str!` list that cannot be globbed and cannot reach a crate by name, so its handler entries
  are relative paths like `../../../runinator-ws-runtime/src/handlers/runs.rs`.
  `route_sources_cover_every_module` walks every handler directory and fails if a module with a
  `routes()` fn is missing from that list; a module left off would silently drop all of its routes
  out of the parity check.
- `store_access_tests.rs` keys its allowlist on `<crate>/<file>`, so moving a handler between files
  *or between crates* means updating its entry.

Both read the crate list from `runinator-ws`'s `HANDLER_CRATES`, so a new handler crate must be added
there or it is invisible to both guards.

`Router::merge` panics on a duplicate method+path, so the split cannot silently shadow a route.
Note that path prefix does not imply owning module: `/workflows/{id}/grants` is served and
documented by `runinator-ws-identity`'s `handlers/auth.rs`, because that is where its handlers are.

## Authorization

Two axes, both enforced backend-side; see `docs/permissions.md` for the full model.

- **Capabilities** are the named, documented catalog of platform/org privileges (`runinator-models/src/capabilities.rs`, mirrored to the command center). Gate a privileged handler with `authz::require_capability(&ctx, Capability::X)` and add the caller's set to the resolver in `authz::capabilities_for`. Do **not** add a new bare `require_admin` gate for a user-facing action — add a capability so the backend and the ui reference one dictionary. `require_admin`/`require_service_or_admin` remain for platform-admin-or-service internal traffic; `require_org_admin(ctx, org_id)` remains for org-scoped resource checks.
- **Resource grants** (`Permission` View/Run/Edit/Own) gate individual workflows/pipelines via `authz::require_workflow`/`require_pipeline`; leave these as-is.
- The command center gates against `GET /auth/me`'s `capabilities`; it hides nav/panels and disables actions the caller lacks, but this never replaces backend enforcement.

## Configuration

CLI/config changes usually affect more than one place.

Check these when adding or renaming runtime options:

- the crate's `config.rs` or `cli.rs`
- `runinator-supervisor.json`
- `README.md` and crate-specific README files
- `deploy/k8s/` manifests and overlays
- Dockerfiles for service binaries
- the `xtask` crate (`xtask/src/`), which builds/publishes/deploys the workspace

Local development defaults should continue to work with:

```bash
cargo build --workspace
cargo run -p runinator-supervisor -- start
cargo run -p runinator-supervisor -- status
cargo run -p runinator-supervisor -- stop
```

`tools/keychain-export` (Swift, macOS Keychain only) and `tools/runinator-secret-sync`
(Go, `client-go`) bridge one operator's local credentials (e.g. a Claude Code login)
into Kubernetes Secrets. They are an optional, macOS-operator-machine bridge for that
one credential source, not part of the portable runtime — the portable credential
path is `CredentialStore` (`runinator-auth`), `SecretCipher` (`runinator-utilities`),
and the settings store's `secret://` references, all of which are OS-agnostic.

## Error Handling And Async

- Prefer returning `SendableError` where the crate already uses that convention.
- Preserve structured `RuntimeError` codes where call sites already use them.
- Do not use `unwrap` or `expect` in runtime paths unless the process truly cannot continue and existing style already does so nearby.
- Keep blocking provider/plugin execution inside `spawn_blocking` or equivalent isolation.
- Preserve graceful shutdown with `Notify` and `ctrl_c` patterns in service binaries.
- Avoid holding locks across `.await`.

### Error Dictionaries

Every error a crate emits carries a stable numbered code from a per-crate dictionary built on `ErrorDescriptor` (`runinator-models::errors`). A descriptor pairs a numbered code, a dotted runtime key (kept for back-compat lookups), and a short summary; it renders as `"CODE - summary: detail"`. Each crate's `errors.rs` keeps an ordered `DICTIONARY: &[ErrorDescriptor]` exposed through a trait: providers implement `ProviderErrors`, every other crate implements `EngineErrors`.

- Prefixes name the domain, like providers (`JIRA`, `SLACK`, …). `RUNI` is the fallback for the engine *runtime* crates that have no self-contained error vocabulary — `runinator-ws`, `-worker`, `-waker`, `-plugin`, `-database`, `-utilities` — partitioned by per-crate number range (ws=`RUNI1xx`, worker=`RUNI2xx`, …). Crates with their own domain vocabulary get a crate-specific prefix instead: `runinator-broker`=`BROKER`, `-comm`=`COMM`, `-api`=`API`, `-wdl`=`WDL`, `-workflows`=`WORKFLOW`. Two dictionaries are shared across a crate family rather than per-crate, in both cases because the family emits one error type: `WDL` is defined once in `runinator-wdl-syntax`'s `errors.rs` for `runinator-wdl`, `-wdl-syntax`, `-wdl-sema`, and `-wdl-codegen`, which all emit `WdlError`; `WORKFLOW` is defined once in `runinator-compute`'s `errors.rs` for it and `runinator-workflows`, which both emit `WorkflowValidationError`. `BROKER` is defined in `runinator-broker-core` and re-exported by `runinator-broker`.
- For ad-hoc errors, build a descriptor and call `.error(detail)` (or `.bare()`); do not hand-roll `RuntimeError::new` with a one-off code string. Add new errors as the next number in that crate's range.
- For crates whose errors are a `thiserror` enum, keep the enum (matching, `#[from]`/`#[source]` stay intact) and apply the code two ways: prefix each variant's `#[error("CODE - …")]` string, and add a parallel `ErrorDescriptor` `DICTIONARY` + `EngineErrors` impl in the same `errors.rs`. Keep the `#[error]` literal and its dictionary entry in sync.
- lib crates expose `pub mod errors;` so their bins reference descriptors by path; a bin that owns its `errors.rs` may need `#![allow(dead_code)]` since bins flag unused `pub` items. The desktop `runinator-command-center` is out of scope for this catalog.

## Tests And Verification

Before handing off non-trivial Rust changes, run the narrowest useful checks first:

```bash
cargo fmt --all --check
cargo test -p <crate>
cargo test --workspace
```

Use `cargo check -p <crate>` when a full test run is slow or when the crate has no tests. If a change touches shared contracts, prefer `cargo test --workspace`.

For command center changes, use the existing Tauri build path and verify UI behavior separately.

`.github/workflows/ci.yml` runs the same checks on push/PR: `cargo fmt --all --check` and
`cargo test --workspace` on linux with postgres/mariadb service containers (so the dialect-parity
suites actually execute rather than skipping), `runinator-provider-db`'s live connector suite
against postgres, mongo, **and both mariadb:11 and mysql:8**, the broker-backend suites against
live kafka and rabbitmq, a compile job over the optional features, a `cargo check --workspace
--all-targets` compile job on macos and windows, and the command center's `pnpm test`/`lint`/`build`.
`.github/workflows/release-builds.yml` is separate and only runs on dispatch or a published release.

### Verifying a broker-backend or optional-feature change

A default `cargo test --workspace` builds default features only, which leaves the opt-in broker
backends and the kubernetes provisioner compiled by nobody — including the exact feature set
`deploy/Dockerfile` ships. The `optional-features` job compiles those, and it uses `--all-targets`
deliberately: that is what builds the per-backend integration tests, which is what stops them
drifting out of sync with a changed `ActionCommand`.

The `broker-backends` job runs the kafka and rabbitmq suites against real brokers. Both are
`#[ignore]`d and **self-skip into a green run** when their env var is missing, so the job greps the
output for the skip line and fails on it — a passing exit code alone does not mean a broker was ever
touched. Keep that guard if you touch the step, and keep `--nocapture`, which is what makes the skip
visible.

To run them locally:

```bash
docker run -d --name ci-rabbit -p 5672:5672 rabbitmq:4-alpine
RUNINATOR_RABBITMQ_URI=amqp://guest:guest@127.0.0.1:5672/%2f \
  cargo test -p runinator-broker --features rabbitmq --test rabbitmq -- --ignored
```

Kafka additionally needs its topics created first (`runinator.actions`, `runinator.control`,
`runinator.results`); see the `Start Kafka` step in `ci.yml` for the single-node KRaft invocation.

The two-engine mysql matrix is not redundancy. mysql and mariadb report the same column types
under different names, and each hides bugs the other exposes: mariadb implements `json` as
`longtext` and reports it as BLOB (mysql 8 reports JSON), while mysql 8 serves `information_schema`
from the data dictionary, uppercasing its column labels and returning them as VARBINARY. Both
engines also report `boolean` and `tinyint(1)` as the same BOOLEAN, which sqlx's `bool` decode
flattens. `connector/sql/decode.rs` resolves each of these from the payload rather than the type
name, and column metadata is reconciled against the decoded value so `kind` never contradicts it.
Test a connector change against both images.

DECIMAL/NUMERIC decodes through `BigDecimal` (the sqlx `bigdecimal` feature), because a json number
is an f64 and both engines allow precisions far past what that holds. `decimal_to_json` emits a
number only when the value round-trips through f64 unchanged, and the exact digits as a string
otherwise — so a `numeric(40,8)` never silently rounds. Note that postgres's wire format drops
trailing zero groups, so a value's *scale* does not survive it; do not write assertions that depend
on trailing zeros.

MongoDB is a first-class store backend and ships by default (`mongo` is a default feature of
`runinator-provider-db`; `--no-default-features` opts out). Its `bson` dependency hard-enables
`serde_json/preserve_order`, and cargo unifies that across the workspace — which is fine, because
the domain and wire type is `runinator_models::value::Value`, whose `Map` is its own `BTreeMap`.
Every value crossing a workflow, the reducer, or the database stays sorted-key by construction, and
`preserve_order` reaches only raw `serde_json::Value` in provider internals, where insertion order
is the better answer anyway. Do not reintroduce a "keep bson out of the workspace" workaround. The
one thing this does change: `serde_json::Value` is no longer a sorted-key reference, so assert our
wire form against a literal rather than against a serde_json round trip — see
`runinator-models/src/tests.rs`.

A workspace-wide cargo invocation unifies features across every member, including
`runinator-command-center/src-tauri`. That is why `runinator-desktop-agent` pins `rfd` to the same
feature set `tauri-plugin-dialog` selects — the two default backends (`xdg-portal` vs `gtk3`) are
mutually exclusive and rfd's build script panics on linux if unification turns both on. Check for
this whenever a workspace crate and the tauri crate share a transitive dependency.

### Verifying a schema or persistence change

A migration or `operations/` change is only half-tested by `cargo test -p runinator-database`: the
default run has no postgres or mysql, so both dialect suites skip themselves. Bring the engines up
and run it again:

```bash
docker compose -f runinator-database/tests/docker-compose.yml up -d --wait
RUNINATOR_TEST_POSTGRES_URL=postgres://runi:runi@127.0.0.1:55433/runi \
RUNINATOR_TEST_MYSQL_URL=mysql://root:runi@127.0.0.1:53307/runi \
  cargo test -p runinator-database
docker compose -f runinator-database/tests/docker-compose.yml down -v
```

The assertions are shared: `src/dialect_parity.rs` holds one lifecycle body that all three backends
run, so cover a new operation by adding to it rather than to one dialect's file. `sqlite_lifecycle`
runs it unconditionally, which is what keeps the body honest when nobody has docker up.

Adding a migration means adding it to all three directories under `runinator-database/migrations/`.
`migration_parity_tests.rs` enforces that the version sets match and needs no database, so it fails
in a plain `cargo test` rather than on someone's first postgres deploy. A migration only one engine
can have goes in that file's `DIALECT_ONLY` list with a reason.

## Change Hygiene

- Read nearby code before editing; mirror existing naming, async style, and error conventions.
- Keep edits scoped to the crate that owns the behavior.
- Do not introduce new workspace dependencies for small conveniences.
- Do not move shared structs between crates casually; that is a public boundary change.
- Avoid broad refactors while fixing localized behavior.
- Keep generated/runtime artifacts out of commits, especially `build/`, `target/`, and `.runinator-supervisor/`.
- Update docs/config examples in the same change when behavior changes.

## Architecture Checklist

Before adding code, ask:

- Does this crate own the behavior I am changing?
- Is the dependency direction still from services toward shared contracts, not the reverse?
- Are waker, worker, web service, broker, and database responsibilities still distinct?
- Have all serializers, mappers, API clients, and config files been updated for shared contract changes?
- Can the local supervisor stack still run after this change?
