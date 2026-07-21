# AGENTS.md

Guidance for agents working in this repository. Keep changes aligned with the existing architecture before adding new abstractions or cross-crate dependencies.

## Project Shape

Runinator is a Rust workspace for scheduling and executing tasks across a small distributed runtime using a resumable state-machine orchestrator.

Primary runtime flow:

1. `runinator-ws` owns the HTTP API and the reducer, persists scheduled tasks/workflow runs/orchestration records through `runinator-database`, publishes scheduled work on the broker `wake` channel, publishes `ActionCommand`s on the broker action channel, consumes the broker `ingress` channel (drive + control requests), and runs the trigger-firing, action-dispatch, wake-publisher, and reconcile loops in-process.
2. `runinator-waker` is a small, horizontally scalable, broker-only timer/relay: it consumes the `wake` channel, sleeps until each ready node is due, then publishes a drive on the `ingress` channel. It has no database, no HTTP client to the web service, and shares no channel with the worker.
3. `runinator-worker` polls the broker action channel, resolves a provider/plugin, executes the task, and publishes results on the broker result channel (records also reachable through `runinator-api` compatibility endpoints). It self-publishes its built-in provider metadata to the web service on startup.
4. `runinator-ctl` is the control CLI (`runinatorctl`). Among other commands, `workflows apply` is the one-shot pack importer: it compiles a `.wdl`/`.wdlm`/directory (including any `.wdls` secrets and `.wdlp` pipelines) **client-side**, zips the compiled artifacts (`workflows.json` + optional `secrets.json` + optional `pipelines.json`), and uploads a single `application/zip` to the web service's `/packs/import` endpoint. Compilation never happens on the backend; `/packs/import` only reads the compiled JSON. There is no long-running importer service. Pack zip read/write lives in `runinator-utilities::pack`; the entry-name layout is the wire contract shared by ctl/api/command-center/mcp (writers) and ws (reader). File extensions: `.wdl` workflow, `.wdls` settings, `.wdlm` pack manifest (JSON — lists `workflows`/`pipelines`/`settings`), `.wdlp` pipeline (WDL pipeline grammar), `.wdlt` tests.
5. `runinator-supervisor` runs the local stack from `runinator-supervisor.json`.

There is also a Tauri `runinator-command-center` client. Keep frontend UI changes separate from runtime crates unless the change explicitly touches the desktop UI.

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
- `runinator-database`: persistence interfaces and concrete SQLite/Postgres implementations. Database-specific mapping belongs here, not in `runinator-ws`.
- `runinator-ws`: API server, reducer, and repository orchestration. It should depend on the database trait, not on worker/waker/provider internals.
- `runinator-broker`: broker trait, message/delivery types, in-memory backend, HTTP backend/client/server, and future broker adapters. Channels are `action`, `control` (ws→worker), `result` (worker→ws), `wake` (ws→waker), `ingress` (waker/worker→ws), and `events` (ws→every ws replica). All channels except `events` are competing-consumer (one delivery per consumer group); `events` is **fan-out** — every subscriber receives every message (rabbitmq fanout exchange, per-replica kafka group, per-consumer in-memory/wire receiver), so ws replicas can fan UI events to all connected WebSocket clients. The `action` and `control` channels are additionally **target-routed**: commands carry an `ActionTarget` and consumers use `receive_for`/`receive_control_for` with a `ConsumerProfile`, so a pinned action or a cancel stamped with the executor-holding replica reaches only a matching worker (backends without native routing bounce mismatches via nack; a targeted control nobody can match is dropped after `STALE_CONTROL_TTL_SECONDS`). Waker, worker, and web service should talk to the `Broker` trait where practical. A new channel must be implemented across every backend (in-memory/http/tcp/kafka/rabbitmq) and both wire transports.
- `runinator-waker`: broker-only timer/relay. It consumes the `wake` channel, sleeps until due, and publishes a drive on the `ingress` channel. It must not execute task providers, must not write to the database, and must not depend on `runinator-api` or the worker.
- `runinator-worker`: task execution loop and provider resolution. It should not calculate schedules or mutate state except through API calls intended for worker results.
- `runinator-workflows`: workflow validation, graph cycle detection, and condition evaluation logic.
- `runinator-wdl`: the WDL surface language (grammar, parser, lowering to the JSON workflow model, and decompiling back), plus the `.wdls` secrets front end and the `.wdlp` pipeline front end (`parse_pipeline_str` → `PipelineBundle`, `pipeline_to_wdlp` back). It must round-trip every node kind's parameters, but its grammar must only express well-formed graphs. Do not add WDL syntax for degenerate or malformed graphs (e.g. a parallel with no matching join, a condition with no branches, a missing start node); the decompiler may error on such JSON instead. Keep the grammar a description of valid programs, not a serializer for every possible JSON shape. Header `trigger cron "..."` declarations and input-field defaults are carried in `definition.metadata.triggers` / the field's `default`; the web service materializes pack-managed triggers (`metadata.managed_by = "wdl"`) on import. A `.wdlp` pipeline lowers to a portable `PipelineBundle` (members + links by workflow name); on import the web service resolves names to ids, upserts the `Pipeline`, and materializes each link as a managed `chained` trigger carrying `configuration.pipeline_id` (reconciled by pipeline id; header-trigger reconciliation skips triggers that carry a `pipeline_id`). The pipeline itself never runs — its chained triggers are the runtime linkage.
- `runinator-plugin`: dynamic plugin loading and `Provider` trait integration. Keep FFI details contained here.
- `runinator-provider-*`: provider implementations. Always implement a new library for a new provider. Keep provider-specific configuration and external system behavior out of core crates.
- `runinator-utilities`: small cross-cutting helpers such as startup/logging, credential store trait, and data export. Do not turn this into a dumping ground for domain logic.

