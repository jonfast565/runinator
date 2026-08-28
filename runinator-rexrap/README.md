# runinator-rexrap

REXRAP is a human-friendly workflow language that **transpiles to the existing runinator
JSON workflow model**. It is purely an author-time front end: `compile_str` lowers REXRAP
to a `WorkflowDefinition` (with a `WorkflowGraph` `definition` and a `RuninatorType`
`input_type`), and `decompile` reconstructs REXRAP from a definition. The web service, waker,
worker, broker, and database are unchanged — they keep consuming the same graph.

The grammar in [`src/rexrap.pest`](src/rexrap.pest) is the canonical spec.

## Why

The JSON model is precise but the control flow is invisible: every edge is a
`{ "$node": "id" }`, every value is `{ "$ref": { "params": [...] } }`, every string is a
`{ "$concat": [...] }`, and conditions are nested objects. REXRAP makes the graph readable —
sequence implies edges, blocks expand into control nodes, references are dotted paths,
and conditions are infix.

## Example

```
workflow "Core Team SDLC Pipeline" v1 {
    params {
        jira: { base_url: string, email: string, token: string, jql: string }
    }

    do {
        @timeout(60s)
        let tickets = jira.search(
            base_url: params.jira.base_url,
            jql:      params.jira.jql,
        )

        for ticket in tickets.issues limit 50 {
            subflow("Ticket Work", params: {
                ticket,
                parent_workflow_run_id: run.run_id
            }, detached: true, reuse: true, name: "Ticket Work: ${ticket.key}")
        }
    }
}
```

## Language

