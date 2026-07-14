# Runinator Enhancement Roadmap

## Context

This is an advisory survey, not a single implementation task. Based on a survey of the workspace (runtime crates, broker, database, auth, and the Tauri/Vue command center), the codebase is architecturally sound and feature-rich, but has clear gaps in **operational maturity**, **frontend polish/accessibility**, and **runtime/language completeness**. Below is a prioritized roadmap; each item names the owning crate(s) and key files so any one can be turned into a focused implementation plan later.

The guiding constraint from `AGENTS.md`: keep dependency direction services→shared-contracts, keep changes scoped to the crate that owns the behavior, and thread any shared-contract change through every broker backend, mapper, and config file.

---

## Tier 1 — Highest leverage (operational hardening)

These make the distributed runtime debuggable and safe to run in production. Today there is no way to correlate a workflow run across ws → broker → worker.

### 1.1 Distributed tracing + metrics — ✅ implemented
- **Status:** `runinator-utilities/src/logger.rs` now installs a `tracing-subscriber` registry (stdout + file layers, `RUNINATOR_LOG` `EnvFilter`, `log` macros bridged). A `trace_id` correlation id is carried on the `ActionCommand` / `WorkflowResultEvent` envelopes in `runinator-comm` (serde-default, so it flows through every broker backend) and the worker enters an `execute_action` span (`trace_id`/`run_id`/`node_id`/`attempt`) over each delivery. `runinator-ws` exposes a Prometheus `/metrics` endpoint (public, in OpenAPI) and the `stability.rs` counters (applied/duplicate/retried/dead-lettered/receive-errors) are promoted to `metrics` counters. Remaining/optional: OpenTelemetry OTLP export and W3C traceparent interop.
- **Problem:** No `tracing`/OpenTelemetry and no metrics export anywhere (grep confirms zero). Logging is plain-text `fern`/`log` only. Multi-process issues are effectively undebuggable.
- **Approach:** Adopt the `tracing` ecosystem in `runinator-utilities/src/logger.rs` (swap/augment fern with `tracing-subscriber`), propagate a `trace_id`/`run_id` span context across the broker boundary by adding it to the shared `runinator-comm` command envelopes (so it survives ws→worker hops). Add a Prometheus `/metrics` endpoint in `runinator-ws`. Promote the existing in-memory counters in `runinator-ws/src/stability.rs` (applied/duplicate/retried/dead-lettered) to real metrics.
- **Boundary note:** trace context on broker messages is a `runinator-comm` contract change — thread through every broker backend.

### 1.2 Persisted dead-letter queue + audit log — ✅ implemented
- **Problem:** Dead-lettered result/ingress events were logged and ack'd into the void (`runinator-ws/src/result_consumer.rs`, `background.rs`); there was no durable record of failed messages and no audit trail for auth/sensitive ops.
- **Done:** Added `dead_letters` and `audit_log` tables (sqlite/postgres/mysql migrations `20260620000001_dlq_audit.sql`), `mappers::row_to_dead_letter`/`row_to_audit_log`, and `DatabaseImpl` methods `record_dead_letter`/`fetch_dead_letters`/`record_audit_log`/`fetch_audit_log`. The result and ingress consumers persist a dead-letter record before acking the poison message (`crate::audit::persist_dead_letter`). Login success/failure and authz denials (`require_workflow`) are recorded via `crate::audit::record_audit`. Admin-only `GET /dead_letters` and `GET /audit_log` (in OpenAPI) expose them, with a command-center **Dead Letters** and **Audit Log** view (admin-gated nav, HTTP + Tauri runtimes).

### 1.3 Retry backoff + provider resilience — ✅ implemented
- **Problem:** Fixed 250ms retry backoff with no jitter (`result_consumer.rs`). No external-HTTP timeout on some providers (reqwest defaults).
- **Done:** `ResultConsumerPolicy` now computes exponential backoff with full jitter capped at 30s (`backoff_for`). Provider HTTP clients all set explicit timeouts; the one default client (`runinator-provider-email` notification post) now builds a 30s-timeout client. (Most providers already bound requests by `request.timeout_secs`.)

### 1.4 Rate limiting — ✅ implemented
- **Problem:** No rate limiting on the HTTP API (`runinator-ws/src/auth.rs` gates auth but not volume) — DoS/abuse risk once exposed.
- **Done:** Added `runinator-ws/src/rate_limit.rs`, a tower middleware token bucket keyed by the resolved principal (falling back to the connection IP), layered inside auth so it sees `AuthContext`. Configured via `RUNINATOR_RATE_LIMIT_ENABLED`/`_RPS`/`_BURST` (off by default); `/health`, `/ready`, `/metrics` exempt; over-limit returns `429` + `Retry-After`.

