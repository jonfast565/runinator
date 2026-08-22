# Runinator Enhancement Roadmap

## Context

This is an advisory survey, not a single implementation task. It tracks the remaining gaps in **operational maturity**, **frontend polish/accessibility**, and **runtime/product completeness**, ordered by priority rather than by the survey that discovered them.

Item IDs are **stable** — an item keeps the number it was first filed under (5.3, 6.3, …) even as its priority moves, so "do 6.3" keeps meaning the same thing. The ordering lives in the priority bands below; the tier numbers are just names.

The guiding constraint from `AGENTS.md`: keep dependency direction services→shared-contracts, keep changes scoped to the crate that owns the behavior, and thread any shared-contract change through every broker backend, mapper, and config file.

**Last reprioritized:** 2026-08-22, after the architecture boundary survey (8.1–8.6).

---

## Priority at a glance

| # | Item | Band | Owning crates |
|---|------|------|---------------|
| 8.1 | Narrow persistence contracts | **P1** | store, engine, ws-* |
| 8.2 | Application-service boundary for HTTP | **P1** | engine, ws-* |
| 8.3 | Restore broker-only waker | **P1** | waker, service-bootstrap |
| 6.8 | Secret expiry warnings | **P1** | engine, utilities |
| 8.4 | Decouple UI event publication from engine | **P2** | comm, broker-core, engine, ws-core |
| 8.5 | Separate provider metadata from executors | **P2** | provider-catalog, pack, lsp, ctl, worker |
| 5.3 | Inbound webhook triggers | **P2** | ws, models |
| 6.5 | Cross-run analytics | **P2** | database, ws, command-center |
| 6.9 | Shareable run forms | **P2** | ws, command-center |
| 8.6 | Split the utilities catch-all | **P3** | utilities and its consumers |
| 5.6 | AI cost & token accounting | **P3** | provider-ai, comm/models, database |
| 5.2 | AI-assisted REXRAP authoring | **P3** | command-center, provider-ai |
| 5.7 | Pack environments + promotion | **P3** | ctl, ws, settings store |
| 7.1–7.8 | Loop / iteration semantics | **shipped 2026-08-13** | models, reducer, workflows, REXRAP, command-center |
| 6.6 | Action priority / fairness | **P4** | comm, broker (all backends), engine, worker |
| 6.7 | Retention & redaction policy | **P4** | archiver, database, models |
| 1.2 / 2.2 / 2.3 / 2.1 | Continuous quality track | parallel | varies |

---

## P1 — close correctness and boundary gaps

### 8.1 Narrow persistence contracts
- **Owning crates:** `runinator-store`, `runinator-engine`, `runinator-ws-middleware`, `runinator-ws-identity`, `runinator-ws-authoring`, `runinator-ws-runtime`.
- **Surveyed 2026-08-22:** `runinator-store` already owns the role-based contract, but the engine retains a production dependency on `runinator-database` solely through its re-exported `DatabaseImpl` surface. The engine contains 192 `T: DatabaseImpl` bounds; the HTTP handler crates contain another 279. `DatabaseImpl` composes 17 role traits, so callers that need a small storage slice inherit the complete persistence API.
- **Approach:** import contracts from `runinator-store` directly. Replace broad bounds with the owning role traits (`AuthStore + RbacStore` for authentication, for example); where one atomic use case truly spans roles, add a small named use-case trait such as `PipelineStore` rather than growing a generic repository bound. Keep SQL implementations and dialect mapping in `runinator-database`.
- **Migration:** first sever the engine's production dependency on `runinator-database`, then narrow the authentication middleware, then migrate handler domains one at a time. Preserve `DatabaseImpl` only for composition roots and genuine whole-store tasks such as schema initialization.
- **Why P1:** this turns the existing test seam into a real one, reduces the blast radius of schema work, and prevents database-specific dependencies from creeping into orchestration.

### 8.2 Application-service boundary for HTTP
- **Owning crates:** `runinator-engine`, `runinator-ws-core`, `runinator-ws-identity`, `runinator-ws-authoring`, `runinator-ws-runtime`.
- **Surveyed 2026-08-22:** the three WS domain crates are physically separated, but their library roots re-export `runinator-engine::repository` to preserve moved handler paths. Handlers consequently coordinate database calls, authorization checks, audit records, broker nudges, and UI events directly through generic `Extension<Arc<T>>` state.
- **Approach:** retain the route-domain split, but put explicit command/query services behind it: for example `IdentityAdmin`, `WorkflowAuthoring`, `RunOperations`, and `ReplicaRegistry`. Handlers should translate HTTP input and replies; services should own transactional orchestration, auditing, durable event emission, and persistence coordination.
- **Boundary note:** define each service in terms of the narrow store contracts from 8.1 and capability-focused ports, not `Arc<dyn DatabaseImpl>`. This creates one policy-bearing home for each operation and stops the current aliases becoming permanent public API.
- **Migration:** introduce one service behind an existing route module, migrate its handlers and tests, then delete the corresponding repository alias. Do not attempt a wholesale handler rewrite.

### 8.3 Restore the waker's broker-only boundary
- **Owning crates:** `runinator-waker`, `runinator-service-bootstrap`, `runinator-api`.
- **Surveyed 2026-08-22:** the waker relay library correctly consumes `wake` and publishes `ingress`, but `main.rs` also builds an API client, registers a `ReplicaKind::Waker`, heartbeats it, and refuses to start after bounded registration retries. It therefore has API URL/key configuration and cannot deliver a due wake while the control plane is unavailable.
- **Approach:** remove API-client setup, replica registration, heartbeat, API-key configuration, and their retry policy from the waker. Preserve liveness and telemetry; derive fleet visibility from broker-consumer metrics, or use a separately deployed optional observability reporter if registry visibility is required.
- **Verification:** add a process-level test that a waker can consume and settle a wake when the web service is unavailable. The existing in-memory relay tests remain the behavioral contract for the core loop.
- **Why P1:** a timer backend should fail only with its broker dependency. Reintroducing the web service into its critical path undermines the resilience separation that motivated the component.

### 6.8 Secret expiry warnings
- **Owning crates:** `runinator-engine` (settings store), `runinator-utilities` (credential store).
- **Problem:** The creds-sync pack copies credentials on a cron, but nothing warns **before** one expires — the first signal is a failed job.
- **Approach:** Optional expiry metadata on settings-store secrets and a scan that raises a notification ahead of expiry.
- Delivered through the shipped notification-policy layer (**6.1**, Appendix B); it is the cheapest proof that layer generalizes beyond run failures.

