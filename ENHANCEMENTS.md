# Runinator Enhancement Roadmap

## Context

This is an advisory survey, not a single implementation task. It tracks the remaining gaps in **operational maturity**, **frontend polish/accessibility**, and **runtime/product completeness**, ordered by priority rather than by the survey that discovered them.

Item IDs are **stable** — an item keeps the number it was first filed under (5.3, 6.3, …) even as its priority moves, so "do 6.3" keeps meaning the same thing. The ordering lives in the priority bands below; the tier numbers are just names.

The guiding constraint from `AGENTS.md`: keep dependency direction services→shared-contracts, keep changes scoped to the crate that owns the behavior, and thread any shared-contract change through every broker backend, mapper, and config file.

**Last reprioritized:** 2026-08-04, after re-verifying every open item against the codebase.

---

## Priority at a glance

| # | Item | Band | Owning crates |
|---|------|------|---------------|
| 6.3 | Workflow revision history + diff + rollback | **P0** | database, engine, ws, command-center |
| 6.4 | Declarative idempotency on action nodes | **P1** | worker, comm, models, wdl |
| 3.2 | Heartbeat-driven executor-lease invalidation | **P1** | worker, engine |
| 6.8 | Secret expiry warnings | **P1** | engine, utilities |
| 5.3 | Inbound webhook triggers | **P2** | ws, models |
| 6.5 | Cross-run analytics | **P2** | database, ws, command-center |
| 6.9 | Shareable run forms | **P2** | ws, command-center |
| 5.6 | AI cost & token accounting | **P3** | provider-ai, comm/models, database |
| 5.2 | AI-assisted WDL authoring | **P3** | command-center, provider-ai |
| 5.7 | Pack environments + promotion | **P3** | ctl, ws, settings store |
| 6.6 | Action priority / fairness | **P4** | comm, broker (all backends), engine, worker |
| 6.7 | Retention & redaction policy | **P4** | archiver, database, models |
| 1.2 / 1.4 / 2.2 / 2.3 / 3.4 / 2.1 | Continuous quality track | parallel | varies |

---

## P0 — protect the workflows already running

This covers the runtime's blind spots around work that is *already in production*. Its absence is felt at the worst possible moment, and it is cheap relative to its payoff because the supporting machinery already exists.

### 6.3 Workflow revision history + diff + rollback
- **Owning crates:** `runinator-database` (revision store + migrations), `runinator-engine` (repository), `runinator-ws`, `runinator-command-center`.
- **Verified 2026-08-04:** still open. `workflows` remains one **mutable** row per workflow (`runinator-database/migrations/sqlite/20260101000001_init.sql:40`); `version` became a semver string (`20260608000001_workflow_semver.sql`) but no revision migration exists — the latest migration is `20260726000001_run_correlation_key.sql`.
- **Problem:** In-flight runs are already safe (`workflow_runs.workflow_snapshot`), so the hard part is done — but you cannot see what changed, who changed it, or roll back. Since `runinatorctl workflows apply` overwrites definitions wholesale from a pack, a bad apply is currently unrecoverable.
- **Approach:** Persist each accepted definition as an immutable revision (definition + author + source: ui/pack/api). Yields diff-on-import, "which revision did this run execute", and rollback-to-revision.
- **Boundary note:** this is the foundation **5.7 (pack environments)** needs — sequence 6.3 first.

---

## P1 — close the known correctness gaps

### 6.4 Declarative idempotency on action nodes
- **Owning crates:** `runinator-worker` (claim before invoke), `runinator-engine`, `runinator-models`/`runinator-comm` (action contract), `runinator-wdl`.
- **Verified 2026-08-04:** still open — `runinator-worker/src` and `runinator-comm/src` contain **no** references to idempotency. The `idempotency_keys` table and `/idempotency_keys` endpoints exist, but only as a **manual** store a workflow can call (`runinator-ws/src/handlers/automation.rs:347-376`); the executor path never touches them. Meanwhile the authoring guidance below (A.1) asks every provider author to hand-roll dedupe as a convention.
- **Approach:** `.idempotent(key: <expr>)` on an action node. The worker claims the key before invoking the provider and replays the stored result on redelivery, converting a documented footgun into a platform guarantee. Fixes **A.7** (result-publish failure re-running the whole action) for free.
- **Boundary note:** adding the key to the action contract is a `runinator-comm`/`runinator-models` change — thread through every broker backend, `mappers.rs`, both DB backends, and the WDL compile path.
- **Why P1 and not P0:** it converts existing-but-unused infrastructure into a guarantee, but no current production incident traces to it.

