# Historical Workflow VM Cutover Plan (Implemented)

> Historical design record. The destructive VM cutover described here is complete; current runtime
> behavior is recorded in [Phase 14 readiness](workflow-vm-phase14-readiness.md).

## Summary

Replace the graph reducer, ready-node queue, node-run history, and invocation-call subsystem with
one compiled workflow VM:

```text
Workflow graph
    ↓ compile
Versioned WorkflowModule + source map
    ↓ execute
Persisted continuations
    ↓ yield
Durable effects
    ↓ settle
Resume continuation
```

The existing VM models, interpreter prototype, store trait, broker envelopes, migrations, and row
mappers are scaffolding only. They must be reviewed and integrated rather than treated as completed
subsystems.

The release is an intentional clean break:

- all services and workers upgrade together;
- new generic effect messages replace action/result messages;
- node-run APIs are removed;
- nonterminal runs are canceled at cutover;
- legacy execution history is discarded.

## Incremental Delivery Phases

These phases are intended to be implemented and reviewed independently. Each phase must leave the
workspace buildable; the new runtime remains deployment-disabled until the coordinated cutover.

### Phase 0: Baseline and change isolation

- Inventory the current dirty worktree and separate unrelated compute-language changes from the VM
  cutover.
- Restore a passing baseline for the crates touched by the cutover.
- Record the focused verification commands used by every later phase.

Exit gate: the baseline failures, if any, are documented and no VM change is confused with
pre-existing work.

### Phase 1: Finalize shared VM records

- Finalize versioned module, continuation, effect, journal, and source-map records.
- Remove tolerant corrupt-state fallbacks.
- Pin serialization and version-rejection behavior with tests.

Exit gate: `runinator-models` and `runinator-comm` tests pass with stable JSON fixtures.

### Phase 2: Finalize the bytecode instruction set

- Define the complete instruction vocabulary needed by every node kind.
- Define continuation frames for loops, try/finally, maps, forks, joins, interrupts, compensation,
  and debugging.
- Make unsupported opcodes and versions hard errors.

Exit gate: the instruction enum and continuation shape can represent every existing runtime state
without referring to `RunCursor` or node-run state.

### Phase 3: Build the graph compiler framework

- Add basic-block construction, labels, jump fixups, and mandatory source-map generation.
- Compile start/end/fail and ordinary linear transitions.
- Add compiler golden tests and module serialization round trips.

Exit gate: simple effect-free workflows compile and execute to the same terminal result as the
existing runtime.

### Phase 4: Compile compute and provider nodes

- Lower conditions, switches, transforms, assertions, outputs, and invocation programs.
- Lower action and packaged-function calls to typed effect instructions.
- Preserve retry, timeout, runner, tags, and idempotency policies in effect payloads.

Exit gate: linear workflows containing pure computation and provider calls compile exhaustively and
produce the expected effect requests.

### Phase 5: Compile parking and child-run nodes

- Lower timers, approvals, gates, signals, input, event waits, and child-run waits.
- Express every wait as one effect yield and one typed resume value.
- Remove node-specific polling decisions from the compiled path.

Exit gate: each parking node has yield/resume/restart tests using the host-free VM.

### Phase 6: Compile structured control flow

- Lower loops, re-entry, try/catch/finally, compensation, and interrupts.
- Verify frame copying and unwinding rules.
- Pin debugger node boundaries through the source map.

Exit gate: nested control-flow fixtures execute correctly in the fake VM host.

### Phase 7: Compile concurrency

- Lower parallel, join, race, map, barrier, collect, mutex, throttle, cooldown, and circuit breaker.
- Represent concurrency as multiple continuations with explicit join/race coordination state.
- Define deterministic ordering and loser cancellation.

Exit gate: concurrency and restart tests pass without using legacy cursor helpers.

### Phase 8: Complete the pure VM

- Implement every finalized opcode.
- Enforce instruction budgets, one outstanding effect per continuation, and stale-resume rejection.
- Implement fork, join arrival, race winner, interrupt handler, and terminal outcomes.

Exit gate: every `WorkflowNodeKind` compiles and runs against the in-memory host.

### Phase 9: Implement read-only SQL persistence