If a change requires a dependency from a lower-level/shared crate back into a service crate, stop and redesign the boundary.

## Coding Standards

- Favor guard clauses over deep nesting to keep logic flow flat and readable.
- If a functionality can have different implementations, always use traits to define the interface.
- Favor comments as appropriate for Rust but make them lower case, single line, with a period at the end.
- Use RustDoc comments (`///`) where necessary on public methods, but keep them short, succinct, dense, and dispassionate.
- Do not put all the code for a library in `lib.rs`; break it out into smaller, focused files.
- Do not put tests in the same files as code; break them out into a `tests.rs` file (or a `tests` module in a separate file).

## Runtime Contracts

Preserve the command lifecycle:

- Workflows are executed as state-machines with nodes like `task`, `wait`, `condition`, `approval`, `loop`, and `subflow`.
- The web service owns the reducer and publishes `ActionCommand` values through `runinator-broker` for `task` nodes (drained from the durable `workflow_action_dispatches` outbox by an in-process publisher loop). The waker never publishes `ActionCommand`s.
- The web service publishes a `WakeCommand` on the `wake` channel for every pending ready node (the wake-publisher loop doubles as the durable reconcile backstop; the broker dedupes wakes already in flight). The waker relays a due wake to a `WsIngressCommand::Drive` on the `ingress` channel, which the web service consumes to run the reducer.
- Workflow run states (`queued`, `running`, `waiting`, etc.) are persisted separately from individual task run statuses.
- Workers acknowledge broker deliveries only after processing and any required result logging has completed.
- Worker outputs, logs, artifacts, and node-run status/results may be delivered as broker result events consumed by `runinator-ws`, or through compatibility endpoints in `runinator-api`; only `runinator-ws` persists them through `runinator-database`, and workers must not write directly to the database.
- Broker messages should remain serializable and backend-neutral.
- Any command or control payload that crosses the broker/waker/worker boundary must use the shared contracts in `runinator-comm` end to end. Do not add broker-local, waker-local, or worker-local duplicates for the same path; extend `ActionCommand`, `ControlCommand`/`ControlKind`, `WakeCommand`, `WsIngressCommand`, or `UiEvent` (the fan-out UI event on the `events` channel) and thread that type through every relevant backend and delivery wrapper.
- Do not add direct waker-to-worker or worker-to-waker channels. Worker-originated control requests travel worker→`ingress`→web service (`WsIngressCommand::Control`); web-service control travels ws→`control`→worker (`ControlCommand`). The two directions use distinct channels so neither consumes its own messages.
- Discovery/gossip types in `runinator-comm` should stay transport-friendly and serde-compatible.

When adding fields to shared structs, check every boundary that serializes, persists, or maps that type:

- `runinator-models`
- `runinator-comm`
- `runinator-database/src/mappers.rs`
- SQLite/Postgres implementations
- `runinator-api`
- ctl task/pack import (WDL compile + `workflows apply` compiled-pack zip)
- command center models, if the field is user-facing

## Provider And Plugin Guidance

Providers execute task actions; they are not schedulers, API clients, or persistence layers.

- Keep provider resolution in `runinator-worker`.
- Keep dynamic library loading and FFI safety wrappers in `runinator-plugin`.
- Treat plugin ABI names (`runinator_marker`, `name`, `call_service`) as public contracts.
- Provider/action metadata belongs next to the executable provider: built-ins expose it through `Provider::metadata()`, and plugins expose it through the `metadata` ABI function. Do not duplicate provider metadata in workflow or provider packs.
- For third-party integrations, look for a well-maintained client library before hand-rolling HTTP payloads and API semantics.
- Always add a new provider as a separate crate: `runinator-provider-<name>`.
- Keep `action_name`, `action_function`, and `action_configuration` semantics compatible with existing task import and execution paths.

## Database And API Guidance

The database crate owns persistence behavior. The web service owns HTTP behavior.

- Add new persistence operations to `DatabaseImpl` first, then implement them for SQLite and Postgres together.
- Keep SQLx row mapping centralized in `runinator-database`, especially `mappers.rs`.
- Keep repository functions in `runinator-ws/src/repository.rs` thin. They should orchestrate database calls and web responses, not duplicate SQL behavior.
- Keep public API payloads in shared model/API crates when they must be consumed by multiple binaries or the command center.

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
- `runinator-stack.yaml`
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

- Prefixes name the domain, like providers (`JIRA`, `SLACK`, …). `RUNI` is the fallback for the engine *runtime* crates that have no self-contained error vocabulary — `runinator-ws`, `-worker`, `-waker`, `-plugin`, `-database`, `-utilities` — partitioned by per-crate number range (ws=`RUNI1xx`, worker=`RUNI2xx`, …). Crates with their own domain vocabulary get a crate-specific prefix instead: `runinator-broker`=`BROKER`, `-comm`=`COMM`, `-api`=`API`, `-wdl`=`WDL`, `-workflows`=`WORKFLOW`.
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