**Statements imply edges.** Statements in sequence wire the forward edge (actions use
`on_success`, control-ish leaves use `next`). A synthetic `start`/`end`/`fail` are always
emitted. Every implicit part can also be written explicitly — see [Implicit vs explicit](#implicit-vs-explicit).

**A workflow's statements live in `do { … }`.** The runtime block is what a run executes; the
header declarations (`params`, `import`, `trigger`, `alias`, `type`, `interrupt`, …) sit above it.

**`let` binds a step's value.** Actions (`provider.fn(...)`), subflows (`subflow(...)`), control
blocks, and `compute` blocks bind with `let name = ...` or `let name: Type = ...`. Bare statements
are allowed when the value is not referenced. `emit`, `wait`, `approve`, and similar side-effect
statements stay bare unless you want a bound graph value.

**`async` is a call-site marker, not a colored callee.** A plain call runs inline and yields `T`;
`async` schedules the same call as a task and yields `task[T]`, which `await` joins and `detach`
drops. The callee is identical either way, so nothing is ever written twice. Consecutive `async`
launches fan out into one `parallel`, joined where the first handle is consumed.

```
let build = ci.build(repo: input.repo)          // sync  -> Artifact
let scan  = async security.scan(repo: input.repo)  // async -> task[Report]
let report = await scan                          // joins -> Report
```

**`routes { … }` makes transitions explicit.** Each arm names an outcome (or a predicate) and hands
control on with `continue`:

```
let deploy = github.deploy()
    routes {
        on success {
            continue verify
        }
        on failure {
            continue rollback
        }
        on timeout {
            continue alert
        }
    }
```

`end` and `fail` are reserved targets (the generated terminal nodes).

**Attributes are configuration.** `@timeout(60s)`, `@retry(3)`, `@tags("ci", "release")`, `@mcp`,
`@reentry(max_visits: 5)`, `@runner("creds-sync")`, and `@idempotent(key: input.invoice_id)` are
written as lines above the step. `@` is the only attribute surface: there is no fluent
`.timeout(...)` postfix chain. `@id("x")`, `@skip`, `@lock`, and `@deadline(30s)` describe the graph
node; the rest describe how the step executes.

`@idempotent(key: <expr>)` names the external effect this action has, so the platform can refuse to
produce it twice. The expression is resolved per dispatch against the same run context the action's
arguments see, then qualified by workflow (`workflow:<id>:<key>`) — two runs of the same workflow
computing the same key dedupe against each other; an unrelated workflow computing the same string
does not. Lowers to the action's `idempotency_key`.

The worker reserves the key before invoking the provider. Once an execution *succeeds* under a key,
any later delivery carrying it replays the recorded result instead of re-invoking; because the result
is recorded before the status publish, a failed publish no longer re-runs the side effect. A failed
attempt records nothing and frees the reservation, so `@retry(...)` still works. A key that resolves
to null or empty is treated as absent rather than shared, since collapsing every run onto one key
would silently skip real work.

What it cannot do: when a worker dies mid-invocation, nothing can know whether the side effect
landed. The resolved key is therefore also passed to the provider (`ProviderExecutionRequest
.idempotency_key`) so providers with native idempotency can dedupe on it upstream.

`@runner("<type>")` requires the step to run on a worker advertising the `runner=<type>` label
(`RUNINATOR_WORKER_LABELS`). The engine dispatches it to a live matching worker and parks the effect
until one connects, so pair it with `@timeout(...)` to fail the run when no such worker is available.
Lowers to the action's `required_labels` (`{ "runner": "<type>" }`).

`@retry(max, backoff: <dur>, max: <dur>, jitter: <bool>, on: any|failure|timeout)` — only `max`
(attempt count) is required. `backoff` is the first-retry delay and doubles each attempt up to the
`max` cap (defaults 1s/300s); `jitter` randomizes each delay into `[delay/2, delay]`; `on` narrows
which terminal status retries (`failure` skips retrying timeouts, e.g. so a long, expensive action
is not blindly re-run). Defaults preserve the historical behavior (exponential 1s→300s, retry any).

**Node kinds**

| REXRAP | JSON kind |
|---|---|
| `let id = provider.fn(args)` / `provider.fn(args)` | action |
| `let id = subflow("WF", params: { }, reuse: true)` / `subflow("WF", detached: true)` | subflow (wait / fire_and_forget) |
| `wait 30s until "ready"` | wait |
| `wait until <cond> every 30s` | condition poll-wait (sugar for `until <cond> { wait 30s }`; `every` defaults to 30s) |
| `signal "name" key <expr>` (event wait; `key` lets an external webhook route here by correlation value; bound by `@timeout(...)`) | signal |
| `emit "type" { data }` (payload is any expression; parenthesize an event-less scalar: `emit (42)`) | emit |
| `approve "..." type "..." { meta }` | approval |
| `set name = ...` / `set meta { }` | config |
| `fail "msg"` | fail |
| `if / else if / else` | condition |
| `match subj { "x" -> {} when c -> {} else -> {} }` | switch |
| `toggle <value> { on -> {} off -> {} }` (light switch: truthy → `on`, else `off`) | toggle |
| `split on <key> { 30% -> {} 70% -> {} else -> {} }` (weighted rollout, sticky per key via `hash(key)`) | percentage |
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
**Purity, not the grammar, decides where work runs:** a pure expression completes in the VM, while an
*effectful* call (`http_get`, `http_post`, `now`, `uuid`, `env`) is a semantic
error outside a `compute` block, since it must dispatch to a worker. A `compute { }` block is the
only place effectful calls and multi-statement programs (`let` / `return` / `goto` / `if`) live;
it lowers to `std.run` when pure and `std.exec` when effectful.

Foreign-language compute code uses a fenced form:

````
let score = compute "python" ```
def main(context):
    return {"score": context["input"]["score"] + 1}
@timeout(30s)
```
````

The fenced source is carried verbatim in the compiled workflow and lowers to `std.code`, which runs
on a worker through Docker. Foreign source must define `main(context)` and return one
JSON-serializable value; Runinator owns context loading and output serialization. Python awaitables
and JavaScript promises are awaited. The returned value becomes the compute node output and is
available to later REXRAP nodes as `score.field`. A typed binding such as
`let score: { score: integer } = compute ...` also validates the returned JSON at runtime. Bash's
`main` receives the context as a JSON string and must print its JSON result to stdout; Bash logging
must use stderr. The Docker image and optional bash setup script are configured by an
administrator under Admin -> Settings -> Foreign Languages, with built-in defaults for
`python`/`py`, `javascript`/`js`/`node`, `bash`/`sh`, `ruby`/`rb`, `perl`/`pl`, `php`,
`go`/`golang`, `swift`, `powershell`/`pwsh`/`ps1`, `csharp`/`c#`/`cs`,
`fsharp`/`f#`/`fs`, and `vbnet`/`vb.net`/`visualbasic`/`vb`. Setup
scripts are bash, so configured images must include `bash`. Local and Kubernetes workers need a
Docker-compatible CLI/runtime available to the worker process.

Entry points use each language's native function syntax. JavaScript must export
`main(context)` from its module (`export function main(context) { ... }`); Python may use
`def main(context)` or `async def main(context)`. Ruby, Perl, and PHP define their ordinary
`main` function. Foreign source that does not define this entry point fails before producing a
result; writing `RUNINATOR_OUTPUT` or printing JSON from a top-level script is not supported.
Go is the one naming exception because the language reserves `main` for the process entry point:
Go source uses `package main` and exports `func Main(context any) any`. Swift uses
`func main(_ context: Any) throws -> Any`, and PowerShell defines `function main($context)`.
C# defines `Foreign.Main(JsonElement context)`, F# defines `Foreign.main` over a `JsonElement`,
and VB.NET defines `Foreign.Main(JsonElement context)`. The three .NET languages share the
administrator-configurable .NET SDK image.

### Functions (`fn`)

Top-level `fn` definitions are reusable callables, hermetic over their parameters (only the params,
plus any nested lambda params, are in scope — no `params.*`/`config.*`/step outputs). A body is
either a single expression or a compute-style statement block:

```
fn label(id: string) -> string = "case-" ++ id            // expression body

fn build(id: string, token: string) -> object = {         // block body
    let resp = std.exec.http_get("https://api/cases/" ++ id)
    return { id: id, status: resp.body.status }
}
```

A block body reuses the `compute` lines (`let` / `return` / `if`, but **not** `goto` — a function is
not a graph region) and lowers to the same program form; falling off the end without a `return`
yields null (a void function). `@recursive(max_depth: N)` is required on any function that can reach
itself directly or mutually.

Functions may be **effectful** (call `std.exec.http_get`/`http_post`, read `secret.*`) — this is how
an integration can be packaged as one reusable function. Like any effectful call, an effectful
function may only be invoked inside a `compute` block (or another effectful body); calling it from a
declarative position (a condition, a value reference, an action argument, a parameter default) is a
semantic error, and any compute block that calls it dispatches to a worker (`std.exec`).

### Namespaces and imports

Names are qualified, not flat. There are three namespace roots:

- **`std`** — the builtin standard library, organized into modules: `std.math`, `std.strings`,
  `std.collections`, `std.objects`, `std.encoding` (incl. `hash`/`hash_percent` — pure, stable,
  process-independent hashes for deterministic bucketing/percentage rollouts), `std.logic`,
  `std.dates`, `std.regex`, and the
  effectful `std.exec`. A **prefix** intrinsic call must be fully qualified — `std.math.add(a, b)`,
  not `add(a, b)` — though the fluent/method form (`a.add(b)`) needs no prefix. The `std.` prefix is
  surface-only: the compiled graph and runtime dispatch use the bare leaf, so already-stored
  workflows are unaffected.
- **providers** — a provider action's name may be a dotted path; the trailing segment is the
  function and the leading segments are the provider (`github.repos.create_pr(...)` →
  provider `github.repos`, function `create_pr`).
- **workflow namespace** — an optional `namespace <path> { ... }` block or document-level
  `namespace <path>` declaration qualifies workflow identities so a subflow target can name a
  workflow in another pack (`subflow("core_sdlc.ticket_work")`).

`import` opens a namespace into local scope (header declaration, pure surface sugar — the compiled
graph always holds fully-resolved names):

```
namespace core_sdlc {
    workflow "Ticket Work" v1 {
        import std                          // the whole stdlib, callable bare: add(a, b), upper(x)
        import std.strings                  // just the strings module, callable bare: upper(x)
        import std.collections as col       // aliased: col.map(xs, f)
    }
}
```

Resolution order for an unqualified call is: file-local user `fn` → imported names → otherwise a
builtin intrinsic must be qualified or imported (a bare prefix intrinsic call is a semantic error
that names the module to use). The decompiler always emits the canonical `std.<module>.<leaf>` form.

**Source text includes**: `file("scripts/job.py")` reads a UTF-8 text file at compile time,
relative to the `.rrx` file's directory, and lowers to a normal string value. Paths must be
relative and cannot contain `..`, so pack compilation stays deterministic and local to the source
tree.

**Directory listings**: `dir("scripts")` lists the files under a directory at compile time and
lowers to an array of forward-slash relative paths (sorted, e.g. `["job.py", "lib/util.py"]`). It
lists the top level only by default; pass a boolean to recurse (`dir("scripts", true)`) and an
optional trailing integer to cap the recursion depth (`dir("scripts", true, 2)`). The same
relative-path safety rules as `file()` apply, and the listed files are bundled with the pack source.

For embedded source, use a fenced inline block:

````
let run = console.run(command: inline("python", ```
print("hello")
```))
````

Both forms are author-time conveniences; the runtime receives the compiled string value and does
not read files.

**Conditions**: `== != > >= < <=`, `contains`, `in`, `starts_with`, `ends_with`,
`exists x`, `&&`, `||`, `!`.

**Parameter typing**: `{ a: string, b?: integer, c: string[], d: map<string>, e: A | B }`
maps to `RuninatorType`. Open structs use `...: type`, e.g. `{ known: string, ...: any }`.
Refined forms are available for author-time constraints: `enum["dev", "prod"]`,
`integer range 0..10`, `number range 0.0..1.0`, and `duration range 1s..1h`
(open-ended ranges such as `integer range 0..` are allowed).

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

    do {
        ...
    }
}
```

The cron expression must be a string literal; the optional `with { … }` object is the run parameters.
`disabled` creates the trigger disabled, and `blackout` carries RFC3339 start/end timestamps.
Triggers belong to their workflow, so they are carried inside the compiled definition
(`definition.metadata.triggers`) and **materialized at import**: the web service replaces that
workflow's pack-managed (`managed_by: rexrap`) cron triggers with the declared set (idempotent on
re-apply; manually-added triggers are left alone). This works for a single source or a directory
pack, and they round-trip through decompile.

**Watch guards**: a header `watch <cond> -> <target>` declares a workflow-level cancellation guard.
The VM re-evaluates every guard on each drive — *including while the run is parked* on a
gate/signal/poll — and, the first time a condition holds, jumps the run to the handler node (fires at
most once per run). This replaces copy-pasted "poll, then bail if state changed" checkpoints with one
declaration that also catches the change mid-park. Guards lower to `definition.metadata.watches`
(`[{ condition, handler }]`) and round-trip through decompile. The condition sees the same context as
any other condition (params + node outputs), so a guard over live external state still needs a node
that refreshes the watched value.

```
workflow "Ticket Work" v1 {
    watch status_poll.fields.status.name != config.status.in_review -> handle_drift

    do {
        ...
    }
}
```

**Compensation (saga)**: an action node may declare `compensate <provider.fn(args)>`. When a later
step drives the run to a failed terminal (`fail`), the engine runs the recorded compensations of
every already-succeeded node in reverse order before the run terminates `Failed`. Compensation
parameters resolve against the live context, so a rollback can read the origin node's output (e.g. a
created resource id). Rollback is best-effort: a failing compensation does not halt the unwind.

```
let deploy_api = github.dispatch(workflow_id: "deploy", ref: "main")
    compensate github.dispatch(workflow_id: "rollback", ref: "main")