### 3.2 Slow failover on a dead worker
- **Owning crates:** `runinator-worker`, `runinator-engine`.
- **Verified 2026-08-04:** still open. `EXECUTOR_LEASE_GRACE_SECONDS = 60` (now at `runinator-worker/src/worker.rs:37`, used at :490) means a crashed worker's node run is not reclaimable until `timeout_seconds + 60s` elapses. With long job timeouts, a pod crash strands that node for the full timeout window.
- **Approach:** Invalidate the lease off the worker replica heartbeat (already tracked via `register_replica_session` / `spawn_replica_heartbeat`) rather than only off the action deadline. Small, localized, and directly reduces MTTR on a pod crash.

### 6.8 Secret expiry warnings
- **Owning crates:** `runinator-engine` (settings store), `runinator-utilities` (credential store).
- **Problem:** The creds-sync pack copies credentials on a cron, but nothing warns **before** one expires — the first signal is a failed job.
- **Approach:** Optional expiry metadata on settings-store secrets and a scan that raises a notification ahead of expiry.
- Delivered through the shipped notification-policy layer (`notification_policies` / `notification_deliveries`); it is the cheapest proof that layer generalizes beyond run failures.

---

## P2 — extend reach

### 5.3 Inbound webhook *triggers* (start a run)
- **Owning crates:** `runinator-ws` (`handlers/webhook.rs`, trigger materialization), `runinator-models` (triggers).
- **Verified 2026-08-04:** still open. `WorkflowTriggerKind` remains exactly three variants — `Cron`, `Manual`, `Chained` (`runinator-models/src/workflows.rs:196-229`). `handlers/webhook.rs` only *wakes/signals an already-parked run*; there is no way to **start** one from an inbound event.
- **Approach:** Add a `trigger webhook "..."` header declaration that mints a signed inbound URL to start a new run, with a payload-mapping expression into workflow inputs. Reuse the existing pack-managed-trigger materialization path (`metadata.managed_by = "wdl"`).
- **Boundary note:** a new trigger kind is a shared-contract change — thread through `runinator-models` triggers, ctl WDL compile, mappers, and the command-center trigger catalog.
- **Ranking note:** the highest-reach item for *new* work, and ranked below P0 only because that protects workflows already running. The shipped per-workflow concurrency policy makes it meaningfully safer — an unthrottled inbound webhook is precisely the source that makes such a policy mandatory.

### 6.5 Cross-run analytics
- **Owning crates:** `runinator-database` (aggregate queries), `runinator-ws`, `runinator-command-center`.
- **Verified 2026-08-04:** still open — there are **zero** stats/analytics routes in `runinator-models/src/api_routes.rs`. Per-run Gantt (5.5) and per-replica telemetry exist, but nothing aggregate.
- **Approach:** Success rate and p50/p95 duration by workflow, failure clustering by node/provider, flakiest-node ranking, queue-wait vs. execution time. Mostly a query layer over data already persisted on `workflow_node_runs` (`started_at` / `finished_at` / `attempt` / status) — no new writes. Surfaces "provider X times out 8% of the time".
- **Boundary note:** aggregate SQL belongs in `runinator-database`, not in handlers.

### 6.9 Shareable run forms
- **Owning crates:** `runinator-ws`, `runinator-command-center`.
- **Problem:** The `input` node kind exists, but starting a run requires an authenticated command-center user who understands workflows.
- **Approach:** A signed, scoped "run this workflow" URL that renders the workflow's `input_schema` as a form for non-authors. Opens the platform beyond its authors.
- **Boundary note:** a public-ish surface — reuse the existing capability/resource-grant model (`docs/permissions.md`) rather than adding a bypass path.

---

## P3 — AI surface and multi-environment lifecycle

### 5.6 AI cost & token accounting
- **Owning crates:** `runinator-provider-ai`, `runinator-models`/`runinator-comm` (result event), `runinator-database`.
- **Verified 2026-08-04:** still open — `runinator-provider-ai/src/claude_code.rs` captures no token or cost fields. There is no hook to attribute AI spend per node/run/workflow.
- **Approach:** Capture usage in the provider, thread it back on the `WorkflowResultEvent`, persist per node-run, roll up per run/workflow in the command center.
- **Boundary note:** adding usage to the result event is a `runinator-comm`/`runinator-models` contract change — thread through every broker backend, `mappers.rs`, and both DB backends.
- **Note:** lands more cleanly now that rate cards exist in `billing.rs`, and it pairs naturally with 6.5's aggregate query layer.

