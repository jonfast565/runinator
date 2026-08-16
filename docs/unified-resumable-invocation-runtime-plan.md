# Unified Resumable Invocation Runtime

## Summary

Replace the partially unified `WorkflowExpression + ComputeProgram + EvalEnv` stack with one versioned invocation IR and resumable VM.

The VM handles pure intrinsics, WDL `fn`, closures, provider actions, and packaged functions uniformly. Pure calls complete in-process; effectful calls yield a durable request to orchestration, which dispatches it through the existing broker/worker lifecycle and later resumes the persisted continuation.

Remove `compute`, `std.run`, and `std.exec` as concepts. WDL uses `do` blocks for multi-statement evaluation, while ordinary provider-call statements compile as single-call invocation nodes.

## Core Model and Public Interfaces

- Add versioned wire types in `runinator-models`:

  - `InvocationModule { version, entry, functions }`
  - `InvocationProgram` as stable stack-oriented bytecode.
  - `CallableTarget::{Intrinsic, Local, Provider, Packaged}`
  - `CallPolicy` carrying timeout, retry, runner, tags, and idempotency key.
  - `InvocationContinuation` containing call frames, instruction pointers, lexical locals, operand stacks, and the pending resume slot.
  - `InvocationEffect` containing the resolved target, arguments, policy, and packaged-function binding where applicable.
  - `InvocationStep::{Complete, Yield, Failed}`.

- Keep effect classification separate from value types:

  - `Pure`: guaranteed to finish inside the reducer.
  - `Durable`: may yield a provider/package effect.
  - `Unknown`: first-class function parameters whose effect depends on the runtime closure; reject these in declarative pure-only positions.

- Replace the ad hoc callable registries with one `CallableCatalog` assembled from intrinsic metadata, top-level WDL functions, provider metadata, and packaged-function catalog entries. It owns name resolution, argument binding, signatures, effect classification, and packaged-version pinning.

- Preserve lexical closure behavior: lambdas capture only their visible local environment, and both closures and recursive WDL functions use the same VM call frames. Retain the global and annotated recursion limits.

## Compiler and WDL Changes

- Replace:

  ```wdl
  node result <- compute {
      let x = ...
      return ...
  }
  ```

  with:

  ```wdl
  node result <- do {
      let x = ...
      return ...
  }
  ```

- A `do` block supports `let`, expression statements, `if/else`, and `return`. Falling through returns `null`. Remove `goto`; graph movement remains expressible only through workflow control-flow statements.

- Permit every cataloged callable in expressions and `do` blocks. Ordinary action syntax such as `github.deploy(...)` remains valid and compiles into a single-call invocation program when used as a workflow statement.

- Add an unambiguous policy postfix for calls inside expressions:

  ```wdl
  provider.call(...)
      with {
          timeout: 30s,
          retry: { max_attempts: 3, backoff: 2s, max: 60s, jitter: true, on: "any" },
          runner: "functions",
          idempotency_key: params.request_id
      }
  ```

  Existing statement-level `.timeout()`, `.retry()`, `.runner()`, and `.idempotent()` forms migrate to this representation. The enclosing invocation-node policy supplies defaults; call-site values override individual fields.

- Represent foreign code as the existing `std.code` callable using fenced inline source, for example:

  ````wdl
  node result <- std.code(
      language: "python",
      source: inline("python", ```
      def main(context):
          return {"value": 1}
      ```)
  )
  ````

- Compile top-level WDL functions into `InvocationModule.functions`; stop storing executable function bodies in `metadata.functions`. Keep render-only source metadata only where required for decompilation.

- Lower `transform` through the pure invocation compiler so it uses the same expression evaluator. Conditions, defaults, and parameter resolution use a pure-completion VM entrypoint that rejects any yielded effect.

- Effectful higher-order lambdas execute durably and sequentially. Explicit workflow `map … concurrency K` remains the mechanism for parallel durable work.

## Runtime and Persistence

- Implement the resumable VM in `runinator-compute`:

  - `step(module, continuation, input) -> InvocationStep`
  - `resume(module, continuation, effect_result) -> InvocationStep`
  - `evaluate_pure(program, context) -> Value`, which fails if execution yields.
  - Pure calls push their result directly; durable calls persist the continuation after the call instruction and yield an `InvocationEffect`.

- Add `WorkflowNodeKind::Invocation` and retire `Action` as the execution implementation. An invocation node owns one normal `WorkflowNodeRun`, which stays running across yielded calls.

- Add durable invocation records:

  - `workflow_invocations`: root node run, cursor, module version, continuation, status, final output/error, timestamps.
  - `workflow_invocation_calls`: invocation, sequence number, target/action, arguments, policy, attempt/status, result/error, deadline, and packaged binding.
  - Add optional `invocation_call_id` attribution to node-run chunks and artifacts.

- Add the necessary invocation operations to `ReducerStore`; keep database SQL implementations generic across SQLite, Postgres, and MySQL.

