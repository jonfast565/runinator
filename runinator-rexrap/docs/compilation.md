# REXRAP compilation and round trips

RexRap is a structured source language over the portable `WorkflowDefinition` graph. Its compiler
infers convenient graph details, analyzes the source before lowering, and preserves enough
render-only metadata for faithful decompilation.

## Implicit versus explicit form

REXRAP infers the entry edge, sequential edges, node IDs, and several defaults. Every inferred part
can also be written explicitly, and both forms compile to the same graph. `decompile --explicit`
emits the canonical fully expanded source.

| Implicit (inferred) | Explicit form | Default |
|---|---|---|
| synthetic `start` → first statement | `start -> <id>` at the top of `do` | first statement |
| sequential happy-path edge | `ok -> <id>` for actions/subflows/approvals or `next -> <id>` elsewhere | next statement |
| auto node ID (`action_1`, `for_loop_2`, …) | `let x = …` or `@id("x")` | generated |
| step `@timeout(…)` | `@timeout(60s)` | 60 seconds |
| step `@retry(…)` | `@retry(1)` | one attempt |
| `while`/`until` cap | `limit 1000` | 1,000 |
| `for` cap / `map` fan-out | `limit none` / `concurrency none` | unbounded |
| approval kind | `type "generic"` | `generic` |
| `parallel` / `race` policy | `join all` / `winner first_success` | always emitted |
| control-block continuation | trailing `} next -> <cont>` | next statement |

`until c` is sugar for `while !c`. `subflow(..., detached: true)` selects fire-and-forget instead
of waiting. `limit none`, `concurrency none`, and an omitted cap have the same meaning.

This terse workflow:

```rexrap
workflow "Hello" v1 {
    do {
        let greeting = console.run(command: "echo hi")
    }
}
```

is equivalent to the fully explicit form:

```rexrap
workflow "Hello" v1 {
    start -> greeting

    do {
        @timeout(60s)
        @retry(1)
        let greeting = console.run(command: "echo hi")
            routes {
                on success {
                    continue end
                }
            }
    }
}
```

## Semantic analysis

`compile_str` runs semantic analysis after parsing and before lowering. Diagnostics anchor to
source spans (`RexRapError::Semantic { span, message }`). Expression and condition spans make a bad
operand, field, or reference more precise than blaming the whole statement.

The four analysis groups are:

- **Name/reference resolution** — every path head and transition target must resolve.
- **Scope correctness** — loop/map variables stay inside their body, and duplicate or reserved
  (`start`, `end`, `fail`) node IDs are rejected.
- **Type checking** — parameter paths, provider arguments/results, subflow signatures, iterable
  inputs, boolean contexts, comparisons, and conversions use the `RuninatorType` algebra. `prev`
  and `run` remain runtime-only and opaque.
- **Reachability** — an untargeted statement after a terminator or diverted happy path is a warning.

Errors block compilation. `compile_str_with_diagnostics` returns a definition plus warnings, while
`analyze_source` returns all diagnostics. `RexRapError::render(src)` and
`Diagnostic::render(src)` produce caret diagnostics:

```text
error: unknown field 'b' on 'input'
 --> line 4, column 34
  |
4 |     console.run(command: params.b)
  |                          ^^^^^^^
```

The same analysis runs when decompiled source is compiled again, keeping round trips semantically
valid.

## Decompiler scope

JSON → REXRAP recovers the structured feature set: linear sequences, `for`, `if/else`, `match`,
`toggle`, `split`, `parallel`/`join`, `race`, `map`, `try/catch/finally`, and all leaf node kinds.
Compile → decompile → compile tests pin that contract.

Arbitrary hand-written graphs with irreducible control flow, such as cross-block jumps that do not
match a structured shape, are not guaranteed to decompile.

See [CLI and pack workflow](tooling.md) for the commands and [Runtime model](runtime.md) for the
separate graph-to-VM compilation performed when a run starts.