### 5.2 AI-assisted WDL authoring in the command center
- **Owning crates:** `runinator-command-center`, `runinator-provider-ai`.
- **Problem:** Authoring WDL/graphs is manual; new users face a blank canvas.
- **Approach:** Natural-language → WDL draft, generated against the live backend-driven node/edge/trigger **catalog metadata**. "Add a Slack notify after the approval fails" edits the draft graph in place. The catalog gives the model a constrained, validated tool surface so it emits well-formed graphs rather than free text. Draft stays the source of truth.
- **Unblocked:** the shipped `POST /workflows/simulate` (5.1) gives generated drafts a validation loop, which is what makes this tractable now.

### 5.7 Pack environments + promotion
- **Owning crates:** `runinator-ctl`, `runinator-ws` (packs), settings store.
- **Problem:** `semver.rs` exists but there is no dev→staging→prod lifecycle; a pack imports with one fixed set of config/secret bindings.
- **Approach:** Environment-scoped pack deployment with a diff/promote flow (`runinatorctl workflows promote <pack> staging→prod`) and per-environment config/secret binding, so the same compiled pack runs against different settings-store values per environment.
- **Blocked on 6.3** — promotion without revision history has nothing to diff or roll back to.

---

## P4 — when scale or customer data makes them urgent

### 6.6 Action priority / fairness
- **Owning crates:** `runinator-comm` (action contract), `runinator-broker` (all backends), `runinator-engine` (dispatch outbox), `runinator-worker`.
- **Problem:** `ActionCommand` has no priority — the only `priority` in the models is edge-selection ordering (`runinator-models/src/workflows.rs:884`). One `map` fan-out of 5,000 items can starve an interactive run behind it on a shared consumer group.
- **Approach:** A priority lane, or weighted fair queueing per org/workflow, pairing with the quota machinery already in `runinator-models/src/billing.rs`.
- **Boundary note:** the most invasive item in the roadmap — priority must be honored by **every** broker backend (in-memory/http/tcp/kafka/rabbitmq) and both wire transports, or ordering silently differs per deployment. Do not start this before P0 is done.

### 6.7 Retention & redaction policy
- **Owning crates:** `runinator-archiver`, `runinator-database`, `runinator-models`.
- **Problem:** `runinator-archiver` ages data out, but run parameters, outputs, and logs are stored raw. There is no field-level redaction and no per-org retention policy.
- **Approach:** Declarable sensitive fields redacted at persist time, plus per-org retention windows honored by the archiver. Becomes urgent the moment customer data lands in a run.

---

## Continuous quality track (run in parallel, low risk)

These are unbounded-effort quality work rather than discrete features. None blocks anything; each can absorb spare capacity.

### 1.2 Accessibility pass
- **Verified 2026-08-04:** ~46 ARIA attributes across 62 components (up from 29, still thin). Remaining: `aria-label`/`title` on icon buttons, focus trapping in modals (`WorkflowStepEditorModal.vue`), text fallback for color-only status badges, semantic heading hierarchy.

### 1.4 Bulk actions, loading/empty states, error recovery
- **Verified 2026-08-04:** still open. `DataTable.vue` has no multi-select; no bulk enable/disable/delete/rerun across workflows or runs. Only one skeleton/loading affordance exists in the whole `ui/` tree. Add skeletons, richer empty states, and a "Retry" affordance on the error toast (`ToastHost.vue`) instead of console-only errors.

### 2.2 Frontend test gaps
- **Verified 2026-08-04:** **0 test files across 21 components** in `runinator-command-center/src/ui/components/workflow/` (canvas, node, step editor — the most complex, highest-LOC components). Core utilities and Pinia adapters remain well covered; presentation components are not.

### 2.3 / 3.3 Panic hardening — narrowed
- **Verified 2026-08-04:** `runinator-wdl/src/parser.rs` is now **clean (0 `expect(` calls)** — that half is done. The remaining cluster is `runinator-ws/src/openapi.rs` (11 calls, e.g. `:114`, `:2407-2572`). These are document-generation paths over structures the file itself just built, so the residual risk is low — convert opportunistically per the error-dictionary convention rather than as a project.

### 3.4 DB migration parity tests
- **Verified 2026-08-04:** still open. `sqlite_tests.rs`, `mysql_tests.rs`, and `mappers_tests.rs` exist, but nothing exercises sqlite↔postgres(↔mysql) **schema parity**. Schema drift between backends is a classic production surprise; add round-trip/migration parity tests in `runinator-database`.

