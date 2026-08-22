# Workflow VM Phase 14 Cutover Record

Phase 14 is complete. The destructive VM cutover migration is in place on every supported dialect,
and live execution uses continuation/effect records rather than the removed reducer, ready-node,
node-run, invocation-call, and legacy action-dispatch paths. This document records the gates that
made the cutover safe; it is not a rollout checklist for a fresh deployment.

## Implemented scope

1. ~~The effect protocol now spans in-memory, HTTP, TCP, WebSocket, Kafka, and RabbitMQ. Provider and
   infrastructure effects are executor-routed; Kafka and RabbitMQ use distinct physical
   topics/queues so the two hosts cannot race.~~
2. ~~`runinator-worker` executes provider `WorkflowEffectRequest::Action` commands with effect-scoped
   secret resolution, packaged-function staging, streaming output, cooperative cancellation,
   artifact relocation, idempotency, and a durable terminal/artifact result outbox.~~
3. ~~The engine infrastructure host handles timers, child/await-run, condition gates, run-scoped
   mutexes, and the automatic coordination kinds. Approval, input, signal, manual gate, event-wait,
   and collect receipts remain durably parked until their external producer settles that effect.~~
4. ~~VM cancellation, pause/resume/step, replay-from-step, interaction settlement, chunks, artifacts,
   ctl/MCP resources, and command-center run views now use continuation/effect identities. VM
   changes publish coarse workflow-run WebSocket events instead of node-run events.~~
5. ~~The engine starts only VM continuation/effect loops. Operational metrics, archiving, CLI/MCP
   resources, worker leases, and web handlers use the replacement records.~~

## Required implementation order

### ~~Gate A: complete and route the effect protocol~~

~~Status: implemented.~~

- ~~Preserve the effect executor class (`provider`, `infrastructure`) through every wire format.~~
- ~~Implement effect command/result channels in every broker backend and both wire transports.~~
- ~~Add cross-backend fixtures that prove dedupe, targeting, nack/redelivery, and result correlation.~~

### ~~Gate B: replace execution hosts~~

~~Status: implemented. Provider actions and infrastructure effects execute on their classified
hosts. External interactions use the durable effect receipt as their registration and are settled
through the effect endpoint; unsupported coordination kind names fail at the infrastructure host.~~

- ~~Teach the shared worker runtime to execute provider effects, including secret resolution,
  packaged-function staging, streaming chunks/artifacts, cancellation, and idempotent redelivery.~~
- ~~Add an engine infrastructure host for every non-provider effect variant.~~
- ~~Reject unsupported effect variants at the correct host instead of letting them bounce forever.~~

### ~~Gate C: make VM startup and settlement atomic~~

~~Status: implemented. API, function, console, manual-trigger, replay, cron/backfill, and pipeline
member starts all use the atomic VM bootstrap. Schedule claims receive precompiled modules and
freeze the module, root continuation, and first journal entry in the same transaction as the firing
slot and public run. Pipeline starts bind the member attempt in that transaction as well. The VM
host no longer depends on `RuntimeStore`: it reports terminal run IDs to the engine, which owns
pipeline advancement and reconciles terminal members missed by a crash.~~

- ~~Add one store transaction that creates the workflow run, compiled module, root continuation, and
  first journal entry.~~
- ~~Move API, trigger, backfill, console, replay, child-run, and pipeline-member starts to it.~~
- ~~Move pipeline advancement out of the legacy `RuntimeStore` orchestrator and onto VM terminal
  outcomes.~~

### ~~Gate D: finish operator writes~~

~~Status: implemented. Operator reads and writes use continuations and effects;
the command center projects graph rows from VM records without fetching node-run resources. Legacy
node-run endpoints and client methods remain only for the still-running reducer path and are removed
with that path in Gate E.~~

- ~~Move cancellation, pause/resume/step, replay, approval/input/signal/gate settlement, chunks, and
  artifacts to continuation/effect identities.~~
- ~~Remove node-run resources from the API client, command center, ctl, MCP, and WebSocket events.~~

### ~~Gate E: remove legacy runtime~~