---

## P2 — extend reach and clarify cross-process contracts

### 8.4 Decouple UI event publication from the engine
- **Owning crates:** `runinator-comm`, `runinator-broker-core`, `runinator-engine`, `runinator-ws-core`, `runinator-ws`.
- **Surveyed 2026-08-22:** `runinator-ws-core::EventBus` embeds `runinator_engine::EnginePublisher`, so a crate intended for wire payloads, responses, and local websocket fan-out depends upward on the engine. `EnginePublisher` also combines two unrelated responsibilities: publishing `UiEvent`s to the broker and signaling in-process wake/agent publisher loops with `Notify`.
- **Approach:** extract a broker-backed UI-event publisher port shared by the engine and web service; keep the WS-local broadcast bridge in `runinator-ws-core`. Model wake and agent nudges separately as optional process-local signals owned by the embedded-engine composition root. The out-of-process engine remains correct through durable polling, with nudges only reducing latency.
- **Boundary note:** do not put this publisher in `runinator-comm`, because `runinator-broker-core` already depends on that contract crate. A small adapter crate depending on both contracts, or a broker-core extension that does not create a cycle, keeps dependency direction intact.
- **Result:** `runinator-ws-core` no longer needs an engine dependency, and standalone versus embedded engine deployment stops leaking into HTTP handler state.

### 8.5 Separate provider metadata from provider executors
- **Owning crates:** `runinator-provider-catalog`, `runinator-pack`, `runinator-lsp`, `runinator-ctl`, `runinator-worker`, `runinator-desktop-agent`.
- **Surveyed 2026-08-22:** `runinator-provider-catalog::metadata()` builds every `Box<dyn Provider>` and calls `metadata()` on it. Pack compilation, the LSP, and ctl use that function only for validation, but consequently link AI, database, sandbox, and integration-provider execution code.
- **Approach:** create a lightweight, static built-in metadata catalog for authoring and compilation, and retain a runtime registry that constructs executable providers for workers only. Add a parity test that compares the runtime registry's metadata with the static catalog so provider additions cannot drift.
- **Result:** compiler-facing crates depend on provider vocabulary rather than on executable integrations, improving build time, dependency clarity, and the safety boundary around provider code.

### 5.3 Inbound webhook *triggers* (start a run)
- **Owning crates:** `runinator-ws` (`handlers/webhook.rs`, trigger materialization), `runinator-models` (triggers).
- **Verified 2026-08-04:** still open. `WorkflowTriggerKind` remains exactly three variants — `Cron`, `Manual`, `Chained` (`runinator-models/src/workflows.rs:196-229`). `handlers/webhook.rs` only *wakes/signals an already-parked run*; there is no way to **start** one from an inbound event.
- **Approach:** Add a `trigger webhook "..."` header declaration that mints a signed inbound URL to start a new run, with a payload-mapping expression into workflow inputs. Reuse the existing pack-managed-trigger materialization path (`metadata.managed_by = "rexrap"`).
- **Boundary note:** a new trigger kind is a shared-contract change — thread through `runinator-models` triggers, ctl REXRAP compile, mappers, and the command-center trigger catalog.
- **Ranking note:** the highest-reach item for *new* work. The shipped per-workflow concurrency policy (**6.2**, Appendix B) makes it meaningfully safer — an unthrottled inbound webhook is precisely the source that makes such a policy mandatory.

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

## P3 — AI, lifecycle, and dependency cleanup

### 8.6 Split the `runinator-utilities` catch-all
- **Owning crates:** `runinator-utilities` and its 27 direct consumers.
- **Surveyed 2026-08-22:** one crate currently combines application paths, liveness, startup, logging/OpenTelemetry/resource telemetry, secret encryption and files, ZIP pack I/O, shell/FFI helpers, CSV/XLSX export, and GPU telemetry. Its dependencies are therefore a shared transitive build and conceptual surface for otherwise unrelated consumers.
- **Approach:** split by capability rather than by caller: observability, secrets, filesystem/runtime support, and pack-wire I/O are sensible first boundaries; move data export to the provider/reporting boundary. Preserve stable facades temporarily if migration churn would otherwise be high.
- **Boundary note:** separate crates provide real isolation in this workspace. Cargo feature flags alone will not, because workspace feature unification re-enables optional dependencies whenever another consumer selects them.
- **Migration:** start with the leaf `pack` and observability modules, whose import sets are already distinct, then move secrets as an auditable unit. Leave generic helpers only after every remaining module has a coherent owner.

### 5.6 AI cost & token accounting
- **Owning crates:** `runinator-provider-ai`, `runinator-models`/`runinator-comm` (result event), `runinator-database`.
- **Verified 2026-08-04:** still open — `runinator-provider-ai/src/claude_code.rs` captures no token or cost fields. There is no hook to attribute AI spend per node/run/workflow.
- **Approach:** Capture usage in the provider, thread it back on the `WorkflowResultEvent`, persist per node-run, roll up per run/workflow in the command center.
- **Boundary note:** adding usage to the result event is a `runinator-comm`/`runinator-models` contract change — thread through every broker backend, `mappers.rs`, and both DB backends.
- **Note:** lands more cleanly now that rate cards exist in `billing.rs`, and it pairs naturally with 6.5's aggregate query layer.

### 5.2 AI-assisted REXRAP authoring in the command center
- **Owning crates:** `runinator-command-center`, `runinator-provider-ai`.
- **Problem:** Authoring REXRAP/graphs is manual; new users face a blank canvas.
- **Approach:** Natural-language → REXRAP draft, generated against the live backend-driven node/edge/trigger **catalog metadata**. "Add a Slack notify after the approval fails" edits the draft graph in place. The catalog gives the model a constrained, validated tool surface so it emits well-formed graphs rather than free text. Draft stays the source of truth.
- **Unblocked:** the shipped `POST /workflows/simulate` (5.1) gives generated drafts a validation loop, which is what makes this tractable now.

### 5.7 Pack environments + promotion
- **Owning crates:** `runinator-ctl`, `runinator-ws` (packs), settings store.
- **Problem:** `semver.rs` exists but there is no dev→staging→prod lifecycle; a pack imports with one fixed set of config/secret bindings.
- **Approach:** Environment-scoped pack deployment with a diff/promote flow (`runinatorctl workflows promote <pack> staging→prod`) and per-environment config/secret binding, so the same compiled pack runs against different settings-store values per environment.
- **Unblocked** by 6.3 (shipped 2026-08-05) — `workflow_revisions` is the thing a promotion diffs against and rolls back to.

