# runinator-wdl

WDL is a human-friendly workflow language that **transpiles to the existing runinator
JSON workflow model**. It is purely an author-time front end: `compile_str` lowers WDL
to a `WorkflowDefinition` (with a `WorkflowGraph` `definition` and a `RuninatorType`
`input_type`), and `decompile` reconstructs WDL from a definition. The web service, waker,
worker, broker, and database are unchanged — they keep consuming the same graph.

The grammar in [`src/wdl.pest`](src/wdl.pest) is the canonical spec.

## Why

The JSON model is precise but the control flow is invisible: every edge is a
`{ "$node": "id" }`, every value is `{ "$ref": { "params": [...] } }`, every string is a
`{ "$concat": [...] }`, and conditions are nested objects. WDL makes the graph readable —
sequence implies edges, blocks expand into control nodes, references are dotted paths,
and conditions are infix.

## Example

```
workflow "Core Team SDLC Pipeline" v1 {
    params {
        jira: { base_url: string, email: string, token: string, jql: string }
    }

    node tickets = jira.search(
        base_url: params.jira.base_url,
        jql:      params.jira.jql,
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
emitted. Every implicit part can also be written explicitly — see [Implicit vs explicit](#implicit-vs-explicit).

**Node leaves carry `node`.** The value-producing leaves — actions (`provider.fn(...)`),
subflows (`spawn`/`call`), and `compute` blocks — are declared with the `node` keyword, optionally
binding the output (`node x = provider.fn(...)`). These are the statements that otherwise read like
bare function calls, so `node` keeps them distinct from the `fn` functions and pure expressions
around them. Every other statement (`wait`, `output`, `approve`, control flow, …) stays bare. `let`
is no longer a workflow-level keyword; it now means only a pure local inside a `compute` block.

**Arrows make transitions explicit.** `-> done` (single) or outcome arrows:

```
node deploy()
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
| `node provider.fn(args).mods` | action |
| `node spawn`/`node call "WF" reuse as ... with { }` | subflow (fire_and_forget / wait) |
| `wait 30s until "ready"` | wait |
| `emit "type" { data }` (payload is any expression; parenthesize an event-less scalar: `emit (42)`) | emit |
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

