# runinator-wdl

WDL is a human-friendly workflow language that **transpiles to the existing runinator
JSON workflow model**. It is purely an author-time front end: `compile_str` lowers WDL
to a `WorkflowDefinition` (with a `WorkflowGraph` `definition` and a `RuninatorType`
`input_type`), and `decompile` reconstructs WDL from a definition. The scheduler, worker,
broker, and database are unchanged — they keep consuming the same graph.

The grammar in [`src/wdl.pest`](src/wdl.pest) is the canonical spec.

## Why

The JSON model is precise but the control flow is invisible: every edge is a
`{ "$node": "id" }`, every value is `{ "$ref": { "input": [...] } }`, every string is a
`{ "$concat": [...] }`, and conditions are nested objects. WDL makes the graph readable —
sequence implies edges, blocks expand into control nodes, references are dotted paths,
and conditions are infix.

## Example

```
workflow "Core Team SDLC Pipeline" v1 {
    input {
        jira: { base_url: string, email: string, token: string, jql: string }
    }

    let tickets = jira.search(
        base_url: input.jira.base_url,
        jql:      input.jira.jql,
    ).timeout(60s)

    for ticket in tickets.issues limit 50 {
        spawn "Ticket Work" reuse
            as "Ticket Work: ${ticket.key}"
            with { ticket, parent_workflow_run_id: run.run_id }
    }
}
```

## Language

**Statements imply edges.** Statements in sequence wire the forward edge (actions use
`on_success`, control-ish leaves use `next`). A synthetic `start`/`end`/`fail` are always
emitted.

**Arrows make transitions explicit.** `-> done` (single) or outcome arrows:

```
deploy()
    ok      -> verify
    fail    -> rollback
    timeout -> alert
```

`done` and `fail` are reserved targets (the terminal nodes).

**Chaining is configuration.** `.timeout(60s) .retry(3) .tags("ci","release") .mcp()
.reentry(5)` on actions.

**Node kinds**

| WDL | JSON kind |
|---|---|
| `provider.fn(args).mods` | action |
| `spawn`/`call "WF" reuse as ... with { }` | subflow (fire_and_forget / wait) |
| `wait 30s until "ready"` | wait |
| `emit "type" { data }` | emit |
| `approve "..." type "..." { meta }` | approval |
| `set name = ...` / `set meta { }` | config |
| `fail "msg"` | fail |
| `if / else if / else` | condition |
| `match subj { "x" -> {} when c -> {} else -> {} }` | switch |
| `for x in coll limit N { }` | loop |
| `map x in coll concurrency K { }` | map |
| `parallel { branch {} branch {} } join all` | parallel + join |
| `race winner first_success { branch {} }` | race |
| `try { } catch { } finally { }` | try |

**Expressions**: `input.x`, `prev.x`, `run.x`, `<binding>.x` (dotted refs); `"a ${x}"`
or `a ++ b` (`$concat`); `a ?? b` (`$coalesce`); `string(x)` / `json(x)`; object/array
literals.

**Conditions**: `== != > >= < <=`, `contains`, `in`, `starts_with`, `ends_with`,
`exists x`, `&&`, `||`, `!`.

**Input typing**: `{ a: string, b?: integer, c: string[], d: map<string>, e: A | B }`
maps to `RuninatorType`.

**Annotations**: `@id("explicit")` and `@skip` for round-trip stability.

**Typed bindings**: `let tickets: { issues: any[] } = jira.search(...)` annotates a step's
output type. The annotation is checked during semantic analysis, persisted in the graph
metadata, and re-emitted by the decompiler so it survives a round trip.

## Semantic analysis

`compile_str` runs a semantic pass on the AST — after parsing, before lowering — so
diagnostics anchor to source spans (`WdlError::Semantic { span, message }`). Spans are
**expression-granular**: `Expr` and `Cond` carry their own spans, so a bad operand, a missing
field, or an unknown reference is blamed precisely rather than the whole statement. (A dotted
path still shares one span, so `input.a.b` blames the path, not the `b` segment.) It performs
four checks:

- **Name/reference resolution** — every path head (`input`/`prev`/`run`, an in-scope
  loop/map variable, or a declared step label) and every transition target must resolve.
- **Scope correctness** — loop/map variables are only visible inside their body; duplicate
  or reserved (`start`/`end`/`fail`) node ids are rejected.
- **Type checking** — reuses the `RuninatorType` algebra: `input.*` field access is checked
  against the declared input type, `for`/`map` sources must be iterable, ordering
  comparisons need orderable operands, and `string(x)` rejects composite values. Action,
  subflow, `prev`, and `run` references are `any` (no provider metadata author-time), so
  references through them stay permissive.
- **Reachability** — statements that follow a terminator (`fail`, or a step whose happy-path
  arrow diverts the linear successor) and are not targeted by any transition are flagged.
  Reachability findings are **warnings**, not errors.

Errors block compilation; warnings are dropped by `compile_str` and surfaced by
`compile_str_with_diagnostics`, which returns the definition plus the warning list. The same
pass runs again when decompiled WDL is recompiled, so a round trip stays semantically valid.

`analyze_source` returns *all* diagnostics (errors and warnings) for a source, and both
`WdlError::render(src)` and `Diagnostic::render(src)` produce a rustc-style caret snippet:

```text
error: unknown field 'b' on 'input'
 --> line 4, column 34
  |
4 |     console.run(command: input.b)
  |                          ^^^^^^^
```

`runinatorctl wdl check` uses these to report every finding (parse errors keep pest's own
rich rendering).

## CLI

```
runinatorctl wdl compile  workflow.wdl [-o out.json]
runinatorctl wdl decompile workflow.json [-o out.wdl]
runinatorctl wdl format   workflow.wdl [-o out.wdl] [--check]
runinatorctl wdl check    workflow.wdl
```

The importer also accepts `.wdl` files directly alongside JSON packs.

## Decompiler scope

JSON → WDL recovers the full structured feature set — linear sequences, `for` loops,
`if/else`, `match`, `parallel`/`join`, `race`, `map`, `try/catch/finally`, and all leaf
node kinds — verified by compile → decompile → compile round-trip tests. Arbitrary
hand-written graphs with irreducible control flow (cross-block gotos that don't match a
structured shape) are not guaranteed to decompile.