```

It lowers to the node's `compensation` (a `WorkflowAction`) and round-trips through decompile.

**Annotations**: `@id("explicit")`, `@skip`, `@lock`, and `@timeout(300s)` for round-trip
stability and node-level orchestration metadata. Step `@timeout(...)` remains the provider
command timeout; `@timeout(...)` maps to the workflow node timeout.

**Typed bindings**: `let tickets: { issues: any[] } = jira.search(...)` annotates a step's
output type. The annotation is checked during semantic analysis, persisted in the graph
metadata, and re-emitted by the decompiler so it survives a round trip.

**Workflow returns**: a workflow may declare the state shape a waiting subflow call exposes:

```
workflow "Deploy" v1 returns { url: string, env: enum["dev", "prod"] } {
    params { env: enum["dev", "prod"] }
    console.run(command: params.env)
}
```

The compiled definition stores this under `definition.metadata.rexrap.output_type`; the runtime wire
model is unchanged. A waiting `subflow(...)` binding sees the type at `child.state`, while
`detached: true` remains fire-and-forget metadata.

**Argument aliases**: shared arguments can be named once in the workflow header and spread with
`...name`, so a connection's `base_url`/`email`/`token` are written once instead of on every call:

```
workflow "Ticket Work" v1 {
    alias jira_conn = { base_url: config.jira.base_url, email: config.jira.email, token: secret.jira.token }

    do {
        let t = jira.transition(...jira_conn, key: params.ticket.key, transition_id: config.transitions.done)
    }
}
```

A `...name` spread works anywhere an object's entries are written: action arguments, object
literals `{ ... }`, subflow `params: { ... }`, and `approve "..." { ... }` metadata — including nested
objects. Entries apply in source order with **positional last-wins** (like JS spread): a later
`key: value` overrides an earlier spread of the same key, and a later spread overrides an earlier
entry. Aliases may compose other aliases (`alias full = { ...base, token: secret.x }`); reference
cycles are a compile error.

Aliases are surface sugar: spreads are expanded **before** semantic analysis and runtime
execution, so the runtime graph never sees an alias — the aliased and fully-expanded forms run
identically. To keep round trips faithful, lowering also records the authored alias declarations
and each call's spread layout in a render-only `rexrap` metadata sidecar (alongside declared types);
both `format` and `decompile` re-emit `alias`/`...name` from it, so aliased source compiles,
decompiles, and recompiles back to the same source — including composition and positional
overrides. Graphs authored without this sidecar (e.g. hand-written JSON, or compiled before the
sidecar existed) decompile to the equivalent fully-expanded form. A `secret.*` value spread through
an alias is still a whole argument value, so the "no secret mid-string" rule holds.

## Implicit vs explicit

REXRAP hides a lot for brevity: the entry edge, sequential edges, node ids, and several defaults
are inferred. Every one of them can be written explicitly instead, and the two forms compile to
the **same graph** — implicit is sugar, nothing is required. `decompile --explicit` emits the
canonical fully-expanded source so a reader never has to guess how a workflow is wired.

| Implicit (inferred) | Explicit form | Default |
|---|---|---|
| synthetic `start` → first statement | `start -> <id>` (top of body) | first statement |
| sequential happy-path edge | `ok -> <id>` (action/subflow/approval) or `next -> <id>` (wait/emit/config, control blocks) | next statement |
| auto node id (`action_1`, `for_loop_2`…) | `let x = …` (action/subflow/compute) or `@id("x") …` (any statement) | generated |
| step `@timeout(…)` | `@timeout(60s)` | 60s |
| step `@retry(…)` | `@retry(1)` | 1 attempt |
| `while`/`until` cap | `limit 1000` | 1000 |
| `for` cap / `map` fan-out | `limit none` / `concurrency none` | unbounded |
| approval kind | `type "generic"` | `generic` |
| `parallel` / `race` policy | `join all` / `winner first_success` | always shown |
| control-block continuation | trailing `} next -> <cont>` | next statement |

`until c` is sugar for `while !c`, and `subflow(..., detached: true)` picks fire-and-forget instead
of waiting. `limit none` /
`concurrency none` and an omitted cap are identical; the explicit form surfaces `none`.

So this terse workflow:

```
workflow "Hello" v1 {

    do {
        let greeting = console.run(command: "echo hi")
    }
}
```

is exactly this fully-explicit one (`decompile --explicit`):

```
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