---

## P4 — when scale or customer data makes them urgent

### 6.6 Action priority / fairness
- **Owning crates:** `runinator-comm` (action contract), `runinator-broker` (all backends), `runinator-engine` (dispatch outbox), `runinator-worker`.
- **Problem:** `ActionCommand` has no priority — the only `priority` in the models is edge-selection ordering (`runinator-models/src/workflows.rs:884`). One `map` fan-out of 5,000 items can starve an interactive run behind it on a shared consumer group.
- **Approach:** A priority lane, or weighted fair queueing per org/workflow, pairing with the quota machinery already in `runinator-models/src/billing.rs`.
- **Boundary note:** the most invasive item in the roadmap — priority must be honored by **every** broker backend (in-memory/http/tcp/kafka/rabbitmq) and both wire transports, or ordering silently differs per deployment. Do not start this before **6.5** gives you the numbers to show queue starvation is real.

### 6.7 Retention & redaction policy
- **Owning crates:** `runinator-archiver`, `runinator-database`, `runinator-models`.
- **Problem:** `runinator-archiver` ages data out, but run parameters, outputs, and logs are stored raw. There is no field-level redaction and no per-org retention policy.
- **Approach:** Declarable sensitive fields redacted at persist time, plus per-org retention windows honored by the archiver. Becomes urgent the moment customer data lands in a run.

---

## P2 — loop / iteration semantics

Filed 2026-08-12 from a survey of the `loop` node across the runtime, type, language, and editor
layers and completed 2026-08-13. The notes below retain the original problem statements alongside
the shipped behavior.

### 7.1 Loop result accumulation — shipped 2026-08-13
- **Shipped:** loop frames snapshot resolved items and accumulate one body result per lap;
  `LoopOutput.results` exposes the ordered array and a bound `node r <- for …` collects it.
- **Owning crates:** `runinator-models`, `runinator-runtime`, `runinator-workflows`, `runinator-rexrap-codegen`.
- **Problem:** a `for` loop cannot collect per-iteration results. `MapOutput` carries
  `outputs: Vec<Value>` (`runinator-models/src/workflow_outputs.rs`); `LoopOutput` carries only
  `last`. A bound loop (`node r <- for …`) does not collect either — `control_value_expr`
  (`runinator-rexrap-codegen/src/lower/mod.rs`) returns `path_expr(["prev"])` for everything but
  `parallel`, so the synthetic collector node just re-emits the last body node's output.
- **Approach:** a `results: Vec<Value>` accumulator on `LoopFrame`, exposed as
  `steps.<loop>.output.results` typed `T[]`, with `node r <- for …` binding it.
- **Boundary note:** this is also the change that justifies moving the resolved item list into the
  frame the way `MapFrame` does. The frame deliberately does *not* hold `items` today, because it is
  rewritten every lap and parking a large array in `workflow_runs.state` per write buys nothing
  while items are re-resolved per drive anyway. An accumulator makes the frame item-resident, at
  which point resolving once on entry becomes the cheaper option too.

### 7.2 Round-trippable loop variable names — shipped 2026-08-13
- **Shipped:** loop and map binding names are preserved in `metadata.rexrap.control_vars` and restored
  by decompile, including nested shadowing.
- **Owning crates:** `runinator-rexrap-codegen`.
- **Problem:** `emit_loop` calls `self.fresh_var()` (`decompile/mod.rs`), which always yields
  `item`/`item2`/`item3`. The author's identifier only ever existed as a lowering scope key
  (`lower/blocks.rs`), so any graph-editor save — which regenerates the REXRAP pane via decompile —
  rewrites `for ticket in …` to `for item in …`. This is the single largest legibility loss in the
  loop surface: after one canvas round trip, `packs/sdlc/rexrap/sdlc-deploy.rrx` reads
  `for item in tickets.issues { … item.key … }`.
- **Approach:** record the name in the existing `metadata.rexrap` sidecar, exactly as `control_ids` and
  `type_hints` already are (written in `lower/mod.rs`, read back via `MetadataReader`,
  `decompile/metadata.rs`), and prefer it over `fresh_var()`. Same fix applies to `emit_map`.

### 7.3 REXRAP surface for loop position — shipped 2026-08-13
- **Shipped:** `for ticket, i in …` binds the zero-based runtime index; it formats, type-checks,
  lowers to `LoopOutput.index`, and round-trips through graph metadata.
- **Owning crates:** `runinator-rexrap-syntax`, `runinator-rexrap-sema`, `runinator-rexrap-codegen`.
- **Problem:** the loop variable binds only to `["item"]`. `index`, `count`, `has_next`, and `last`
  are emitted by the runtime and — since the type-contract fix — now type-check, but they have no
  surface syntax: a ref to them decompiles to a raw node id (`decompile/expr.rs` falls through to
  `self.dotted(node_id, output)`), and the id is generated (`"for_loop"`), so it is not nameable.
- **Approach:** an index binding (`for ticket, i in …`, grammar `for_stmt` in `rexrap.pest`) and/or a
  position accessor.

### 7.4 Loop-aware reference picker in the command center — shipped 2026-08-13
- **Shipped:** reverse-adjacency filtering offers upstream outputs only, loop-local fields are
  confined to the body region, output shapes come from the backend catalog, `interrupt` is a root,
  and sample contexts retain each node's output history.
- **Owning crates:** `runinator-command-center`.
- **Problem:** `src/core/utils/workflow-references.ts` iterates the node *array* and skips anything
  that is not `kind === "action"`, so `loop`, `map`, `transform`, `parallel`, `join`, `subflow`,
  `input`, and `collect` outputs are never offered and the loop item is unreachable in the graph
  editor. It also does no graph walk — the only positional rule is `node.id !== currentNodeId`, so
  *downstream* nodes are offered as if their output were available. No reverse adjacency exists
  anywhere in `src/`; the Tarjan SCC pass in `src/core/workflow/index.ts` is the only cycle-aware
  code and it feeds layout only.
- **Approach:** derive loop-body scope with the `interrupt-regions.ts` walk pattern (`nodeTargets`
  plus the `nodes.has(id)` cycle guard, already back-edge safe), build reverse adjacency for
  upstream-only filtering, and source non-action output types from the backend catalog.
- **Boundary note:** `runinator-rexrap-ide/src/completion.rs` already does correct loop-scope
  element-type inference for the *text* editor, including the shadowing rule. The canvas should
  reach parity with it, not reimplement it.