---

## Tier 2 — Runtime & language completeness

These remove known footguns and "reserved but not implemented" features.

### 2.1 `poll`/`while` loop construct (WDL + reducer) — ✅ implemented

### 2.2 Race-branch cancellation — ✅ implemented

### 2.3 Plugin FFI cancellation — ✅ implemented

### 2.4 Authorization phase 2 — ✅ implemented
- **Problem:** Resource-based authz was scaffolding only (`runinator-ws/src/authz.rs`, `runinator-models/src/auth.rs`); only workflow-level Own/Edit/Run/View with admin bypass, no enforcement coverage.
- **Done:** Grant enforcement is wired across every workflow/run-scoped endpoint — workflows, runs, node-runs, triggers, gates/approvals/automation records, artifacts, debug — via `require_workflow`/`require_run_workflow`/`require_*_workflow`, with `visible_workflow_ids`/`filter_records` scoping list responses and `grant_owner` stamping creators. Runs and sub-resources inherit the parent workflow's permission (no separate ResourceType yet — by design). Api-key minting is inline-scoped (only admins mint service/cross-user keys). The command-center `PermissionsView` (Users/Teams/Access/API-keys) is fully wired and admin-gated via `visibleNavSections()`. Enforcement is covered by `workflow_permission`/`require_workflow`/`visible_workflow_ids` tests in `runinator-ws/src/tests.rs`.

---

## Tier 3 — Command center UX & accessibility

The desktop client is feature-complete but light on polish; these are user-facing wins.

### 3.1 Dark mode
- Light theme only; hard-coded color tokens in `src/styles/base.css` (`--surface: #ffffff`, etc.). Introduce CSS theme variables + `prefers-color-scheme` + a toggle. Mechanical but broad.

### 3.2 Accessibility pass
- ~29 ARIA attributes across 62 components. Add `aria-label`/`title` to icon buttons, focus trapping in modals (`WorkflowStepEditorModal.vue`), text fallback for color-only status badges, and a semantic heading hierarchy.

### 3.3 Live expression preview
- `ExpressionJsonEditor.vue` detects unresolved references but cannot evaluate expressions against sample data. Per memory `project_cc_expression_editor`, a TS-only evaluator/live-preview isn't possible today. Option: a ws "evaluate expression" endpoint the editor calls for preview — highest-value editor improvement.

### 3.4 Bulk actions, loading/empty states, error recovery
- No multi-select/bulk enable-disable-delete-rerun across workflows/runs. Add skeletons/spinners (only "Loading logs…" exists), richer empty states, and a "Retry" affordance on the error toast (`ToastHost.vue`) instead of console-only errors.

---

## Tier 4 — Test coverage & robustness

### 4.1 Backend test gaps
- Zero tests: `runinator-waker`, `runinator-supervisor`, `runinator-bootstrap`, and providers `aws`/`catalog`/`console`/`sql`. The waker (core timer/relay loop) being untested is the notable risk.
- Add DB round-trip/migration tests in `runinator-database` (none currently exercise sqlite↔postgres schema parity).

### 4.2 Frontend test gaps
- 0% component test coverage for the entire `components/workflow/` directory (canvas, node, step editor — the most complex, highest-LOC components). Utilities/stores are well covered (~80%); components are ~5%.

### 4.3 Panic hardening
- `expect()` clusters in `runinator-wdl/src/parser.rs:92-132` (parser state) and `runinator-ws/src/openapi.rs` (11 calls). Convert runtime-path panics to structured `RuntimeError`s per the error-dictionary convention.

---

## Recommended sequencing

1. **Start with 1.1 (tracing/metrics)** — it pays for itself immediately by making every other change observable, and it's mostly additive (no behavior change).
2. **Then 1.2 + 1.3** (DLQ/audit + backoff) — closes the silent-failure paths the tracing will expose.
3. **Pick one Tier-2 item** based on product need: `poll/while` (2.1) unblocks pack migration; authz (2.4) unblocks multi-user deployment.
4. **Dark mode + a11y (3.1/3.2)** can run in parallel as low-risk UX wins.

---

## Verification (per area, when implemented)