`compile_str` runs a semantic pass on the AST — after parsing, before lowering — so
diagnostics anchor to source spans (`RexRapError::Semantic { span, message }`). Spans are
**expression-granular**: `Expr` and `Cond` carry their own spans, so a bad operand, a missing
field, or an unknown reference is blamed precisely rather than the whole statement. (A dotted
path still shares one span, so `params.a.b` blames the path, not the `b` segment.) It performs
four checks:

- **Name/reference resolution** — every path head (`input`/`prev`/`run`, an in-scope
  loop/map variable, or a declared step label) and every transition target must resolve.
- **Scope correctness** — loop/map variables are only visible inside their body; duplicate
  or reserved (`start`/`end`/`fail`) node ids are rejected.
- **Type checking** — reuses the `RuninatorType` algebra: `params.*` field access is checked
  against the declared parameter type, action arguments are checked against provider metadata,
  provider results type step outputs, subflow `with` parameters and `state` are checked when a
  workflow signature is available, `for`/`map` sources must be iterable, boolean contexts require
  booleans, ordering comparisons need orderable operands, and `string(x)`/`json(x)` reject
  incompatible values. `prev` and `run` references remain runtime-only and opaque.
- **Reachability** — statements that follow a terminator (`fail`, or a step whose happy-path
  arrow diverts the linear successor) and are not targeted by any transition are flagged.
  Reachability findings are **warnings**, not errors.