- Finish strict row mappers for modules, continuations, effects, and journal entries.
- Implement fetch/list/history operations on generic `SqlStore<B>`.
- Add SQLite, PostgreSQL, and MySQL mapper and query parity tests.

Exit gate: persisted records round-trip identically on all three dialects.

### Phase 10: Implement transactional VM writes

- Implement root creation, CAS continuation updates, yield transactions, effect settlement, forks,
  joins, races, cancellation, terminal settlement, and journal sequencing.
- Implement continuation leasing and effect-dispatch outbox claims.
- Compose `WorkflowVmStore` into `DatabaseImpl`.

Exit gate: crash-window and duplicate-delivery tests prove the transactional invariants on every
dialect.

### Phase 11: Add the VM runtime host

- Load compiled run modules and claimed continuations.
- Apply each `WorkflowVmStep` through `WorkflowVmStore`.
- Settle top-level workflow and pipeline state from terminal continuation outcomes.

Exit gate: the engine can execute complete workflows through the VM host using an in-memory broker.

### Phase 12: Replace broker and worker protocols

- Carry generic effect commands/results through every broker backend and transport.
- Update workers and the desktop agent to execute provider effects.
- Update the waker and infrastructure handlers for timer and control-plane effects.

Exit gate: no new-runtime message uses action-command, invocation-call, or node-run identity.

### Phase 13: Replace APIs, history, and graph cursors

- Add continuation, effect, journal, chunk, and artifact APIs.
- Update generated/manual API clients and command-center services.
- Render graph cursors from continuation instruction pointers and module source maps.
- Move debugger commands to continuation IDs.

Exit gate: the complete operator experience works without reading node-run records.

### Phase 14: Remove the legacy runtime

- Delete reducer handlers, ready-node scheduling, node-run/invocation persistence, action dispatch,
  compatibility aliases, and obsolete tests.
- Remove legacy schema objects in the destructive cutover migration.
- Complete a search-based audit for forbidden legacy runtime symbols.

Exit gate: the workspace contains one workflow execution path: compiled modules, continuations,
effects, and journal records.

### Phase 15: Coordinated cutover

- Build and test every service at the same VM/protocol version.
- Stop new starts, immediately cancel legacy nonterminal runs, and verify all legacy queues are
  drained.
- Back up the database, apply the destructive migration, deploy every component, and run the smoke
  suite.

Exit gate: production creates and resumes only VM-backed runs; rollback instructions and the
pre-cutover backup have been verified.

## Implementation Phases

### 1. Stabilize the foundation

- Separate the unrelated in-progress compute-expression changes from the VM-cutover diff and
  establish a passing workspace baseline.
- Review `WorkflowModule`, `WorkflowContinuation`, `WorkflowEffect`, journal records, and effect wire
  envelopes for serialization compatibility and complete lifecycle fields.
- Replace tolerant persistence fallbacks that manufacture continuations or effects with explicit
  corrupt-state errors.
- Add version fields to modules, continuations, effects, and wire messages; reject incompatible
  versions before execution.
- Define stable identifiers:
  - continuation ID identifies one branch;
  - `(continuation_id, effect_sequence)` identifies one logical effect;
  - `(effect_id, attempt)` identifies one delivery attempt;
  - `(workflow_run_id, journal_sequence)` orders run history.
- Keep one unsettled effect per continuation. Parallelism is represented exclusively by multiple
  continuations.

Completion gate: shared model, communication, store, and runtime crates pass serialization,
version-rejection, duplicate-yield, duplicate-resume, and fork-identity tests.

### 2. Compile every workflow graph into bytecode

- Add a graph compiler in `runinator-workflows`; graph definitions remain the public authoring
  format.
- Compile validated graphs into basic blocks with explicit instructions for:
  - graph-node entry;
  - value/condition evaluation;
  - transitions and conditional jumps;
  - durable effect yields;
  - fork, join, race, and map;
  - loop and re-entry frames;
  - try/catch/finally and compensation;
  - interrupts and resume;
  - return, fail, and output.
- Lower all 38 `WorkflowNodeKind` variants. Compilation must be exhaustive so adding a new kind fails
  to compile until its lowering exists.