- **Also here:** `interrupt` is missing from `STATIC_ROOTS` despite being a real lowered root;
  `buildSampleContext` overwrites `steps[node_id]` per node run, so a loop body's "resolved against
  last run" preview only ever shows the final pass.

### 7.5 Loop edge slots and stale `parameters.target` cleanup — shipped 2026-08-13
- **Shipped:** the catalog names `transitions.next` as “Loop body” and `on_success` as “Loop exit”;
  layout, validation, and fixtures no longer read or emit `loop.parameters.target`.
- **Owning crates:** `runinator-workflows`, `runinator-command-center`.
- **Problem:** `loop` declares no `edge_slots` (`node_kinds/control_flow/loop.rs`), unlike `map`. The
  body edge therefore renders as "Next" and the exit as "Success" (`transitionLabel`,
  `src/core/workflow/index.ts`), and nothing marks the body region on the canvas. Three frontend
  sites still read a `parameters.target` that `loop` never emits — layout and validation in
  `src/core/workflow/index.ts`, plus `__tests__/catalog-fixtures.ts`, which means the frontend tests
  validate loop behavior against a shape production never produces.

### 7.6 Editor/compiler parity for loop-variable typing — shipped 2026-08-13
- **Shipped:** completion shows local types and uses union element inference, `for x: T in …`
  provides an explicit fallback, strict action results are closed, strictness controls iterable
  diagnostics, nested blocks retain `prev`, and completion includes a map snippet.
- **Owning crates:** `runinator-rexrap-ide`, `runinator-rexrap-syntax`, `runinator-rexrap-sema`.
- **Problem:** three parity gaps. (a) completion offers the loop var with `detail: "local"` and no
  type at all (`completion.rs`), so `tic<tab>` inside a loop offers `ticket` labelled only "local".
  (b) completion's loop inference uses `element_type()` without the `union_element_type()` fallback
  that sema's `check_iterable` and `higher_order_item_type` both use, so the editor is strictly
  weaker than the compiler on unions. (c) `ForStmt.var` is a bare `String` (`ast/statements.rs`) —
  there is no `for x: T in …` annotation, so when inference degrades to `Any` the only remedy is
  retyping the *source* node.
- **Related:** action `results_type()` builds an **open** struct (`runinator-models/src/providers.rs`),
  so a typo'd field types as `Any`, the loop over it never errors, and the whole body loses
  checking. `TypePolicy::Strict` (`runinator-rexrap-sema/src/options.rs`) gates action config, subflow,
  and array-literal homogeneity checks, but is **not** consulted in `check_iterable`.

### 7.7 Unify the two iteration bounds — shipped 2026-08-13
- **Shipped:** `WorkflowNode::iteration_limit` is the shared runtime contract over the two legacy
  wire locations; both reducer paths use it, and generic reentry visits are now cursor-scoped.
- **Owning crates:** `runinator-runtime`, `runinator-models`.
- **Problem:** `max_iterations` (loop-only, cursor-scoped) and `reentry.max_visits`
  (`runinator-models/src/workflows/nodes.rs`; generic, counted **run-wide** and including
  failed/canceled runs, exits via `on_exhausted`) are unrelated mechanisms for the same concern.
  `while`/`until` lower to the second, `for` to the first, so the two loop forms cannot be reasoned
  about uniformly.

### 7.8 Loops are excluded from dry-run simulation — shipped 2026-08-13
- **Shipped:** the dry-run walker models cached loop items, lap position, `last`, accumulated
  results, iteration caps, body routing, and exhaustion routing.
- **Owning crates:** `runinator-workflows`.
- **Problem:** `GraphRole::STEP.reentrant().not_simulatable()` (`node_kinds/control_flow/loop.rs`);
  `simulate.rs` reports loop/parallel/join/map/race/try/subflow as unsupported. A workflow whose
  interesting behavior is inside a loop cannot be dry-run at all, which blunts **5.1**.

### Smaller loop-adjacent defects (fold into 7.x as convenient)
- **Fixed 2026-08-13:** `while c limit none` is grammatically legal (`for_limit` in `rexrap.pest` accepts `"none"`) but
  `parse_while` does `first_inner(inner)?` and the inline `"none"` literal produces no child pair,
  so it fails with the opaque `RexRapError::lower("expected child node")`. The `for` path is fine
  because `limit_none` is a named atomic rule.
- `while`/`until` bind no iteration variable and expose no state.
- **Fixed 2026-08-13:** `prev` resets to `Any` at every block boundary (`runinator-rexrap-sema/src/sema/types.rs`), so
  `for x in prev.items` inside a nested block is untyped. `runinator-rexrap-ide/src/lib_tests.rs` has a
  test (`prev_has_no_fields_after_control_flow`) that *asserts* this gap.
- **Fixed 2026-08-13:** There is no `map` snippet in the completion `CONSTRUCTS` list, though `for`/`while`/`if`/`match`/
  `toggle`/`split`/`parallel`/`try` all have one.
- **Fixed 2026-08-13:** `TryOp` reads its body output through `latest_succeeded_output_excluding`
  (`runinator-runtime/src/orchestration/control_flow.rs`), a run-wide reverse scan with the same
  fan-out defect the loop path was fixed for: a `try` region inside a `parallel` can pick up a
  sibling branch's output. Left alone in the loop pass to avoid altering `try` semantics
  unannounced; the loop path now uses a cursor-scoped lookup and `try` is the last caller.

---

## Continuous quality track (run in parallel, low risk)

These are unbounded-effort quality work rather than discrete features. None blocks anything; each can absorb spare capacity.

### 1.2 Accessibility pass
- **Verified 2026-08-04:** ~46 ARIA attributes across 62 components (up from 29, still thin). Remaining: `aria-label`/`title` on icon buttons, focus trapping in modals (`WorkflowStepEditorModal.vue`), text fallback for color-only status badges, semantic heading hierarchy.

### 2.2 Frontend test gaps
- **Verified 2026-08-04:** **0 test files across 21 components** in `runinator-command-center/src/ui/components/workflow/` (canvas, node, step editor — the most complex, highest-LOC components). Core utilities and Pinia adapters remain well covered; presentation components are not.

### 2.3 / 3.3 Panic hardening — narrowed
- **Verified 2026-08-04:** `runinator-rexrap/src/parser.rs` is now **clean (0 `expect(` calls)** — that half is done. The remaining cluster is `runinator-ws/src/openapi.rs` (11 calls, e.g. `:114`, `:2407-2572`). These are document-generation paths over structures the file itself just built, so the residual risk is low — convert opportunistically per the error-dictionary convention rather than as a project.