### 2.1 Remaining backend test gaps
- **Verified 2026-08-04, partially closed:** `runinator-waker` now has tests (`src/tests.rs`, 5 cases, including the head-of-line `due_wake_is_not_blocked_by_a_not_yet_due_wake` regression) and metrics (`runinator_waker_wakes_received/driven/requeued_total`, `runinator_waker_drive_failures_total`, `runinator_waker_wake_lead_ms`). `runinator-supervisor` has one test file. Still at zero: `runinator-bootstrap` and `runinator-provider-aws`.
- **Residual:** no end-to-end `wake → ingress → drive` integration test crossing the crate boundary, and no alert wired to the `wake_lead_ms` histogram. Both are small and worth doing — but this is no longer the "highest residual risk" it was in the 2026-06-29 survey.

---

## Appendix — Worker / job authoring pitfalls (reference, not a work queue)

These are footguns when creating new providers and workflow jobs, grounded in `runinator-worker/src/executor.rs` and `worker.rs`. They are **standing authoring guidance**, not scheduled work — they belong in a provider-authoring checklist so new jobs inherit the right defaults. (Formerly Tier 4; A.1 and A.7 are the ones **6.4** would convert from convention into a platform guarantee.)

### A.1 Make every provider action idempotent (the big one)
- The executor lease (`claim_workflow_node_run_executor`) prevents *concurrent* duplicate execution, but it **fail-opens on a transport error** and only protects while held. A worker that crashes *after* a side effect but *before* `broker.ack` will redeliver and re-execute. Any action with external side effects (charges, posts, writes) must dedupe on its own key — `workflow_node_run_id` is available in the request and is a natural idempotency key.

### A.2 A timeout stops *waiting*, not the work
- Provider code runs in `spawn_blocking` (`executor.rs:69`). On timeout the `CancellationToken` is cancelled, but a provider that never polls the token (or has no internal client timeout) keeps running on a blocking thread after the node is already marked `TimedOut`. Consequences: (a) Tokio blocking-pool thread leak (default 512 — exhaust it and the worker wedges), and (b) a "timed out" job still mutating the outside world. **Rule for new providers:** honor the cancellation token in any loop, and set an explicit client timeout ≤ `request.timeout_secs`.

### A.3 Don't model "wait for X" as a long-running task
- Each in-flight action pins one blocking thread *and* one concurrency permit for its whole duration. A task that sleeps/polls for an hour burns both the entire time. Use the `wait` / `gate` / `signal` node kinds, which park in the reducer with zero worker footprint. Tasks should be short, active work.

### A.4 Tune `max_concurrent_actions` per workload
- It is a single per-worker semaphore across *all* action types. One memory-heavy job × high concurrency can OOM the pod and starve light jobs queued behind it. For heterogeneous workloads, run separate worker deployments tuned per workload rather than one large pool.

### A.5 Consumer-group default differs by backend (horizontal-scaling gotcha)
- `broker_consumer_id` defaults to the shared group `runinator-workers` on **kafka**, but to a fresh per-worker `worker_id` UUID on **rabbitmq/http/tcp/in-memory** (`config.rs:90`). Whether N workers *compete* for actions or each receives *every* action depends on the backend's consumer-id→group mapping. **When scaling workers on a non-kafka backend, set `broker_consumer_id` explicitly to the same value across the fleet** so they compete instead of double-executing. Verify on the chosen backend before scaling past one worker.

### A.6 Secret resolution is on the job's critical path
- `resolve_secret_refs` runs per delivery. If the settings store is unavailable, the job publishes `Failed` and acks — it does *not* retry at the broker level. Jobs touching `secret://` refs should carry a node-level `retry` policy so a transient secret-store blip recovers. (**6.8** warns before the credential dies; this handles the store being briefly unreachable.)

### A.7 Result-publish failures redeliver the whole action
- If a job succeeds but `publish_status`/`flush` fails, the delivery is nacked and the entire action re-runs — looping back to A.1. Idempotency (A.1, and **6.4** as a platform feature) is the mitigation here too.

---

## Verification (per area, when implemented)

- **Backend:** `cargo fmt --all --check`, `cargo test -p <crate>`, then `cargo test --workspace` for shared-contract changes. Confirm the local stack still runs: `cargo run -p runinator-supervisor -- start|status|stop`.
- **WDL changes:** round-trip a `.wdl` through compile→decompile→format and confirm idempotency.
- **Frontend:** `npm test`, `npm run build`, `npm run lint` in `runinator-command-center`, plus the Tauri build path; verify keyboard/focus behavior and both themes manually.

---

## Note

This roadmap is a survey for prioritization — no single item is fully specified for execution yet. Pick one (e.g. "do 6.3") to get a detailed, file-by-file implementation plan.