**Expressions**: `params.x`, `prev.x`, `run.x`, `<binding>.x` (dotted refs); `"a ${x}"`
or `a ++ b` (`$concat`); `a ?? b` (`$coalesce`); `string(x)` / `json(x)`; arithmetic
(`+ - * / %`); standard-library calls (`std.strings.upper(x)`, `std.collections.len(xs)`, …) and
higher-order calls with lambdas (`std.collections.map(xs, x => x.id)`, `…filter`, `…reduce`);
object/array literals. See [Namespaces](#namespaces-and-imports) for the `std.<module>.<leaf>`
addressing and `import`.

**Access chaining**: any value-producing expression can be followed by `.key` / `.0` (dot) or
`[expr]` (bracket) access — `http_get(url).body`, `split(s, ",")[0]`, `(a ?? b).field`,
`items[params.idx]`. On a plain reference this just extends the path (`params.items[0].name` is one
`$ref`); on a call result, parenthesized expression, or object/array literal it lowers to the `at`
intrinsic (missing key → null, mirroring path access). A `[expr]` key may be dynamic.

**Method chaining (fluent / UFCS)**: a value can be followed by `.method(args)`, which desugars to a
function call with the receiver as the first argument — `recv.f(a)` ≡ `f(recv, a)`. Since every
standard-library intrinsic takes its subject first, pipelines read left-to-right:

```
params.xs.filter(x => x.gt(1)).map(x => x.mul(2))   // fluent receiver-first, no std. needed
std.strings.split(params.csv, ",").join("-")         // qualified prefix, then fluent
params.name.upper()                                  // == std.strings.upper(params.name)
std.exec.http_get(url).body.host                     // access + method chained
```

A bare `.field` (no parentheses) stays a field/path access even when it shares a name with a
function (`params.map.value` is a path), so the two never collide. The fluent/method form is the
namespace-free sugar — `recv.upper()` needs no `std.` because the receiver carries it; only the
**prefix** form requires qualification (`std.strings.upper(x)`). Method calls decompile to the
canonical qualified form.

One expression grammar serves every position — action arguments, conditions, `${…}`
interpolation, and `compute` lines — so a call or lambda is legal anywhere an expression is.
**Purity, not the grammar, decides where work runs:** a pure expression folds eagerly in the
reducer, while an *effectful* call (`http_get`, `http_post`, `now`, `uuid`, `env`) is a semantic
error outside a `compute` block, since it must dispatch to a worker. A `compute { }` block is the
only place effectful calls and multi-statement programs (`let` / `return` / `goto` / `if`) live;
it lowers to `std.run` when pure and `std.exec` when effectful.

### Namespaces and imports

Names are qualified, not flat. There are three namespace roots:

- **`std`** — the builtin standard library, organized into modules: `std.math`, `std.strings`,
  `std.collections`, `std.objects`, `std.encoding`, `std.logic`, `std.dates`, `std.regex`, and the
  effectful `std.exec`. A **prefix** intrinsic call must be fully qualified — `std.math.add(a, b)`,
  not `add(a, b)` — though the fluent/method form (`a.add(b)`) needs no prefix. The `std.` prefix is
  surface-only: the compiled graph and runtime dispatch use the bare leaf, so already-stored
  workflows are unaffected.
- **providers** — a provider action's name may be a dotted path; the trailing segment is the
  function and the leading segments are the provider (`github.repos.create_pr(...)` →
  provider `github.repos`, function `create_pr`).
- **workflow namespace** — an optional `namespace <path>` header qualifies a workflow's identity so
  a subflow target can name a workflow in another pack (`call "core_sdlc.ticket_work"`).

`import` opens a namespace into local scope (header declaration, pure surface sugar — the compiled
graph always holds fully-resolved names):

```
namespace core_sdlc                 // this workflow's identity namespace
import std                          // the whole stdlib, callable bare: add(a, b), upper(x)
import std.strings                  // just the strings module, callable bare: upper(x)
import std.collections as col       // aliased: col.map(xs, f)
```

Resolution order for an unqualified call is: file-local user `fn` → imported names → otherwise a
builtin intrinsic must be qualified or imported (a bare prefix intrinsic call is a semantic error
that names the module to use). The decompiler always emits the canonical `std.<module>.<leaf>` form.

**Source text includes**: `file("scripts/job.py")` reads a UTF-8 text file at compile time,
relative to the `.wdl` file's directory, and lowers to a normal string value. Paths must be
relative and cannot contain `..`, so pack compilation stays deterministic and local to the source
tree.

**Directory listings**: `dir("scripts")` lists the files under a directory at compile time and
lowers to an array of forward-slash relative paths (sorted, e.g. `["job.py", "lib/util.py"]`). It
lists the top level only by default; pass a boolean to recurse (`dir("scripts", true)`) and an
optional trailing integer to cap the recursion depth (`dir("scripts", true, 2)`). The same
relative-path safety rules as `file()` apply, and the listed files are bundled with the pack source.

For embedded source, use a fenced inline block:

````
node run = console.run(command: inline("python", ```
print("hello")
```))
````

Both forms are author-time conveniences; the runtime receives the compiled string value and does
not read files.

**Conditions**: `== != > >= < <=`, `contains`, `in`, `starts_with`, `ends_with`,
`exists x`, `&&`, `||`, `!`.

**Parameter typing**: `{ a: string, b?: integer, c: string[], d: map<string>, e: A | B }`
maps to `RuninatorType`. Open structs use `...: type`, e.g. `{ known: string, ...: any }`.

**Parameter defaults**: a top-level parameter field may carry a default — `name: type = expr` — used
when the field is omitted at run start:

```
params {
    poll_interval: integer = 30
    base_url:      string  = config.api.base_url
    label:         string  = "run-" ++ string(params.poll_interval)
    token:         string  = secret.api.token
}
```

The default is an ordinary expression (a literal, object/array, or a `config.*` / `run.*` /
`secret.*` / sibling `params.*` reference; `prev` and step outputs are rejected since defaults run
before any step). A defaulted field is implicitly optional. Defaults are evaluated lazily against
the run context (after `config` resolves, with secrets left as `secret://` strings), filling only
omitted fields and never overwriting a supplied value; one default may read another. They survive
compile → decompile → recompile and are stored on the field in `input_type`.

**Version**: the optional `v` suffix in the workflow header is a semantic version,
`v<major>[.<minor>[.<patch>]]` (e.g. `v1`, `v1.2`, `v1.2.3`). Missing components default to
zero, so `v1` lowers to `1.0.0`. The decompiler always emits the canonical full form.

**Triggers**: a workflow header may declare cron schedules that fire runs of the workflow:

```
workflow "Nightly" v1 {
    trigger cron "0 9 * * *"
    trigger cron "*/5 * * * *" with { source: "cron" }
    trigger cron "0 0 * * *" disabled blackout "2026-01-01T00:00:00Z" to "2026-01-02T00:00:00Z"
    ...
}
```

The cron expression must be a string literal; the optional `with { … }` object is the run parameters.
`disabled` creates the trigger disabled, and `blackout` carries RFC3339 start/end timestamps.
Triggers belong to their workflow, so they are carried inside the compiled definition
(`definition.metadata.triggers`) and **materialized at import**: the web service replaces that
workflow's pack-managed (`managed_by: wdl`) cron triggers with the declared set (idempotent on
re-apply; manually-added triggers are left alone). This works for directory packs, not just `.wdlp`
manifests, and they round-trip through decompile.