### 2.1 Remaining backend test gaps
- **Verified 2026-08-04, partially closed:** `runinator-waker` now has tests (`src/tests.rs`, 5 cases, including the head-of-line `due_wake_is_not_blocked_by_a_not_yet_due_wake` regression) and metrics (`runinator_waker_wakes_received/driven/requeued_total`, `runinator_waker_drive_failures_total`, `runinator_waker_wake_lead_ms`). `runinator-supervisor` has one test file. Still at zero: `runinator-bootstrap` and `runinator-provider-aws`.
- **Residual:** no end-to-end `wake → ingress → drive` integration test crossing the crate boundary, and no alert wired to the `wake_lead_ms` histogram. Both are small and worth doing — but this is no longer the "highest residual risk" it was in the 2026-06-29 survey.

---

## Appendix A — Worker / job authoring pitfalls (reference, not a work queue)

These are footguns when creating new providers and workflow jobs, grounded in `runinator-worker/src/executor.rs` and `worker.rs`. They are **standing authoring guidance**, not scheduled work — they belong in a provider-authoring checklist so new jobs inherit the right defaults. (Formerly Tier 4. **6.4** has since converted the A.1/A.7 pair from pure convention into a platform guarantee for actions that declare `.idempotent(key: ...)` — the notes below still apply verbatim to actions that do not, and to the crash window no reservation can close.)

### A.1 Make every provider action idempotent (the big one)
- The executor lease (`claim_workflow_node_run_executor`) prevents *concurrent* duplicate execution, but it **fail-opens on a transport error** and only protects while held. A worker that crashes *after* a side effect but *before* `broker.ack` will redeliver and re-execute. Any action with external side effects (charges, posts, writes) must dedupe on its own key — `workflow_node_run_id` is available in the request and is a natural idempotency key. **Since 6.4**, a node can instead declare `.idempotent(key: <expr>)` and the platform reserves the key and replays a recorded success; the resolved key also arrives as `ProviderExecutionRequest.idempotency_key`, so a provider with native idempotency should forward it upstream rather than hand-rolling dedupe.

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
- If a job succeeds but `publish_status`/`flush` fails, the delivery is nacked and the entire action re-runs — looping back to A.1. **6.4 closes this for declared keys**: the result is recorded against the key *before* the publish, so the redelivery replays it instead of re-executing. Actions with no declared key still re-run, and A.1 remains the mitigation there.

---

## Appendix B — Retired (implemented)

Kept as a record of what the roadmap no longer covers. Item IDs stay stable, so a retired number is never reused.

### 7.10 Race inside a loop body — shipped 2026-08-13
- **Shipped:** every race visit now owns a fresh node run, and `race_winner_since` counts only
  contender runs created after that visit began. A previous lap's successful branch can no longer
  settle a later lap, including under the `all` policy.
- **Stragglers:** a winner retires its losing cursors. A delayed ready row remains addressed to the
  retired cursor and is discarded; cursor resolution no longer falls back by node id and binds that
  stale row to a same-named contender from the next lap.
- **Tests:** workflow predicate tests pin the per-visit history boundary. Reducer tests cover three
  repeated `first_success` laps, repeated `all` laps, and a first-lap loser arriving after the second
  lap has fanned out.

### 7.9 Fan-out inside a loop body — shipped 2026-08-13
- **Shipped:** a `parallel` inside a loop body now runs correctly on every lap. This took three
  fixes, each independently necessary — reverting any one of them fails
  `a_parallel_inside_a_loop_body_keeps_the_loop_position`
  (`runinator-runtime/src/orchestration/control_flow_tests.rs`) with a different symptom.
  1. **Branch cursors inherit their parent's frames.** `RunCursor::forked_from` replaces
     `RunCursor::forked` on the fan-out path, and `WorkflowRunState::fork_cursor` now takes the
     parent cursor id. The fan-out happens *inside* whatever loop or try region the parent stood in,
     and the forking cursor retires immediately, so that is the only moment its position can be
     carried. `speculative_from` had always done this; the real fan-out path had not. Reverting it:
     the loop restarts at index 0 on every lap and never terminates.
  2. **`parallel` re-fans-out on a fresh visit.** It read `ctx.latest` unconditionally, so *any*
     prior run meant "already fanned out". Now filtered through `is_reentry_stale`, the same
     freshness test twelve parking kinds already apply. Reverting it: only the first lap fans out.
  3. **`join` counts only the lap it is joining.** `join_satisfied` gained a `since` bound — the
     join's own last settled run — because branches keep their previous lap's `Succeeded` runs, so
     an unbounded read fired the join the moment one branch of the new lap arrived and let every
     branch through unjoined. This needed `ensure_node_run_for_visit` as well: recycling one node
     run across laps pins the bound to a fixed id, so the previous lap leaks back in from the third
     lap onward. Reverting it: branches over-run (4 runs across 3 laps).
- **Was:** the combination wedged. Before the loop-frame redesign a `parallel` in a loop spun
  against the inline step limit and reported a `Blocked` run; `packs/sdlc/rexrap/sdlc-deploy.rrx` nests
  its inner `for` inside a `parallel` branch, so it is on exactly this path.

