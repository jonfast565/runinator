# AGENTS.md

Guidance for `runinator-provider-functions` and its engine-managed invocation adapter.

## Ownership

The provider executes published packaged functions. It advertises exactly one action, `invoke`;
the selected package/export comes from the action's `FunctionBinding`. Per-export provider actions
cannot be statically enumerated and would fail worker metadata validation.

## Invariants

- The provider makes no control-plane calls. The worker stages code and passes a local path;
  `InvocationRuntime` is the seam for host, desktop, or future Kubernetes-job execution.
- Reuse `runinator-sandbox` for container execution rather than embedding container policy.
- HTTP invocation has no special runtime path. Publishing creates one hidden, managed single-node
  adapter workflow per export, and the invocation endpoint starts that workflow run. Generate the
  adapter by compiling REXRAP, not by assembling graph JSON.
- Retry, timeout, cancellation, logs, artifacts, and tracing remain ordinary run behavior. The HTTP
  path resolves an alias at call time; authored workflows pin version/digest at compile time. That
  is the only intended semantic difference.
- Managed adapter workflows stay filtered from ordinary workflow listings.

## Where to Start

- Provider execution: `src/`.
- Adapter creation/invocation repository: `../runinator-engine/src/repository/function_adapters.rs`
  and the adjacent functions repository.
- Function binding/archive compilation: `../runinator-pack/AGENTS.md`.
- Container boundary: `../runinator-sandbox/AGENTS.md`.

## Verification

```bash
cargo check -p runinator-provider-functions
cargo test -p runinator-provider-functions
```

Adapter-workflow or invocation changes also require engine/web-service integration coverage.