**Annotations**: `@id("explicit")`, `@skip`, `@lock`, and `@timeout(300s)` for round-trip
stability and node-level orchestration metadata. Action `.timeout(...)` remains the provider
command timeout; `@timeout(...)` maps to the workflow node timeout.

**Typed bindings**: `node tickets: { issues: any[] } = jira.search(...)` annotates a step's
output type. The annotation is checked during semantic analysis, persisted in the graph
metadata, and re-emitted by the decompiler so it survives a round trip.

**Argument aliases**: shared arguments can be named once in the workflow header and spread with
`...name`, so a connection's `base_url`/`email`/`token` are written once instead of on every call:

```
workflow "Ticket Work" v1 {
    alias jira_conn = { base_url: config.jira.base_url, email: config.jira.email, token: secret.jira.token }

    node t = jira.transition(...jira_conn, key: params.ticket.key, transition_id: config.transitions.done)
}
```

A `...name` spread works anywhere an object's entries are written: action arguments, object
literals `{ ... }`, subflow `with { ... }`, and `approve "..." { ... }` metadata — including nested
objects. Entries apply in source order with **positional last-wins** (like JS spread): a later
`key: value` overrides an earlier spread of the same key, and a later spread overrides an earlier
entry. Aliases may compose other aliases (`alias full = { ...base, token: secret.x }`); reference
cycles are a compile error.

Aliases are surface sugar: spreads are expanded **before** semantic analysis and runtime
execution, so the runtime graph never sees an alias — the aliased and fully-expanded forms run
identically. To keep round trips faithful, lowering also records the authored alias declarations
and each call's spread layout in a render-only `wdl` metadata sidecar (alongside declared types);
both `format` and `decompile` re-emit `alias`/`...name` from it, so aliased source compiles,
decompiles, and recompiles back to the same source — including composition and positional
overrides. Graphs authored without this sidecar (e.g. hand-written JSON, or compiled before the
sidecar existed) decompile to the equivalent fully-expanded form. A `secret.*` value spread through
an alias is still a whole argument value, so the "no secret mid-string" rule holds.

## Implicit vs explicit

WDL hides a lot for brevity: the entry edge, sequential edges, node ids, and several defaults
are inferred. Every one of them can be written explicitly instead, and the two forms compile to
the **same graph** — implicit is sugar, nothing is required. `decompile --explicit` emits the
canonical fully-expanded source so a reader never has to guess how a workflow is wired.

| Implicit (inferred) | Explicit form | Default |
|---|---|---|
| synthetic `start` → first statement | `start -> <id>` (top of body) | first statement |
| sequential happy-path edge | `ok -> <id>` (action/subflow/approval) or `next -> <id>` (wait/emit/config, control blocks) | next statement |
| auto node id (`action_1`, `for_loop_2`…) | `node x = …` (action/subflow/compute) or `@id("x") …` (any statement) | generated |
| action `.timeout(…)` | `.timeout(60s)` | 60s |
| action `.retry(…)` | `.retry(1)` | 1 attempt |
| `while`/`until` cap | `limit 1000` | 1000 |
| `for` cap / `map` fan-out | `limit none` / `concurrency none` | unbounded |
| approval kind | `type "generic"` | `generic` |
| `parallel` / `race` policy | `join all` / `winner first_success` | always shown |
| control-block continuation | trailing `} next -> <cont>` | next statement |

`until c` is sugar for `while !c`, and `spawn`/`call` pick fire-and-forget vs wait — these stay as
readable verbs; the canonical form normalizes to `while` and the matching verb. `limit none` /
`concurrency none` and an omitted cap are identical; the explicit form surfaces `none`.