### 6.3 Workflow revision history + diff + rollback — shipped 2026-08-05
- **Shipped:** a `workflow_revisions` table (all three dialects) holding an immutable capture of every accepted definition — definition + input schema + name + version, plus `source` (`ui`/`pack`/`api`/`duplicate`/`rollback`), actor, and an optional note. Three `DefinitionStore` operations back it (`insert_workflow_revision`, `fetch_workflow_revisions`, `fetch_workflow_revision`); the store assigns the per-workflow sequence number rather than the caller, under a unique `(workflow_id, revision)` index. Surfaced as `GET /workflows/{id}/revisions`, `GET /workflows/{id}/revisions/{n}`, and `POST /workflows/{id}/revisions/{n}/restore`; as `runinatorctl workflows revisions|revision|rollback`; and as a Revision history panel in the command center's Workflow Settings dialog that diffs any two revisions through the existing `JsonDiff.vue`.
- **The single chokepoint:** `repository::upsert_workflow` already funnelled every production write (the ws handler serving both `POST /workflows` and `PATCH /workflows/{id}`, pack import, and ctl via the API), so recording happens there and catches all of them. It gained a `&RevisionAuthor` parameter — deliberately a plain struct in `runinator-models` rather than `AuthContext`, mirroring `record_audit`, so the engine still does not depend on the web service's auth extraction.
- **Rollback semantics:** restoring is a *forward write*. The old definition is re-validated against today's provider catalog and saved as a new revision, so a definition referencing an action that has since been removed fails loudly instead of persisting something unrunnable, and the rollback itself stays in the history. It restores the graph only — the current org and enabled flag are preserved, since re-tenanting or re-enabling a deliberately disabled workflow would be a surprise. Gated on the workflow's `Permission::Edit` resource grant, not a new capability.
- **Two writes deliberately excluded:** `normalize_persisted_workflow` bypasses recording because it fires on a *read* and only canonicalizes a stored definition — a lazy migration, not an authored change. `duplicate_workflow` records a revision, but as the new row's own revision 1, since the copy diverges from the original from that point.
- **Unchanged saves record nothing:** the store compares the incoming name/version/definition/input-schema against the head revision and returns `None` on a match, so a pack applied on a cron does not bury real edits under identical rows. Recording is best-effort like the audit trail — failing a legitimate save because its history could not be written would be the worse outcome.
- **Was:** `workflows` was one mutable row per workflow with no history at all. In-flight runs were already insulated by `workflow_runs.workflow_snapshot`, but there was no way to see what changed, who changed it, or get back — and since `runinatorctl workflows apply` overwrites definitions wholesale, a bad apply was unrecoverable.
- **Tests:** 4 in `runinator-database/src/sqlite_tests/revisions.rs` (sequence assignment, dedupe including the rename-with-unchanged-graph case, lookup by number, removal with the workflow), a parity block in `mysql_tests.rs` covering the head read-back that is most likely to differ off sqlite, and 6 in `runinator-ws/src/tests/revisions.rs` (recording, pack attribution, unchanged re-apply, rollback-writes-forward, owner/enabled preservation, missing revision). 5 frontend tests in `workflow-revisions.test.ts`. Workspace: 95 test binaries green; command center 247 tests, build and lint clean.
- **Residual:** History is unbounded: there is no per-workflow cap and `workflow_revisions` is deliberately **not** registered with the archiver, since aging out the rollback target would defeat the feature. `RevisionSource::Ui` vs `Api` is inferred from principal kind (user token vs service key), which is a proxy — a human with a user token and curl records as `ui`; the distinction that matters, pack versus hand edit, is stamped by the import path itself. `workflow_runs` still has no `workflow_revision_id`, so "which revision did this run execute" is answerable only by comparing against `workflow_snapshot`. The command center diffs the JSON definition; diffing the decompiled REXRAP through the existing `POST /rexrap/decompile` would read closer to how workflows are actually authored.

### 3.4 DB migration parity tests — shipped 2026-08-05
- **Shipped:** one shared lifecycle body (`runinator-database/src/dialect_parity.rs`) that all three backends run — workflow upsert by insert/id/name, revision sequencing and dedupe, trigger upsert, the scheduler claim and its lease, result-event replay, idempotency keys, the action-dispatch outbox, notifications, settings, catalog, and automation records. `sqlite_lifecycle` runs it unconditionally; `postgres_full_lifecycle` and `mariadb_full_lifecycle` run the identical body against a live engine when `RUNINATOR_TEST_POSTGRES_URL` / `RUNINATOR_TEST_MYSQL_URL` are set, each provisioning and dropping its own throwaway database. `runinator-database/tests/docker-compose.yml` brings both engines up on ports that collide with neither a local install nor `runinator-provider-db`'s compose. Separately, `migration_parity_tests.rs` asserts the three migration directories carry the same version set with no database at all, so a migration written for one dialect and forgotten for another fails in a plain `cargo test` rather than on someone's first deploy.
- **Found two real bugs, both invisible to the previous suite:** the mysql `archive_marks` migration indexed a `TEXT` column with no prefix length, which MariaDB rejects — so **every mysql migration from 2026-06-21 onward had never applied**, and any mysql deployment was pinned to a schema eleven migrations behind. And `runinator-database` had no postgres coverage whatsoever. The mysql suite existed but was written before the archive work and had evidently not been run against a live engine since.
- **Was:** `mysql_tests.rs` held its own copy of a lifecycle test with no postgres counterpart, so the two server-backed dialects were covered by one unrun file and nothing respectively. Adding a store operation meant a sqlite test and, at best, remembering to hand-copy assertions into the mysql file.
- **Residual:** neither suite runs in CI — `.github/workflows/` has only `release-builds.yml`, so the live-engine runs still depend on a developer bringing docker up. The parity body covers the operations the dialects actually branch on, not all ~200 `DatabaseImpl` methods; a dialect-specific bug in an uncovered method still escapes. And `migration_parity_tests.rs` compares version *sets*, not the schemas the migrations produce — three files sharing a version number but declaring different columns would pass.

### 6.1 Failure alerting + SLA — shipped 2026-08-04
- **Shipped:** `notification_policies` + `notification_deliveries`, engine emission at the terminal transition plus a scanner for the duration events, the `notify on failure|retry_exhausted|sla|parked -> slack|email|app "<target>"` REXRAP header (pack-managed like triggers), a `notifications:manage` capability with CRUD endpoints, and the command-center Alert policies panel. Delivery reuses the action outbox via a new optional `ActionCommand.notification_delivery_id`, so alerts go out through the normal provider path and the engine holds no vendor client.
- **Was:** the `notifications` table, model, UI view, and API all existed, but the only writer was the inbound `POST` — a 3am cron failure stayed silent until somebody opened the Runs view, and there was no SLA concept in the models at all.
- **Residual:** slack/email delivery needs a `secret://slack/bot_token` (or an overriding `with { ... }`) in the settings store; a missing credential surfaces as a failed delivery row rather than a failed run.