- **Backend:** `cargo fmt --all --check`, `cargo test -p <crate>`, then `cargo test --workspace` for shared-contract changes. Confirm the local stack still runs: `cargo run -p runinator-supervisor -- start|status|stop`.
- **Tracing/metrics:** start the supervisor stack, run a workflow, confirm spans correlate a run across ws/worker logs and `/metrics` reports counters.
- **DLQ/audit:** force a failing action, confirm a `dead_letters` row is written and surfaced via API/UI.
- **WDL changes:** round-trip a `.wdl` through compile→decompile→format and confirm idempotency.
- **Frontend:** existing Vitest path (`*.test.ts`) plus the Tauri build path; verify dark mode toggle and keyboard/focus behavior manually.

---

## Note

This roadmap is a survey for prioritization — no single item is fully specified for execution yet. Pick one (e.g. "do 1.1") to get a detailed, file-by-file implementation plan.

---

## Tier 5 — Remaining production gaps (2026-06-29 survey)

Tier 1 operational hardening is largely done (retry/backoff/jitter, executor lease, DLQ/audit, tracing+`trace_id`, `/metrics`, rate limiting, `/health`+`/ready`, graceful shutdown, per-node cancellation). These are the gaps that remain before leaning on the runtime in production.

### 5.1 Waker has no test coverage — highest residual risk
- The waker is the timer/relay heartbeat of the whole system: if it stalls, nothing fires. It currently has zero tests (also noted in 4.1). Add an integration test for the `wake → ingress → drive` path and an alert/metric on wake-channel lag before relying on it in prod.

### 5.2 Slow failover on a dead worker
- `EXECUTOR_LEASE_GRACE_SECONDS = 60` (`runinator-worker/src/main.rs`) means a crashed worker's node run is not reclaimable until `timeout_seconds + 60s` elapses. With long job timeouts, a pod crash strands that node for the full timeout window. Consider invalidating the lease off the worker replica heartbeat (already tracked via `register_replica_session`/`spawn_replica_heartbeat`) instead of only the action deadline.

### 5.3 Panic hardening (carryover from 4.3)
- `expect()` clusters in `runinator-wdl/src/parser.rs:92-132` and `runinator-ws/src/openapi.rs` (11 calls). A malformed pack or request should not be able to panic a handler. Convert runtime-path panics to structured `RuntimeError`s per the error-dictionary convention.

### 5.4 DB migration parity tests
- No tests exercise sqlite↔postgres (↔mysql) schema parity (carryover from 4.1). Schema drift between backends is a classic production surprise; add round-trip/migration parity tests in `runinator-database`.

---

## Tier 6 — Worker / job authoring pitfalls

These are footguns when creating new providers and workflow jobs, grounded in `runinator-worker/src/executor.rs` and `main.rs`. Worth capturing in a provider-authoring checklist so new jobs inherit the right defaults.

### 6.1 Make every provider action idempotent (the big one)
- The executor lease (`claim_workflow_node_run_executor`) prevents *concurrent* duplicate execution, but it **fail-opens on a transport error** (`main.rs:513-517`) and only protects while held. A worker that crashes *after* a side effect but *before* `broker.ack` will redeliver and re-execute. Any action with external side effects (charges, posts, writes) must dedupe on its own key — `workflow_node_run_id` is available in the request and is a natural idempotency key.

### 6.2 A timeout stops *waiting*, not the work
- Provider code runs in `spawn_blocking` (`executor.rs:69`). On timeout the `CancellationToken` is cancelled, but a provider that never polls the token (or has no internal client timeout) keeps running on a blocking thread after the node is already marked `TimedOut`. Consequences: (a) Tokio blocking-pool thread leak (default 512 — exhaust it and the worker wedges), and (b) a "timed out" job still mutating the outside world. **Rule for new providers:** honor the cancellation token in any loop, and set an explicit client timeout ≤ `request.timeout_secs`.

### 6.3 Don't model "wait for X" as a long-running task
- Each in-flight action pins one blocking thread *and* one concurrency permit for its whole duration. A task that sleeps/polls for an hour burns both the entire time. Use the `wait` / `gate` / `signal` node kinds, which park in the reducer with zero worker footprint. Tasks should be short, active work.

### 6.4 Tune `max_concurrent_actions` per workload
- It is a single per-worker semaphore across *all* action types (`main.rs:255`). One memory-heavy job × high concurrency can OOM the pod and starve light jobs queued behind it. For heterogeneous workloads, run separate worker deployments tuned per workload rather than one large pool.

### 6.5 Consumer-group default differs by backend (horizontal-scaling gotcha)
- `broker_consumer_id` defaults to the shared group `runinator-workers` on **kafka**, but to a fresh per-worker `worker_id` UUID on **rabbitmq/http/tcp/in-memory** (`config.rs:90`). Whether N workers *compete* for actions or each receives *every* action depends on the backend's consumer-id→group mapping. **When scaling workers on a non-kafka backend, set `broker_consumer_id` explicitly to the same value across the fleet** so they compete instead of double-executing. Verify on the chosen backend before scaling past one worker.

