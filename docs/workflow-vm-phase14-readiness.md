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
