# REXRAP runtime model

REXRAP is the authoring language for a durable continuation virtual machine. It does not execute
source text directly, and live execution does not repeatedly reduce the JSON graph. The graph is an
intermediate authoring model that is validated, compiled to bytecode, and frozen for each run.

## From source to a running module

The compilation path has two distinct stages:

```text
.rrx source
  -> syntax + semantic analysis
  -> WorkflowDefinition (portable authoring graph)
  -> graph validation + VM compilation at run creation
  -> WorkflowModule + root WorkflowContinuation
```

The first stage belongs to the REXRAP crates. Parsing, name resolution, type checking, lowering,
and decompilation all operate on `WorkflowDefinition`. This is why packs can remain portable and
why the command center can edit the same model without embedding the execution engine.

The second stage belongs to `runinator-workflows`. `compile_workflow_module` converts the validated
graph into a versioned `WorkflowModule` containing instructions, graph source-map entries, and
compiled interrupt handlers. At run creation the database atomically stores:

- the public workflow run and its immutable definition snapshot;
- the compiled module;
- the root continuation; and
- the first append-only journal entry.

The run also keeps a resolved configuration snapshot. Editing a workflow or setting after a run
starts therefore cannot change the module or configuration observed by that in-flight run.

## What the VM executes

A `WorkflowContinuation` is one independently schedulable workflow fiber. It contains the module
version, instruction pointer, operand stack, locals, structured control-flow frames, next effect
sequence, outstanding effect identity, status, and compare-and-swap revision.

The host-free interpreter advances a runnable continuation through inline instructions until it
reaches a durable boundary. Its possible outcomes are:

- **yield** — request an external or asynchronous effect and wait for its settlement;
- **fork** — create child continuations for parallel, race, or map work;
- **join** — persist a branch arrival and combine control when the policy is satisfied;
- **complete or fail** — retire the continuation with a terminal result; or
- **interrupt** — suspend a continuation and start, or resolve, an interrupt-handler continuation.

Pure instructions—loads, stores, branches, jumps, expression evaluation, loop frames, and output
assembly—run inline. A per-drive instruction limit prevents a malformed module from monopolizing a
driver. The engine leases runnable continuations with bounded concurrency and gives each to the VM
host, which persists the returned boundary transactionally.

The graph remains visible through the module's source map. Operator APIs project an instruction
pointer back to a RexRap node and edge; node identity is not the execution record. Execution
history is the append-only workflow journal, while the durable unit of scheduling is the
continuation.

## Effects and resumption

Anything that cannot finish as deterministic inline work is a `WorkflowEffectRequest`. The VM
yields the request, and the host atomically parks the continuation and creates both a durable effect
receipt and its dispatch record. Effect identity is derived from the continuation and its monotonic
sequence, so a repeated drive or broker delivery cannot create a second logical operation.

Effects are routed by executor class:

- **Provider effects** contain an action's provider, function, resolved input, timeout, retry,
  labels, idempotency expression, and immutable packaged-function binding. A worker executes them
  and publishes effect results.
- **Infrastructure effects** cover timers, approvals, gates, signals, input, event waits, child or
  existing-run waits, mutex acquisition, and other engine-owned coordination. The engine's
  infrastructure host handles them; externally resolved interactions remain parked until their
  durable receipt is settled.

Timers are not in-process sleeps. The engine turns timer effects into broker wakes. `runinator-waker`
waits until the due instant and returns the prebuilt settlement through ingress, so an engine
restart does not lose the timer and the waker needs no workflow database or graph knowledge.

When an effect reaches a terminal state, settlement makes its waiting continuation eligible to run
again. The next driver pass loads the frozen module and receipt, supplies either the result or a
classified failure to the VM, and continues at the saved instruction pointer. `Succeeded`,
`Failed`, `Rejected`, `TimedOut`, and `Canceled` remain distinct so RexRap routes such as
`on timeout` and `on reject` do not collapse into a generic failure path.

## Retry, timeout, and idempotency

`@retry(...)` belongs to the yielded action effect, not to the continuation. A retryable failed or
timed-out attempt is re-armed by the engine's effect policy while the continuation remains waiting.
Only the effect attempt and delayed dispatch change. The VM sees one eventual settlement, so retry
does not re-enter graph control flow or duplicate a node visit.

Provider timeouts are enforced by the worker and protected by a later engine-armed deadline wake.
The worker normally reports the precise result first; the wake is a crash/no-matching-worker
backstop. Transactional settlement rejects a late deadline or result after another terminal result
has already won.

`@idempotent(key: ...)` is resolved for the effect and qualified by workflow. Successful provider
results can be replayed without invoking the provider again. The key is also passed to providers
that support upstream idempotency, covering the otherwise unknowable case where a worker dies after
an external side effect lands but before its result is published.

## Parallel, race, and map

Concurrency is represented by multiple continuations, not by one run row moving among several
nodes:

- `parallel` forks one child continuation per branch. A join records arrivals and releases one
  continuation after its `all`, `any`, or `first_success` policy is satisfied.
- `race` forks independent branches, records a deterministic winner, and cancels losing
  continuations and their live effects.
- `map` stores parent scheduling and item bindings in continuation frames and runs child item
  continuations up to the compiled concurrency limit. It does not create child workflow runs.

A run becomes terminal only when its last real continuation retires. Interrupt-handler and
speculative debugger continuations are excluded from that accounting, so they cannot accidentally
settle the run or satisfy a real join.

## Interrupts and debugging

Interrupt declarations are compiled into the frozen module. At an interruptible safe point, the VM
can suspend the current continuation and start a handler continuation in the same run. The handler
resumes, continues, restarts, or fails the interrupted position; it cannot independently fail the
whole run. Unsupported or unsafe interrupt attempts fail open and ordinary execution proceeds.

The debugger is continuation-scoped. Step and continue operate on the selected continuation, and
the source map turns its instruction pointer into the graph location shown to the operator. An
operator pause is distinct from waiting on an effect: settling an effect does not override an
explicit pause.

## Live execution versus simulation

`runinatorctl workflows test` and `POST /workflows/simulate` use the graph simulator. They evaluate
deterministic routing with mocked or replayed outputs and publish no provider effects. They are
useful for authoring assertions, branch coverage, and replaying a recorded path, but they do not
exercise continuation leasing, effect dispatch, broker redelivery, worker execution, timer wakes,
or transactional settlement.

Use a real workflow run when testing durability, concurrency, retries, timeouts, idempotency,
external interactions, or executor routing. Use simulation when testing the authored graph's pure
routing behavior.

## Component boundaries

- `runinator-rexrap-*` owns source syntax, semantic analysis, lowering, decompilation, and editor
  assistance.
- `runinator-workflows` validates authoring graphs and compiles them to `WorkflowModule` bytecode.
- `runinator-runtime` owns the pure continuation VM and its durable host boundary.
- `runinator-engine` leases continuations, dispatches and settles effects, applies retry/deadline
  policy, and coordinates infrastructure effects.
- `runinator-worker` executes provider effects.
- `runinator-waker` relays due timer settlements back to engine ingress.

No live path executes the removed reducer, ready-node queue, node-run state machine, or legacy
action-dispatch/result records.
