# runinator-rexrap

REXRAP is Runinator's authored workflow language. It provides readable control flow, typed values,
provider calls, reusable functions, settings, triggers, and pipeline declarations in unified
`.rrx` source files.

The crate is an author-time facade: syntax is parsed and analyzed, then lowered to the validated
`WorkflowDefinition` wire model. A workflow run does not execute that source or repeatedly reduce
the graph. At run creation, Runinator compiles the frozen definition into a versioned
`WorkflowModule`; the continuation VM executes that module and yields durable effects at external
boundaries.

## Documentation

- [Language reference](docs/language-reference.md) — syntax, expressions, types, control flow,
  attributes, functions, settings, pipelines, and compile/decompile behavior.
- [Compilation and round trips](docs/compilation.md) — inferred graph structure, semantic analysis,
  diagnostics, explicit output, and decompiler limits.
- [Runtime model](docs/runtime.md) — definition-to-bytecode compilation, continuations, effects,
  fork/join execution, retries, timers, interrupts, durability, and simulation.
- [CLI and pack workflow](docs/tooling.md) — checking, formatting, compiling, importing, and
  round-tripping `.rrx` sources.
- [Workflow authoring help](../docs/help/workflow-authoring.md) — operator-oriented import,
  scheduling, ingress, orchestration, and settings guidance.

The grammar in [`runinator-rexrap-syntax/src/rexrap.pest`](../runinator-rexrap-syntax/src/rexrap.pest)
is the canonical syntax spec.

## Small example

```rexrap
workflow "Deploy" v1 {
    params { environment: enum["staging", "production"] }

    do {
        @retry(3, backoff: 5s, on: failure)
        let release = deploy.apply(environment: params.environment)

        wait 30s
        health.verify(release_id: release.id)
    }
}
```

The source lowers to a graph definition. When a run starts, that graph becomes an immutable VM
module. Provider calls yield effects to workers; the timer yields an infrastructure effect to the
waker. Each settled effect makes its waiting continuation runnable again.