### 6.2 Per-workflow concurrency & misfire policy (absorbed 5.4) — shipped 2026-08-04
- **Shipped:** `concurrency <n> on_conflict skip|queue|cancel_previous|allow` (REXRAP header → `definition.metadata.concurrency`, so it versions with the workflow) and `trigger cron "..." catchup fire_once|fire_all|skip [grace <d>] [max <n>]` (→ the trigger's own `configuration.catchup`). Both are evaluated inside the claim transaction in `claim_due_workflow_trigger_firings`, which now returns a `TriggerFiringBatch` carrying the created runs, the runs a `cancel_previous` settled (the loop publishes their worker cancels), and per-policy declined counters. `queue` deliberately leaves the slot due instead of creating a run that parks — the schedule *is* the queue, which is the shape that avoids the 2026-07-13 wake flood. Firing rows gained an `outcome` column so a slot that produced no run is explainable after the fact.
- **5.4 shipped with it:** a `freeze_windows` table (workflow-, org-, or platform-scoped) excluded from the claim in SQL — frozen triggers keep their due slot, so catch-up decides what replays when the window lifts — plus `POST /workflow_triggers/{id}/backfill` (idempotent against already-fired slots via the same firing row, with a dry run and a hard cap), `runinatorctl freeze` / `runinatorctl triggers backfill`, and a command-center **Schedules** tab. Both surfaces are gated on the new `schedules:manage` capability.
- **Was:** `run_trigger_loop` unconditionally claimed and started a run for every due firing — no `singleton`, no `max_concurrent_runs`, no catch-up policy. The workaround was a `mutex` node *inside* the workflow, which **starts** the run and parks it: exactly the shape that produced the 2026-07-13 wake flood (thousands of parked creds-sync runs, a 9k-message wake queue, head-of-line blocking in the waker).
- **Residual:** concurrency is enforced per *workflow*, counted inside the claim transaction. Two triggers of the same workflow claimed concurrently on two replicas can each pass the count, so the cap is best-effort under that specific race; the firing row remains the exact gate against double-firing one slot. Pipeline cron triggers honor freeze windows but have no concurrency policy of their own — their member workflows do. On mysql, a locking read also locks rows scanned in subqueries, so the freeze-window `NOT EXISTS` can make a concurrent replica `SKIP LOCKED` past a trigger; the effect is a one-tick (1s) delay, never a lost slot.

### 1.4 Bulk actions, loading/empty states, error recovery — shipped 2026-08-04
- **Shipped (command center only; no backend change):** three separable pieces.
  - **Bulk actions.** `core/utils/bulk.ts` (`runBulk` + `describeBulkResult`) fans one operation over a selection with bounded concurrency and *collects* failures instead of propagating the first — the backend has no batch endpoints, so partial failure is the normal outcome, not an edge case. `ui/composables/useBulkSelection.ts` holds keyed selection (survives re-sort, prunes rows that leave the filtered list, shift-click ranges), rendered by `SelectCheckbox.vue` + `BulkActionBar.vue`. Wired into **Workflows** (enable / disable / delete) and **Runs** (cancel / replay). `DataTable.vue` and `RunTable.vue` both gained an opt-in `selectable` column and stay presentational — they report intent, the composable owns the state.
  - **Error recovery.** `Toast` gained an optional `action`; `ToastHost.vue` renders it as a button, and a toast carrying one no longer auto-dismisses (retracting the affordance mid-read). `runOperation` gained an opt-in `retryable` that attaches Retry to the error toast — opt-in, not automatic, because a retry re-sends the request: safe for reads and idempotent writes, wrong for a create that may have partially applied. Enabled on the ten list-refresh paths. A partially-failed bulk action reports through `setError` with a **"Retry N failed"** action that re-runs only the failed items.
  - **Loading/empty states.** `TableSkeleton.vue` replaces the centered spinner on a table's *first* load, holding the pane height and column rhythm so rows arrive in place instead of shifting the layout; a background refresh still dims the existing rows. Events/Gates/Resources moved from bare `"No records."` text (Events had **no** empty or loading state at all) to `EmptyState` with a search-aware title and a line explaining what would populate the table.
- **Was:** no multi-select anywhere; no bulk enable/disable/delete/rerun; errors surfaced as a toast that told you what broke and then expired. (The survey's "only one skeleton/loading affordance exists" was overstated — `EmptyState`/`LoadingPanel`/`LoadingSpinner`/`useOperationLoading` already existed and `DataTable` already distinguished first-load from refresh. What was missing was a *content-shaped* placeholder and coverage on three views.)
- **Fixed in passing:** `DataTable.keyForRow` stringified the row key whenever `rowKey` was a field name, so a numeric id never matched the caller's own keys — latent for `selectedKey`, and a hard break for selection. It now preserves string/number as-is.
- **Tests:** `bulk.test.ts` (11 — concurrency ceiling, abort, partial/total failure), `useBulkSelection.test.ts` (11 — ranges, re-sort survival, pruning, anchor reset), and 8 added to `DataTable.test.ts` (skeleton vs. dimmed refresh, checkbox rendering/labelling, colspan). 242 pass; `npm run lint` and `npm run build` clean.
- **Residual:** bulk work is **client-side fan-out** over per-item endpoints — N requests, not one transaction, so a large selection is N round trips and there is no server-side atomicity. Bulk **replay** deliberately offers no retry affordance (a replay creates a run; retrying a failure that surfaced after creation would double-start it), and runs at concurrency 2 so a wide selection cannot stampede the action queue. Bulk actions are gated only by the per-item backend permission check, so a mixed selection reports per-row 403s as ordinary failures rather than hiding the button. `BulkActionBar`/`SelectCheckbox`/`TableSkeleton` have no component tests of their own — they are covered indirectly through `DataTable`. Selection is not yet wired into the other nine `DataTable` views (Schedules, Pipelines, Notifications, …); the machinery is generic, so each is a small opt-in.

### 3.2 Heartbeat-driven executor-lease invalidation — shipped 2026-08-04
- **Shipped:** `claim_workflow_node_run_executor` gained a `heartbeat_stale_before` cutoff, and the lease predicate a third arm — a slot now frees when unclaimed, when the claim ages past the action deadline (as before), **or** when the holding replica is no longer live (`status <> 'offline' AND last_heartbeat_at >= ?` against `replicas`, as a correlated `NOT EXISTS`). Failover after a pod crash is now bounded by the heartbeat window rather than by the job's timeout. Graceful shutdown frees the lease too, via the `offline` status, without waiting for a heartbeat to lapse.
- **Where the cutoff is decided:** `runinator-engine/src/repository/node_runs.rs`, from the existing `REPLICA_STALE_SECONDS` (30s = three missed heartbeats at the worker's 10s interval), now `pub` in `repository/replicas.rs`. Liveness is a platform policy — a worker knows its own deadline but not how long *another* replica may go quiet — so deriving it server-side keeps one definition shared with replica listing and action routing, and keeps it off the claim's wire payload. **The HTTP contract is unchanged**; `WorkflowNodeRunExecutorClaimRequest` did not grow a field.
- **Was:** `EXECUTOR_LEASE_GRACE_SECONDS = 60` (`runinator-worker/src/worker.rs:37`) made `timeout_seconds + 60s` the *only* path back to a crashed worker's node run. With long job timeouts a pod crash stranded that node for the full timeout window. The deadline arm is retained as the backstop for a holder that is still live but has lost the action.
- **Tests:** `executor_lease_frees_when_the_holder_stops_heartbeating` (`sqlite_tests.rs`) covers all three: lease holds while the holder heartbeats even past a long deadline, frees once the heartbeat lapses, and frees on graceful offline with a fresh heartbeat. The existing mutual-exclusion test was kept on the deadline arm alone.
- **Residual:** the `NOT EXISTS` correlates `replicas` from an `UPDATE` on `workflow_node_runs`. That is valid on all three dialects (the mysql target-table restriction applies to selecting *from* the updated table, not to correlating against it), but it is exercised only by the sqlite suite — the mysql/postgres tests are `#[ignore]`d without a live server. `runinator-runtime/src/orchestration/action.rs:21` still keeps its own private copy of `REPLICA_STALE_SECONDS`; the two agree at 30s but are not yet one constant.

### 6.4 Declarative idempotency on action nodes — shipped 2026-08-04
- **Shipped:** `.idempotent(key: <expr>)` on an action node (REXRAP modifier → `WorkflowAction.idempotency_key`, so it versions with the workflow). The reducer resolves the expression per dispatch against the same run context the action's arguments see, qualifies it as `workflow:<workflow_id>:<key>`, and stamps it on a new optional `ActionCommand.idempotency_key`. The worker reserves that key before invoking the provider, via new `claim`/`complete`/`release` operations over the previously manual `idempotency_keys` table (now carrying `owner_node_run_id` / `claimed_at` / `completed_at`, under the reserved `action` scope). The claim is a single upsert, so of two concurrent claimants exactly one acquires.
- **The guarantee, precisely:** once an execution *succeeds* under a key, any later delivery carrying it replays the recorded result instead of re-invoking. The result is recorded **before** the status publish, which is what fixes **A.7** — a failed publish/flush nacks the delivery, and the redelivery now replays rather than re-running the side effect. A concurrent claimant on a live reservation is dropped as a duplicate, the same way the executor lease drops one.
- **What it deliberately does not claim:** when a worker dies mid-invocation nothing can know whether the side effect landed, so the resolved key is also passed to the provider as `ProviderExecutionRequest.idempotency_key` for providers with native (stripe-style) idempotency. That is the honest boundary between what the platform can enforce and what **A.1** still asks of provider authors.
- **Failure semantics:** a failed attempt records nothing and *releases* the reservation, so the node's own `.retry(...)` — and every later run — is not blocked behind a reservation that no longer describes anything. A reservation whose holder died is takeable once it ages past the claimant's own action deadline. A key resolving to null or empty is treated as absent rather than as a shared empty key, since collapsing every run onto one key would silently skip real work.
- **Was:** the `idempotency_keys` table and `/idempotency_keys` endpoints existed but only as a manual store a workflow could call; the executor path never touched them, and A.1 asked every provider author to hand-roll dedupe as a convention.
- **Residual:** the claim/complete SQL is exercised only by the sqlite suite (`idempotency_claim_is_exclusive_and_replays_a_recorded_result`, `idempotency_reservation_frees_on_release_and_on_staleness`) — the mysql/postgres suites are `#[ignore]`d without a live server, and the mysql path is a two-statement read-back rather than a `RETURNING` upsert. The archive column list for `idempotency_keys` was left at the original five columns, so archived rows drop the reservation state. No command-center surface shows which keys are reserved or replayed; the `runinator_worker_actions_replayed_total` counter is the only signal.

### Earlier

- **Operational hardening** (former Tiers 1–2): tracing + `trace_id`, `/metrics`, DLQ/audit, retry backoff + jitter, rate limiting, `/health` + `/ready`, graceful shutdown, executor lease, per-node cancellation.
- **Runtime/language completeness:** poll/while, race-branch cancellation, plugin FFI cancellation, authorization phase 2.
- **1.1 Dark mode** — ✅ shipped. `:root[data-theme="dark"]` token set in `styles/base.css:101`, driven by the `displayPreferences` store through `ui/adapters/browser/theme.ts`, with a `system` mode that follows `prefers-color-scheme` live.
- **1.3 Live expression preview** — ✅ shipped. Backed by a server-side `POST /rexrap/evaluate` (`API_REXRAP_EVALUATE`) called through `core/services/expression.ts`; `ExpressionJsonEditor.vue` renders a debounced preview pane distinguishing a resolved result, an evaluation error, and a reference that is unresolved only because it is absent from the sample.
- **5.1 Workflow test harness + dry-run simulation** — ✅ shipped. `SimulationEnv` in `runinator-workflows` with a `MockEnv` (`testkit.rs`, driven by `tests` blocks in `.rrx` sources) and a `DbSimulationEnv` in `runinator-engine`. `simulate_workflow` reuses the graph transition evaluators and publishes no effects. `runinatorctl workflows test <pack>` runs suites offline; `POST /workflows/simulate` backs the command center's **Dry run** modal. Fan-out kinds (loop/parallel/join/map/race/try/subflow) report as unsupported rather than simulating incorrectly.
- **5.5 Run timeline / Gantt visualization** — ✅ shipped. `core/workflow/run-gantt.ts` (`buildGanttLayout`, unit-tested) + `ui/components/shared/RunGantt.vue`: proportional bars on a shared axis, dashed queued/parked segments, retry (`attempt`) badges, critical-path highlight, live count-up. No backend change.
- **Waker had zero tests** (former 3.1, the survey's "highest residual risk") — ✅ largely closed; see the continuous-track entry 2.1 for what remains.
- **`runinator-rexrap/src/parser.rs` panic cluster** (half of 2.3/3.3) — ✅ clean, 0 `expect(` calls.

---

## Verification (per area, when implemented)

- **Backend:** `cargo fmt --all --check`, `cargo test -p <crate>`, then `cargo test --workspace` for shared-contract changes. Confirm the local stack still runs: `cargo run -p runinator-supervisor -- start|status|stop`.
- **REXRAP changes:** round-trip an `.rrx` source through compile→decompile→format and confirm idempotency.
- **Frontend:** `npm test`, `npm run build`, `npm run lint` in `runinator-command-center`, plus the Tauri build path; verify keyboard/focus behavior and both themes manually.

---

## Note

This roadmap is a survey for prioritization — no single item is fully specified for execution yet. Pick one (e.g. "do 6.3") to get a detailed, file-by-file implementation plan.