So this terse workflow:

```
workflow "Hello" v1 {
    node greeting = console.run(command: "echo hi")
}
```

is exactly this fully-explicit one (`decompile --explicit`):

```
workflow "Hello" v1 {
    start -> greeting
    node greeting = console.run(command: "echo hi").timeout(60s).retry(1)
        ok -> done
}
```

## Semantic analysis

`compile_str` runs a semantic pass on the AST — after parsing, before lowering — so
diagnostics anchor to source spans (`WdlError::Semantic { span, message }`). Spans are
**expression-granular**: `Expr` and `Cond` carry their own spans, so a bad operand, a missing
field, or an unknown reference is blamed precisely rather than the whole statement. (A dotted
path still shares one span, so `params.a.b` blames the path, not the `b` segment.) It performs
four checks:

- **Name/reference resolution** — every path head (`input`/`prev`/`run`, an in-scope
  loop/map variable, or a declared step label) and every transition target must resolve.
- **Scope correctness** — loop/map variables are only visible inside their body; duplicate
  or reserved (`start`/`end`/`fail`) node ids are rejected.
- **Type checking** — reuses the `RuninatorType` algebra: `params.*` field access is checked
  against the declared parameter type, `for`/`map` sources must be iterable, ordering
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
4 |     console.run(command: params.b)
  |                          ^^^^^^^
```

`runinatorctl wdl check` uses these to report every finding (parse errors keep pest's own
rich rendering).

## CLI

```
runinatorctl wdl compile  workflow.wdl [-o out.json]
runinatorctl wdl decompile workflow.json [-o out.wdl] [--explicit]
runinatorctl wdl format   workflow.wdl [-o out.wdl] [--check]
runinatorctl wdl check    workflow.wdl
```

`runinatorctl workflows apply` also accepts `.wdl` files, `.wdlp` manifests, and
directories of `.wdl` files directly alongside JSON packs. The ctl compiles the pack
client-side, zips the compiled artifacts (`workflows.json` + optional `secrets.json`), and
uploads a single `application/zip` to the web service's `/packs/import` endpoint — compilation
never happens on the backend. With no path argument, `workflows apply` falls back to the
`~/.runinator/workflows` folder (honoring `RUNINATOR_HOME`) if it exists.

Re-applying a pack updates what changed: ctl stamps each compiled workflow / secret with its source
file's mtime, so the web service's newer-wins reconciliation overwrites an edited file and skips an
unedited one — without clobbering a workflow a user has since edited in the UI (whose stored
timestamp is newer). A subflow that targets a workflow neither in the pack nor already stored is
rejected at apply time.

## Secrets (`.wdls`)

A `.wdls` file is the secrets/config companion to `.wdl`: a flat list of `secret`/`config`
declarations addressing a dotted `scope.name`, mirroring WDL's `secret.*` / `config.*` reference
surface. Values are pure JSON literals (no references or `${...}` interpolation):

```
secret jira.token    = "abc123"
config jira.base_url = "https://acme.atlassian.net"
config app.retries   = 3
config app.flags     = { beta: true, region: "us" }
```

A dotted name with more than two segments joins the tail with `/` (so `secret jira.api.key` is the
secret `key` under scope `jira` named `api/key`). `secret` entries are stored as redacted secrets;
`config` entries are eagerly-resolvable config values. `parse_secrets_str` lowers `.wdls` to a
`SecretBundle`; `secrets_to_wdls` renders one back. A pack ships secrets as a sibling
`settings.wdls` (or `settings.json`) next to a directory pack, or via a `.wdlp` manifest's
`settings` path; the ctl folds them into the same compiled pack zip.

Standalone secret/config import requires a `.wdls` file (JSON is not accepted):
`runinatorctl settings import secrets.wdls`. The MCP `runinator_import_workflow_bundle` tool
likewise takes WDL `source` text, compiled client-side, rather than a JSON bundle.

## Decompiler scope

JSON → WDL recovers the full structured feature set — linear sequences, `for` loops,
`if/else`, `match`, `parallel`/`join`, `race`, `map`, `try/catch/finally`, and all leaf
node kinds — verified by compile → decompile → compile round-trip tests. Arbitrary
hand-written graphs with irreducible control flow (cross-block gotos that don't match a
structured shape) are not guaranteed to decompile.