~~Status: complete. The engine starts only VM continuation/effect loops; the reducer interpreter,
ready-node API, node-run API, and legacy action-dispatch API have been removed. Pipeline
orchestration lives with the engine and creates member runs through the VM bootstrap. The final
removal audit completed with legacy names retained only in historical migrations and cutover
documentation.~~

- ~~The destructive migration is `20260822000001_destructive_vm_cutover` on every supported
  dialect. It removes legacy execution tables only after all components have been deployed to the
  VM protocol.~~
- ~~Complete the removal audit, permitting legacy names only in historical migrations and cutover
  documentation.~~

### Gate F: caveats and operational boundaries

- ~~Live Kafka/RabbitMQ effect fixtures still need to run in broker-backed CI rather than only compile
  there.~~ CI now provisions the three VM-effect topics and runs provider/infrastructure routing,
  effect-result, acknowledgement, and redelivery fixtures against both live backends.
- ~~Approval, input, signal, manual-gate, event-wait, and collect effects intentionally remain parked
  until their external producer settles the durable receipt.~~ This is the asynchronous interaction
  contract, implemented through durable receipts and `POST /workflow_effects/{id}/settle`, not a
  cutover caveat.
- ~~The public event, signal, pause, resume, and cancellation entry points still had node-run
  fallback branches.~~ They now operate only on VM modules, continuations, and durable effects.
- ~~Retry-exhaustion notifications still inferred their source from node-run history.~~ They now
  derive the failed action, retry attempt, and source node from the durable effect receipt, its
  continuation, and the frozen VM module.
- ~~The engine still retained unlinked reducer-result and mutex-migration source modules.~~ The
  obsolete modules have been removed; the active engine links only the VM effect-result consumer.
- ~~Replay simulation still read node-run outputs.~~ It now reconstructs node outcomes from VM
  effect receipts, their continuations, and the frozen run module.
- ~~The canonical step and continue debugger commands could fall back to reducer cursors.~~ Both
  commands now operate solely on VM continuations, including through the canonical debug endpoint.
- ~~Reducer-only debugger controls still read and wrote node-run/cursor state.~~ Skip, rerun,
  breakpoints, run-to-cursor, and speculative cursor commands and routes are removed; the VM
  debugger exposes continuation-scoped Step and Continue only.
- ~~The command center still surfaced the retired reducer debugger controls.~~ Its debug toolbar
  and shortcuts now expose only VM Step and Continue.
- ~~Approval and manual-gate automation routes could still wake reducer node state.~~ Those routes
  and their repository transitions are removed; external interactions settle durable VM effects.
- ~~External notification delivery still serialized a synthetic node action into the removed action
  outbox.~~ Each delivery now owns a frozen provider-effect command, a leased notification outbox
  record, and delivery-row settlement; it shares worker provider execution without becoming
  workflow work.
- ~~Legacy terminology remains only where it is required to describe historical migrations and this
  cutover record.~~ Historical cutover plans are explicitly labeled; active VM paths use
  continuation/effect identity.

## Verification gate

~~The verification gate required a workflow exercising provider actions, timers, an external
interaction, child runs, concurrency, pipeline linkage, cancellation, restart/redelivery, chunks,
and artifacts to complete without creating or reading a node-run, ready-node, invocation-call,
action-dispatch, or legacy result row on every supported broker/database combination.~~

## Post-cutover audit (2026-08-22)

An audit of this record against the tree found several claims the code did not yet support. Each
was implemented rather than re-scoped; the list is kept here because the gaps say something about
where a cutover of this shape leaks.

- **Store code still targeted dropped tables.** `update_workflow_run_status` settled
  `workflow_ready_nodes` on every terminal transition, and `delete_workflow_run` deleted rows from
  tables the destructive migration had removed — so run deletion, replica listing, and the replica
  reaper all failed against a migrated database. ~3,200 lines of role/operation/mapper code writing
  to removed tables were deleted with them, and `DispatchStore` became `DeliveryStore` once no
  dispatch operation remained on it.
- **The executor lease had no VM counterpart.** Effects gained
  `current_executor_replica_id`/`last_executor_replica_id`, an `EffectResultKind::Claimed` message
  the worker publishes before executing, and release-on-settle; replica load and stale-replica
  reaping read those instead of the removed node-run columns.