### 6.6 Secret resolution is on the job's critical path
- `resolve_secret_refs` runs per delivery (`main.rs:532`). If the settings store is unavailable, the job publishes `Failed` and acks — it does *not* retry at the broker level. Jobs touching `secret://` refs should carry a node-level `retry` policy so a transient secret-store blip recovers.

### 6.7 Result-publish failures redeliver the whole action
- If a job succeeds but `publish_status`/`flush` fails (`main.rs:636,680`), the delivery is nacked and the entire action re-runs — looping back to 6.1. Idempotency (6.1) is the mitigation here too.

---

## Tier 7 — Net-new product capabilities (2026-07-12 survey)

Tiers 1–6 cover operational hardening, language completeness, UX polish, and authoring pitfalls. These are **new product features** rather than hardening of existing ones — capabilities the runtime does not have today. Each was confirmed against the current codebase as a genuine gap. Ordered by leverage; #7.1 (test harness) and #7.3 (webhook triggers) are the highest-value picks.

### 7.1 Workflow test harness + dry-run simulation
- **Owning crates:** `runinator-wdl`, `runinator-workflows`, `runinator-ctl`.
- **Problem:** The only way to verify a workflow behaves is to run it live against real providers. There is no offline way to assert which branch a `condition`/`toggle`/`percentage` takes, or that outputs map correctly — the single biggest confidence gap for the WDL surface.
- **Approach:** Add a `.wdlt` test format (or a `test { }` block in WDL) that mocks provider outputs and asserts on the branch taken and final outputs. Pair it with a **reducer dry-run mode** that walks the state machine with `task` nodes stubbed — no `ActionCommand`s published — so authors can preview the branch taken for given inputs. Expose as `runinatorctl workflows test pack/` for CI.
- **Boundary note:** dry-run belongs in the reducer/`runinator-workflows` evaluation path, not the worker; mocked provider outputs must not touch the broker.

### 7.2 AI-assisted WDL authoring in the command center
- **Owning crates:** `runinator-command-center`, `runinator-provider-ai`.
- **Problem:** Authoring WDL/graphs is manual; new users face a blank canvas.
- **Approach:** Natural-language → WDL draft, generated against the live backend-driven node/edge/trigger **catalog metadata** (per memory `project_catalog_metadata_reactivity`). "Add a Slack notify after the approval fails" edits the draft graph in place. The catalog gives the model a constrained, validated tool surface so it emits well-formed graphs rather than free text. Draft stays the source of truth (per `project_cc_workflow_editing`).

### 7.3 Inbound webhook *triggers* (start a run)
- **Owning crates:** `runinator-ws` (`handlers/webhook.rs`, trigger materialization), `runinator-models` (triggers).
- **Problem:** `handlers/webhook.rs` only *wakes/signals an already-parked run*; there is no way to **start** a new run from an inbound event. Triggers are cron-only today (`metadata.triggers`).
- **Approach:** Add a `trigger webhook "..."` header declaration that mints a signed inbound URL to start a new run, with a payload-mapping expression into workflow inputs. Reuse the existing pack-managed-trigger materialization path (`metadata.managed_by = "wdl"`).
- **Boundary note:** the trigger kind is a shared-contract change — thread through `runinator-models` triggers, ctl WDL compile, mappers, and the command-center trigger catalog.

### 7.4 Workflow-to-workflow / event-driven chaining — ✅ implemented
- **Problem:** Workflows are independent; there is no first-class "run B when A succeeds" without modeling B as a `subflow` child of A. (Investigation confirmed chaining does **not** replace subflows: subflow's default is synchronous with a return path, and its one real usage is a mid-run loop fan-out — so subflows were kept.)
- **Done:** New `WorkflowTriggerKind::Chained` (+ `TriggerSourceKind::Chained`), declared on the source workflow as `trigger on_success | on_failure | on_complete workflow "<name>"` and materialized from `metadata.triggers` (each spec now carries a `kind`; absent ⇒ cron for back-compat). Firing is event-driven from the reducer's terminal settle (`runinator-reducer/src/orchestration/chaining.rs`, wired in `engine.rs` beside `maybe_wake_subflow_parent`), **not** the best-effort `events` channel. Exactly-once per (trigger, source-run) via the `workflow_trigger_firings` dedupe table (`db.try_record_trigger_firing`); cycle-bounded by a `chain_depth` cap (32); top-level-only (subflow/map children don't fan out chains). `on:"failure"` matches `Failed`/`TimedOut` but not a manual `Canceled`. Import validates the target resolves (`IMPORT_UNKNOWN_CHAINED_TARGET`). Full WDL surface (grammar/parse/lower/decompile/format idempotent + LSP) and the backend-driven trigger catalog + command-center TS/CodeMirror. Covered by WDL round-trip, DB round-trip/dedupe, and reducer integration tests (start-once, dedupe on re-drive, status selector, depth cap).