- Reuse `NodeKindSpec` for edge/target metadata; do not introduce another node-kind registry.
- Embed a mandatory source map from instruction ranges to node ID, edge label, and optional REXRAP
  source span.
- Snapshot the compiled module into every new workflow run; never recompile an active run from the
  latest definition.
- Reject malformed graphs and unsupported module versions before creating a run.

Completion gate: every existing valid graph fixture compiles, every node kind has a lowering test,
and source-map tests prove graph cursor and breakpoint locations.

### 3. Complete VM continuation semantics

- Expand the VM from its prototype instruction set to the compiler's complete bytecode vocabulary.
- Make VM stepping host-free and deterministic:
  - input: module, continuation, optional settled effect result;
  - output: complete, fail, transition, yield, fork, or join arrival.
- Persist all execution state in the continuation:
  - instruction pointer and operand stack;
  - locals and invocation frames;
  - loop/try/compensation frames;
  - fork/join lineage;
  - map item/index;
  - interrupt/debug/speculative state;
  - next effect sequence.
- Implement fork semantics:
  - parent atomically becomes a join coordinator;
  - children receive unique IDs and copied scoped frames;
  - each child executes and yields independently.
- Implement join semantics with deterministic branch-result ordering.
- Implement race semantics so the first accepted winner atomically cancels loser effects and
  continuations.
- Implement interrupt handlers as separate continuations that suspend and later resume the
  interrupted continuation.
- Preserve graph debugging through source maps: node breakpoints, step-over boundaries, branch
  cursors, and speculative continuations.

Completion gate: fake-host tests cover linear flow, nested loops, map, parallel, race, joins,
try/finally, compensation, interrupts, debugger branches, cancellation, and restart after every VM
boundary.

### 4. Implement transactional persistence

- Implement `WorkflowVmStore` once on generic `SqlStore<B>` for SQLite, PostgreSQL, and MySQL.
- Add it to `DatabaseImpl` only after all required operations compile across dialects.
- Implement atomic transactions for:
  - root continuation plus initial journal entry;
  - continuation update plus journal entries;
  - continuation suspension plus effect receipt plus dispatch outbox;
  - fork parent plus all children plus join state;
  - effect settlement plus continuation-ready enqueue;
  - join arrival plus final join continuation;
  - race winner plus loser cancellation;
  - terminal continuation plus workflow-run settlement.
- Use continuation-version CAS for every step. A stale machine drive must reread and retry rather
  than overwrite newer state.
- Allocate journal sequence transactionally per run.
- Make duplicate effect insertion return the existing `(continuation, sequence)` receipt without
  adding another outbox record.
- Implement continuation leases and recovery of expired claims.
- Attribute chunks and artifacts to effect ID and continuation ID; remove node-run foreign keys.
- Add dialect-parity tests for every transaction and index-dependent deduplication rule.

Completion gate: persistence crash-window tests prove there is no state where a continuation waits
without an effect, an effect exists without its continuation, or a dispatch exists without its
canonical receipt.

### 5. Replace engine and broker execution paths

- Replace ready-node claims with runnable-continuation claims.
- Replace `WorkflowMachine`'s reducer adapter with a VM host that:
  - loads the run module and claimed continuation;
  - invokes the pure VM;
  - applies the returned step through `WorkflowVmStore`;
  - nudges effect and continuation publishers.
- Replace action-channel payloads with `EffectCommand`; replace workflow-result payloads with
  `EffectResult`.
- Update every broker backend and wire transport together: in-memory, HTTP, TCP, WebSocket, Kafka,
  and RabbitMQ.
- Route provider effects to workers; satisfy infrastructure effects in the owning service:
  - timer through waker;
  - approval/input/signal/event through web-service handlers;
  - child-run and coordination through engine-owned effect handlers.
- Update workers and desktop agent to execute only provider/package effect variants and return
  effect-keyed status, chunks, and artifacts.
- Move retry, timeout, idempotency, executor lease, cancellation, and dead-worker recovery from
  node/invocation records to effect records.
- Keep notifications and UI events as post-commit observers; they must never participate in VM
  decisions.