Errors block compilation; warnings are dropped by `compile_str` and surfaced by
`compile_str_with_diagnostics`, which returns the definition plus the warning list. The same
pass runs again when decompiled REXRAP is recompiled, so a round trip stays semantically valid.

`analyze_source` returns *all* diagnostics (errors and warnings) for a source, and both
`RexRapError::render(src)` and `Diagnostic::render(src)` produce a rustc-style caret snippet:

```text
error: unknown field 'b' on 'input'
 --> line 4, column 34
  |
4 |     console.run(command: params.b)
  |                          ^^^^^^^
```

`runinatorctl rexrap check` uses these to report every finding (parse errors keep pest's own
rich rendering).

## CLI

```
runinatorctl rexrap compile  workflow.rrx [-o out.json] [--typing strict|permissive]
runinatorctl rexrap decompile workflow.json [-o out.rrx] [--explicit]
runinatorctl rexrap format   workflow.rrx [-o out.rrx] [--check]
runinatorctl rexrap check    workflow.rrx [--typing strict|permissive]
```

REXRAP commands default to `--typing strict`. `--typing permissive` exists only for legacy
investigation; pack import paths keep strict typing.

`runinatorctl workflows apply` accepts a unified `.rrx` source or a directory of `.rrx`
sources, plus one standalone workflow-definition JSON file. It rejects raw workflow bundles.
For an REXRAP pack, ctl compiles client-side, zips the compiled artifacts (`workflows.json` plus
optional `secrets.json` and `pipelines.json`), and uploads one `application/zip` to the web
service's `/packs/import` endpoint — compilation never happens on the backend. With no path
argument, `workflows apply` falls back to the
`~/.runinator/workflows` folder (honoring `RUNINATOR_HOME`) if it exists.
Directory pack compilation runs in two passes: first it reads every workflow signature,
then it compiles each workflow with the full pack-local signature table so subflow calls are typed
before upload.