- Suspend atomically: persist the continuation, create the invocation-call record, and enqueue its action outbox row in one store operation. A transaction failure must leave none of the three visible.

- Extend `ActionCommand` and `WorkflowResultEvent` with optional `invocation_call_id`. The root `workflow_node_run_id` remains present for tracing and compatibility.

- Generalize dispatch ownership internally to `NodeRun | InvocationCall`:

  - Call attempts use their own timeout, retry, idempotency, executor claim, chunks, and artifacts.
  - A successful result settles the call and enqueues a drive for the original cursor.
  - The invocation handler pushes the result into the saved resume slot and continues stepping.
  - Exhausted failure or timeout fails the invocation node and follows its existing workflow transition.
  - Cancellation cancels the pending call and marks the invocation and root node run canceled.
  - Duplicate or late results are ignored using call ID plus attempt, matching current node-action behavior.

- Expose effectful std intrinsics as real `std` provider actions. `StdProvider` directly invokes the named intrinsic with resolved arguments; remove its program-interpreter `run/exec` entrypoints. `std.code` remains a durable action.

- Packaged targets retain compile-time `FunctionBinding` pinning, artifact staging, sandboxing, and `functions.invoke`; the VM unifies their call semantics without moving container execution into the evaluator.

- Surface invocation calls as children of the authored node in runtime/debug APIs so retries, logs, artifacts, and suspended call stacks remain inspectable without presenting synthetic graph nodes.

## Migration and Rollout

- Add an explicit invocation-IR migration marker and run the conversion in maintenance mode before normal engine loops or new-run APIs start.

- Preflight must:

  - Require zero queued, running, paused, or otherwise nonterminal workflow runs.
  - Produce a database backup/export.
  - Dry-run every current definition and report unsupported nodes or `$goto` without writing.
  - Abort the entire migration if any definition cannot convert.

- Convert current definitions transactionally:

  - Ordinary action nodes become one-call invocation nodes.
  - `std.run/std.exec` programs become invocation bytecode.
  - `std.code` becomes a one-call invocation.
  - `metadata.functions` becomes module function definitions.
  - Transform expressions are compiled through the invocation IR.
  - Definitions containing `$goto` are rejected for manual WDL control-flow replacement.

- Preserve historical workflow-revision bytes. Restoring a legacy revision runs the same converter and saves the result as a new current revision; legacy definitions are never executed directly.

- Update checked-in packs from `compute` to `do`. The decompiler emits only the new syntax.

- After the migration marker is committed, remove legacy runtime execution paths and reject newly submitted legacy compiled JSON with a clear “recompile with the current WDL compiler” error.

## Test Plan

- VM unit tests:

  - Pure intrinsic, local function, recursion, nested closure, closure capture, dynamic closure application, and return/fallthrough.
  - Yield/resume at every expression position, including nested calls and conditionals.
  - Serialization between yield and resume produces the same result as uninterrupted execution.
  - Unknown-effect callables are rejected in pure-only contexts.
  - Recursion limits and malformed bytecode fail deterministically.

- Compiler tests:

  - `do` parse/format/lower/decompile idempotence.
  - Unified name and named-argument resolution across all callable targets.
  - Provider and package calls inside WDL functions and lambdas.
  - Call-site policy inheritance and overrides.
  - Packaged bindings remain pinned after alias movement.
  - `compute` and `goto` receive migration-oriented diagnostics.

- Reducer/engine tests:

  - Pure invocation completes without broker dispatch.
  - Every effect produces exactly one durable call record and outbox entry.
  - Resume survives process restart between yield and result.
  - Retry, timeout, cancellation, late result, deduplication, executor death, and idempotent replay.
  - Sequential effectful lambda ordering and failure behavior.
  - Parallel workflow cursors maintain independent continuations.
  - Debug/speculative cursors shadow or arm invocation effects under the existing debugger rules.
  - Interrupt, try/catch, compensation, and workflow transitions observe the invocation node’s terminal result.

- Worker tests:

  - Direct dispatch of each effectful std intrinsic.
  - Provider and packaged-function calls preserve secrets, routing, sandboxing, logs, artifacts, and cancellation.
  - Broker round trips preserve invocation call identity and attempt fencing.

- Migration tests across SQLite, Postgres, and MySQL:

  - Dry-run is read-only.
  - Active runs and `$goto` block migration.
  - Successful conversion is atomic and idempotent.
  - Current definitions run equivalently after conversion.
  - Historical revisions remain byte-identical and convert when restored.

## Assumptions

- This is a breaking, coordinated runtime/compiler migration with maintenance downtime.
- Pure calls are not individually persisted; “each call durable” applies to calls classified as effects.
- A failed yielded call fails its enclosing invocation after its own retry policy is exhausted; workflow-level `try` and failure transitions remain the recovery mechanism.
- Provider/package implementations do not themselves suspend into nested workflow effects; unification applies at the WDL invocation boundary.
- No compatibility executor for legacy `std.run/std.exec` definitions remains after migration.