Completion gate: distributed tests prove duplicate broker delivery, stale attempts, worker death,
timeout, cancellation races, and web-service restart resume exactly once.

### 6. Replace history, HTTP APIs, and graph UI

- Replace node-run history endpoints with:
  - run continuations;
  - ordered journal entries;
  - effects and attempts;
  - effect chunks and artifacts.
- Update OpenAPI models and clients in the same change.
- Update command center run detail and graph cursor views:
  - source-map continuation IP to graph node;
  - render one cursor per live continuation;
  - show waiting effect and status on each cursor;
  - render fork ancestry, joins, races, map item/index, interrupts, and speculative branches.
- Change debugging commands to address continuation IDs rather than cursor/node-run IDs.
- Remove node-run service handlers, models, stores, frontend services, and compatibility views.
- Update CLI and command-center console commands together wherever run-history commands change.

Completion gate: API parity tests and frontend tests show complete run history, cursor rendering,
breakpoints, effect logs, chunks, artifacts, and cancellation without reading node-run records.

### 7. Remove the legacy runtime

- Remove:
  - reducer/interpreter node handlers;
  - `RunCursor` and `WorkflowExecutionState` execution authority;
  - ready-node records and queues;
  - workflow node runs/chunks/artifacts;
  - workflow invocations and invocation calls;
  - action dispatch records;
  - reducer aliases, metrics, comments, endpoints, and tests.
- Retain authoring, validation, simulation, and `WorkflowStatus` only where they remain part of public
  workflow/run semantics.
- Rename runtime metrics around VM drives, continuation queues, effect latency, effect retries, and
  journal failures.
- Add repository boundary tests preventing engine, workers, or web handlers from implementing
  workflow transitions outside the VM.

Completion gate: repository search finds no runtime dependency on reducer, ready-node, node-run,
invocation-call, or action-dispatch symbols except destructive migration notes.

## Testing and Verification

Run these gates in order:

1. Focused model/runtime/store tests.
2. Full tests for `runinator-models`, `runinator-workflows`, `runinator-runtime`, `runinator-store`,
   and `runinator-database`.
3. Broker-core and every concrete broker backend.
4. Engine, worker, waker, desktop agent, API, and web-service integration tests.
5. Command-center tests, build, and lint.
6. Cross-platform ctl/worker tests.
7. Rich workflow and end-to-end suites.
8. Full workspace build and tests.
9. Search-based removal audit for all legacy symbols.
10. Fresh database migration tests for SQLite, PostgreSQL, and MySQL.

Required failure scenarios:

- duplicate continuation drive;
- duplicate or late effect result;
- stale effect attempt;
- crash before and after every transactional write;
- expired continuation lease;
- worker loss after effect claim;
- timer redelivery;
- cancel racing effect completion;
- race winner concurrency;
- join recovery after restart;
- unsupported module/continuation version;
- corrupted continuation or journal payload;
- debugger pause/resume with multiple branches.

## Cutover

- Ship all new code behind a deployment-disabled runtime version gate while building it; do not
  support mixed execution within one run.
- Before activation:
  - stop new workflow and pipeline starts;
  - stop legacy engine, waker, and worker consumers;
  - immediately cancel every nonterminal legacy run;
  - verify no legacy ready rows or executing actions remain.
- Apply the destructive migration that drops legacy execution tables and data.
- Deploy the new web service, engine, waker, worker, desktop agent, API client, and command center as
  one coordinated release.
- Enable VM run creation only after every service reports the same protocol and VM version.
- Validate a smoke suite covering provider action, wait, approval, parallel/join, race, map, child
  workflow, interrupt, cancellation, and graph debugging.
- Rollback requires restoring the pre-cutover database backup and all legacy binaries together; the
  new schema is not backward-compatible.

## Definition of Done

The cutover is complete only when:

- every workflow node kind compiles and runs through bytecode;
- every external wait/action is a journaled durable effect;
- all concurrency uses persisted forked continuations;
- every run can recover after process loss from continuation state alone;
- workers and brokers use only the generic effect protocol;
- history and graph views read only continuations, effects, and journal entries;
- all node-run/reducer execution paths and schemas are removed;
- the full verification matrix passes on all supported databases and platforms.