### 7.5 Backfill + freeze/blackout windows
- **Owning crate:** `runinator-ws` (trigger-firing loop), `runinator-ctl`.
- **Problem:** No way to replay missed cron slots, and no way to suppress firing during a change freeze or holiday.
- **Approach:** Backfill — `runinatorctl workflows backfill <wf> --from --to` synthesizes trigger firings for past/missed cron slots. Freeze windows — a calendar (change-freeze, holidays) the trigger-firing loop consults to defer firing until the window closes. Both localize to the trigger-firing loop.

### 7.6 Run timeline / Gantt visualization
- **Owning crate:** `runinator-command-center`.
- **Problem:** `trace_id` and per-node timing exist post-1.1, but the only way to inspect a run's shape is reading logs.
- **Approach:** Per-run timeline view: node durations, parked/waiting gaps, retries, and the critical path, rendered from the correlation data already persisted. Far higher debugging value than raw logs; no backend change required beyond exposing node timing already recorded.

### 7.4a Pipelines view — visualize & author chains — ✅ implemented
- **Problem:** #7.4 shipped the backend, but chains were only visible/editable one workflow at a time in the Settings modal — no way to see the pipeline DAG.
- **Done:** New top-level **Pipelines** tab (left nav, ahead of Workflows) — a Vue Flow canvas with one node per workflow and one edge per `chained` trigger, labelled by selector. Interactive: drag between workflows to create a chain, click an edge to change `on` (success/failure/complete) or enable/disable, delete to remove — all through the existing trigger CRUD (`save_workflow_trigger`/`delete_workflow_trigger`). Reuses `autoArrangeWorkflowLayout` for positioning; data via `fetchWorkflows()` + a fan-out over `fetchWorkflowTriggers()`. New portable `pipeline-graph.ts` builder (unit-tested, flags unresolved target names), `services/pipeline`, a `pipeline` pinia store, and `PipelineNode`/`PipelineCanvas`/`PipelinesView`. Also fixed a bug from #7.4: the `chained` target field used the `subflow` widget (stores an **id**), but chaining resolves by **name** — switched to a `workflow_name` widget so both the canvas and the Settings modal write a resolvable name. Frontend-only except that one-line catalog widget change.

### 7.7 AI cost & token accounting
- **Owning crates:** `runinator-provider-ai`, `runinator-models` (result event), `runinator-database`.
- **Problem:** `provider-ai` (claude_code) captures **no** token/cost usage. There is no hook to attribute AI spend per node/run/workflow.
- **Approach:** Capture usage in the provider, thread it back on the `WorkflowResultEvent`, persist per node-run, and roll up per run/workflow in the command center.
- **Boundary note:** adding usage to the result event is a `runinator-comm`/`runinator-models` contract change — thread through every broker backend, `mappers.rs`, and both DB backends.

### 7.8 Pack environments + promotion
- **Owning crates:** `runinator-ctl`, `runinator-ws` (packs), settings store.
- **Problem:** `semver.rs` exists but there is no dev→staging→prod lifecycle; a pack imports with one fixed set of config/secret bindings.
- **Approach:** Environment-scoped pack deployment with a diff/promote flow (`runinatorctl workflows promote <pack> staging→prod`) and per-environment config/secret binding, so the same compiled pack runs against different settings-store values per environment.

---

### Recommended sequencing (Tier 7)

1. **7.1 (test harness)** — highest authoring-safety leverage, self-contained in the WDL/reducer evaluation path, no shared-contract churn.
2. **7.3 (webhook triggers)** — highest reach; reuses the pack-managed-trigger path. **7.4 (chaining)** and **7.5 (backfill/freeze)** are natural follow-ons once trigger kinds are extensible.
3. **7.6 (run timeline)** — pure frontend win on data already persisted.
4. **7.7 (AI cost)** and **7.2 (AI authoring)** as the AI surface grows.
5. **7.8 (environments)** once multi-env deployment is a real need.
