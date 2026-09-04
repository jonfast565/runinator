# AGENTS.md

Guidance for `runinator-runtime`, the continuation-driven workflow interpreter.

## Ownership

`runinator-runtime` executes validated, versioned workflow modules and commits durable boundaries
through `WorkflowVmHost` and `WorkflowVmStore`. It owns the host-free bytecode interpreter,
continuations, invocation frames, instruction behavior, debugging, and interrupts. Keep HTTP, SQLx,
concrete broker transports, service hosting, and repository orchestration out.

## Continuation and Transition Invariants

- One `WorkflowContinuation` is one independently schedulable thread. Parallel/race instructions
  create child continuations; loop, try, invocation, compensation, interrupt, and debug state live
  in its frames.
- The interpreter is pure and stops at `Yield`, `Fork`, `Joined`, terminal, or interrupt boundaries.
  The host persists the continuation, effects, dispatches, journal, and public-run projection in the
  owning `WorkflowVmStore` transaction.
- A graph-facing `WorkflowVmCursor` is derived from a continuation's instruction pointer and frozen
  module source map. Its node id is a rendering projection, not separate execution state.
- Continuation format and module bytecode have independent versions. Preserve compare-and-swap
  `revision` transitions and deterministic `(continuation_id, next_effect_sequence)` identity so a
  duplicate drive cannot enqueue a second logical effect.
- Creating a run atomically stores the public run, frozen module, root continuation, and first
  journal entry. `suspend_on_effect` atomically freezes the continuation, records the effect, and
  queues its dispatch.
- Settle the public run only after every continuation is terminal. Interrupt-handler continuations
  are excluded from the success/failure vote so a handler cannot decide the run's outcome.
- Debug state is a continuation frame. A breakpoint/step parks at a durable boundary; an effect
  settling while `operator_paused` is set must not silently make that continuation runnable.

## Interrupt Invariants

- `InterruptSource::ALL` is both the exhaustive source list and matching precedence. Add a source by
  updating the model list, detection, and grammar together.
- Arrival-matched sources are derived from the settled effect's immutable request/result. Requested
  external, timer, and orphan-signal interrupts are stored in `pending_interrupt`, consumed by the
  drive that accepts or refuses them, and never decided in the web service.
- Interrupts are fail-open: missing or inapplicable handlers leave normal driving unchanged.
  `retry` is raised by the effect host without suspending the continuation already waiting on that
  effect; it never arrives as a VM resume result.
- Raising a normal interrupt atomically freezes one continuation and creates its handler. Resolving
  atomically retires the handler and applies its `resume` outcome. A handler cannot be interrupted
  and cannot fail the public run.
- Interrupt-region shape and `GraphRole::handler_safe` are compile-time validation owned by
  `runinator-workflows`; the runtime consumes only the frozen module's safe points/handlers.

## Where to Start

- Host-free instruction interpreter: `src/workflow_vm.rs`.
- Durable host and boundary application: `src/workflow_vm_host.rs`.
- Continuation/module wire types: `../runinator-models/src/workflow_vm.rs`.
- Persistence transactions: `../runinator-store/src/roles/workflow_vm.rs`.
- Node and cross-boundary coverage: `src/workflow_vm_node_tests.rs` and existing interpreter
  regressions; add new tests according to the root test-file separation rule.

## Verification

```bash
cargo check -p runinator-runtime
cargo test -p runinator-runtime
```

Cover duplicate drives, fork/join/race, debugger boundaries, effect arrival classification, and
interrupt resolution. Host/store contract changes also require engine and database dialect tests.