- **The run mutex was never released.** `settle_workflow_vm_run` and `cancel_workflow_vm_run` now
  release a run's mutexes transactionally, and `claim_workflow_vm_mutex` carries the stale-holder
  safety net the reducer had.
- **`on_failure`/`on_timeout`/`on_reject` edges compiled away.** The compiler now lowers them into a
  guard frame, and the VM classifies a failure (`WorkflowFailureKind`) to pick the right edge.
- **The interrupt subsystem was inert.** Declared handlers now compile into the module, an
  interruptible node emits a `CheckInterrupt` safe point, and the VM raises, runs, and resolves a
  handler on its own continuation — preserving the rule that a handler can never fail the run.
- **The legacy `action`/`result` channels were still wired through every backend.** Nothing
  produced or consumed them, so `ActionCommand`, `WorkflowResultEvent`, `BrokerMessage`,
  `ResultMessage`, and their `Broker` methods are removed across in-memory, HTTP, TCP, WebSocket,
  Kafka, and RabbitMQ, along with the `--broker-action-topic`/`--broker-result-topic` flags
  (replaced by `--broker-effect-topic`, `--broker-infrastructure-effect-topic`, and
  `--broker-effect-result-topic`). The per-backend targeting, nack-redelivery, and relay-scoping
  tests were ported onto the effect channel rather than deleted, and the http broker's
  replica-scoping guard — previously only on the action receive — now also guards
  `POST /effects/receive`.

- **Node retry was lost entirely.** The pre-VM reducer implemented `@retry(...)` in
  `orchestration/transitions.rs` (`schedule_node_retry`: backoff, jitter, `retry_on` class
  filtering, attempt tracking). The cutover removed it and nothing replaced it — the policy was
  frozen into `WorkflowEffectRequest::Action.retry`, the worker `..`-ignored it, and nothing ever
  incremented `attempt` or re-published a command, so a node annotated `@retry(max_attempts: 5)`
  failed on its first attempt with no signal. Restored in `runinator-engine`: `effect_retry.rs`
  holds the pure policy, the effect-result consumer re-arms a retryable terminal through the new
  `retry_workflow_effect` instead of settling it, and `workflow_effect_dispatches.available_at`
  makes the re-dispatch invisible to the publisher until its backoff elapses. The parked
  continuation deliberately stays `Waiting` across attempts — a retry is invisible to the graph, so
  the only thing that moves is the dispatch.

  `InterruptSource::Retry` fires off the same path, and is the one source raised *without*
  suspending its thread: a thread parked on an effect is already stopped, and suspending it would
  stop its own retried effect from ever settling it. `start_workflow_interrupt_handler` exists for
  that case, and the handler id carries the attempt so each retry gets its own handler rather than
  the first one suppressing the rest.

- **The source map carried a field it could never fill.** `WorkflowSourceMapEntry.source_span` was
  always `None`, and could not be otherwise: `compile_workflow_module` compiles the JSON graph and
  has no text, the authoring `.rrx` is never persisted (a pack ships compiled definitions only), and
  the text a consumer *does* hold is regenerated by `decompile`, whose formatter pass reflows it.
  A frozen offset would therefore have indexed a document nobody has — worse than absent, because a
  highlight would land confidently on the wrong statement. The field is removed, and the capability
  moved to the one layer where text and node ids coexist: `decompile_with_spans` returns the source
  together with each node's byte range *within that same string*, derived by parsing and re-lowering
  the finished text (the round-trip contract is what makes the ids line up). It is served by
  `POST /rexrap/decompile/spans`, kept separate from `/rexrap/decompile` because that endpoint
  returns a bare JSON string its clients already read as one.

  While there, `graph_location` became a bisection instead of a linear scan — the compiler lays
  blocks out consecutively so the ranges are sorted and disjoint, and `source_map_is_ordered` plus a
  compiler test pin that invariant rather than leaving it implicit.

`edge_label` is now populated with the edge slot a landing block stands for, which is what
`GET …/workflow_vm/cursors` and the command center display.