Re-applying a pack is an explicit force update: the web service replaces the pack's stored
definitions, then reconciles its managed triggers, notifications, and pipeline links. Every accepted
workflow definition is captured as an immutable revision, so rolling back creates a new revision
rather than overwriting history. A subflow that targets a workflow neither in the pack nor already
stored is rejected at apply time.

## Settings blocks

A unified `.rrx` source can contain a `settings` block with `secret`/`config`
declarations addressing a dotted `scope.name`, mirroring REXRAP's `secret.*` / `config.*` reference
surface. Values are pure JSON literals (no references or `${...}` interpolation):

```
secret jira.token    = "abc123"
config jira.base_url = "https://acme.atlassian.net"
config app.retries   = 3
config app.flags     = { beta: true, region: "us" }
```

A dotted name with more than two segments joins the tail with `/` (so `secret jira.api.key` is the
secret `key` under scope `jira` named `api/key`). `secret` entries are stored as redacted secrets;
`config` entries are eagerly-resolvable config values. The pack compiler lowers the block to a
`SecretBundle` and folds it into the same compiled pack zip as workflows and pipelines.

Standalone secret/config import requires an `.rrx` source containing a settings block (JSON is not accepted):
`runinatorctl settings import secrets.rrx`. The MCP `runinator_import_workflow_bundle` tool
likewise takes REXRAP `source` text, compiled client-side, rather than a JSON bundle.

## Pipelines and correlated orchestration

A unified `.rrx` pack can also declare a pipeline. A pipeline names its member workflows and links,
and can declare an `ingress` policy for correlation-scoped external events. Its optional
`orchestration` block turns those events into a durable controller: `intent` declares the
priority/coalescing/control effect, `budget` bounds retry classes and selects exhaustion behavior,
and `phase` maps member outputs and workspace leases into the correlated binding. The pack importer
resolves member names, persists the immutable pipeline revision, and materializes managed links.

```rexrap
pipeline "Ticket automation" {
    ingress scope "ticket.lifecycle" {
        on "ready" when unbound -> start
        on "cancel" when active -> dispatch "stop"
    }
    orchestration {
        intent "stop" effect terminate priority 100
        phase "implement" { evidence from "/evidence" }
    }
    workflow "implement"
}
```

The full checked-in reference is
[`packs/autonomous-development/autonomous-development.rrx`](../packs/autonomous-development/autonomous-development.rrx).

## Decompiler scope

JSON → REXRAP recovers the full structured feature set — linear sequences, `for` loops,
`if/else`, `match`, `toggle`, `split`, `parallel`/`join`, `race`, `map`, `try/catch/finally`,
and all leaf node kinds — verified by compile → decompile → compile round-trip tests. Arbitrary
hand-written graphs with irreducible control flow (cross-block gotos that don't match a
structured shape) are not guaranteed to decompile.
