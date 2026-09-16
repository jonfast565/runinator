# Runinator Enhancements — Claude Harness

> Companion to `../ENHANCEMENTS.md`, scoped to one question: what does Runinator still need in
> order to be a complete harness for Claude Code agents, as opposed to a complete durable
> orchestrator that happens to invoke them? Item IDs continue the roadmap's stable-ID convention in
> a new **12.x** band and do not collide with existing entries.

## Context

This survey came out of designing a three-mission agent harness over a real .NET monorepo:
a development mission (Jira ticket to merged, staging-deployed PR), a pull-request review mission,
and a QA mission that authors and executes acceptance tests. The exercise used
`packs/sdlc-missions`, `packs/ai-missions`, `ai-command.claude_code`, the builtin Jira and GitHub
polling adapters, execution profiles, and durable workspaces.

The conclusion is worth stating plainly, because it determines the shape of this list: **the
durable-execution half of an agent harness is done.** Epochs, evidence, budgets with exhaustion
policies, lifecycle intents, bounded steering, workspace leases with recovery, SHA-fenced merges,
approval gates, freeze windows, and offline state-machine tests are all present and were all
directly usable. Nothing in that layer blocked the design.

What is thin is the **agent** half: getting reliable structured decisions out of a model, iterating
on the prompts that produce them, measuring whether those decisions are any good, and reaching
systems that do not yet have a provider crate. Every item below is in that half.

Two framing constraints inherited from `AGENTS.md`: keep dependency direction
services -> shared-contracts, and thread any shared-contract change through every broker backend,
mapper, and config file.

**Source audit:** 2026-09-16, against workspace version `0.35.754`.

---

## Priority at a glance

| # | Item | Band | Owning crates |
|---|------|------|---------------|
| 12.1 | Generic HTTP provider | **P1** | provider-http (new), provider-catalog, secrets |
| 12.2 | Schema-constrained output for `claude_code` | **P1** | provider-ai, models |
| 12.3 | Prompt assets: external, versioned, diffable | **P2** | rexrap, pack, provider-ai |
| 12.4 | Agent decision evaluation harness | **P2** | workflows, ctl, database, command-center |
| 12.5 | Extensible adapter profile schemas | **P2** | adapter-contract, adapter-sdk, adapter-host |
| 12.6 | Conversational steering ingress | **P3** | adapter-host, provider-slack, engine |

Already tracked elsewhere, cross-referenced rather than refiled:

- **AI cost and token accounting** is `ENHANCEMENTS.md` **5.6** (P3). This survey independently
  confirmed it is still open and raises its practical urgency — see the note under 12.2.
- **Editor-first workflow tests** is **9.6** (P2) and is the natural delivery vehicle for 12.4.
- **Structured execution outcomes** is **11.2** (P2) and overlaps 12.2's error taxonomy.

---

## 12.1 Generic HTTP provider

- **Owning crates:** `runinator-provider-http` (new), `runinator-provider-catalog`,
  `runinator-secrets`.
- **Band:** P1.

**Verified 2026-09-16.** There is no HTTP provider crate. `ls runinator-provider-*` returns
`ai, approval, aws, catalog, console, db, email, functions, git, github, github-cli, jira,
local-files, slack, std, support, workspace`. HTTP exists only as two effectful compute intrinsics:
`runinator-compute/src/compute.rs:531` declares
`EFFECTFUL_INTRINSIC_NAMES = ["http_get", "http_post", "now", "uuid", "env"]`, surfaced as
`std.exec.http_get(url)` and `std.exec.http_post(url, body)`.

**Why this is necessary.** Those two intrinsics take a URL and an optional body. They cannot set a
header, so they cannot authenticate against anything using a bearer token, an API key header, or
basic auth. They cannot issue `PUT`, `PATCH`, or `DELETE`. They expose no per-call timeout, no retry
policy, no redirect or TLS control, and no access to the response headers or status semantics beyond
a bare `{status, body}`.

The consequence is that **the set of systems an agent workflow can reach is exactly the set of
systems that already has a hand-written Rust provider.** Everything else costs a new crate, a
packaged function, or `console.run("curl ...")`. All three are bad in different ways: a crate is
weeks of work and a permanent maintenance obligation for what is often a single endpoint; a
packaged function pulls in a container runtime and a Python or Node dependency tree to issue one
request; and `console.run` with `curl` defeats the entire security model, because the secret has to
be interpolated into a command string, and secrets are deliberately restricted to whole argument
values precisely so they never appear in a shell line.

This showed up immediately in the harness design. A QA mission that verifies acceptance criteria
against a staging REST API — which is most of what QA automation *is* — has no native way to issue
an authenticated request. The design was forced to shell out to PowerShell scripts, which means the
assertions live outside Runinator, the results come back as parsed stdout instead of structured
data, and the durable record of what was tested is a log blob rather than a value.

More broadly, an agent harness is a thing that *reaches into other systems on the model's behalf*.
Capping that reach at the provider catalog caps the harness. Every observability tool, ticketing
system, deploy platform, feature-flag service, and internal API a team might want an agent to
consult is on the far side of this gap.

**Approach.** A provider with one well-specified action rather than a family:

```
http.request(
    method: "GET" | "POST" | "PUT" | "PATCH" | "DELETE" | "HEAD",
    url: string,
    headers: map<string>?,
    query: map<string>?,
    body: any?,               // json-encoded unless body_format says otherwise
    body_format: "json" | "form" | "text" | "bytes"?,
    timeout_seconds: integer?,
    follow_redirects: boolean?,
    expect_status: integer[]? // fail the effect outside this set
) -> { status: integer, headers: map<string>, body: any, duration_ms: integer }
```

Secrets pass as whole argument values into `headers`, the same discipline the `jira` and `github`
providers already use for `token`. Declare `credential_scopes` so an execution profile can supply a
bearer token without the workflow naming a secret at all.

**Boundary notes.** Keep it a leaf provider with no dependency on `runinator-engine`. Retry and
timeout should compose with the existing `@retry` and `@timeout` attributes rather than duplicating
them; `expect_status` exists so a `404` can be a *value* in one workflow and a *failure* in another
without a `compute` block in between. An allowlist of permitted hosts, configurable per
organization, is worth designing in from the start — this action is the widest outbound surface the
platform would have, and an agent chooses its arguments.

---

## 12.2 Schema-constrained output for `claude_code`

- **Owning crates:** `runinator-provider-ai`, `runinator-models`.
- **Band:** P1.

**Verified 2026-09-16.** `runinator-provider-ai/src/provider.rs` declares an `output_schema`
parameter on the `codex` action (line 118,
`ParameterMetadata::optional("output_schema", RuninatorType::Any)`) and **no equivalent on
`claude_code`** (lines 58-83). The Claude path in `claude_code.rs` extracts `result` and
`session_id` and passes the parsed envelope through as `response`; nothing validates its shape.

**Why this is necessary.** Every routing decision in every shipped mission pack is a string
instruction followed by an unguarded parse. From `packs/sdlc-missions/sdlc-missions.rrx:166`:

> `Reply only with JSON: {"next_member":"runinator.sdlc.implement"|"runinator.sdlc.verify","summary":"short summary","findings":["..."]}`

and then, twelve lines later:

```rexrap
let decision: { next_member: string, summary: string, findings: string[] }
    = std.encoding.parse_json(review.response.result)
```

This is the load-bearing mechanism of the entire orchestration model. `next_member` is how a phase
chooses the next phase; it is the edge of the state machine. And it is protected by nothing but a
sentence in a prompt.

The failure mode is not exotic. A model that prefixes one sentence of preamble, wraps the object in
a fenced code block, or emits a trailing explanation produces a `parse_json` that throws. That
throw is classified as a deterministic failure, which consumes an attempt from
`budget "deterministic" attempts 2`, and two of those pause the whole mission into operator
handoff. So a cosmetic formatting slip — the single most common way a model output deviates —
escalates directly to a human interrupt, discarding a session that may represent an hour of work.

It is also an avoidable asymmetry. Codex phases can constrain their output; Claude phases, which
are the ones the shipped packs actually use, cannot. A team standardizing on Claude gets the weaker
guarantee.

**Approach.** Two layers, in order of value:

1. **Accept `output_schema` on `claude_code`** and thread it to the CLI's structured-output support,
   matching the `codex` action's contract exactly so the two are substitutable. Validate the parsed
   result against the schema in the provider and return a typed error when it does not conform.
2. **Add a bounded reformat retry** in the provider for the case where the response is
   *semantically* right but *syntactically* wrapped: strip a fenced block, or re-ask once with the
   parse error attached, before surfacing a failure. This is a provider-local concern and should not
   consume an orchestration budget attempt, because it is not a failure of the agent's judgment.

A third, cheaper mitigation belongs in the packs regardless of provider work: classify a parse
failure as `transient` rather than `deterministic` so it retries against the more generous budget.

**Boundary notes.** If validation failures become a distinct outcome rather than a generic effect
error, that is the same contract surface as **11.2 structured execution outcomes** — worth landing
together. Attaching the usage figures the CLI already returns in its result envelope while touching
this code path would also close most of **5.6** (AI cost and token accounting) at near-zero marginal
cost, since the response object is already being parsed here. 5.6 is filed at P3 on its own merits;
paired with this item it is close to free, and it is what lets an operator answer "what did this
ticket cost" — a question every team adopting an agent harness asks within the first week.

---

## 12.3 Prompt assets: external, versioned, diffable

- **Owning crates:** `runinator-rexrap`, `runinator-pack`, `runinator-provider-ai`.
- **Band:** P2.

**Verified 2026-09-16.** Every prompt in every shipped pack is an inline string literal. The
implement-phase prompt at `packs/sdlc-missions/sdlc-missions.rrx:131` is a single unbroken line of
roughly 500 characters embedded in the workflow source. The language does provide a compile-time
include — `file("scripts/job.py")`, per `runinator-rexrap/docs/language-reference.md` — but no
shipped pack uses it for prompts.

**Why this is necessary.** Prompt text is the agent's actual program. In a harness, it is the part
that changes most often and the part whose changes are hardest to reason about. Keeping it inline
creates three distinct problems.

*It is unreviewable.* A one-word change inside a 500-character single-line string produces a diff
that no reviewer can read. The most consequential edits in the system are the ones least visible in
code review.

*It is not independently versionable.* A prompt edit is a workflow revision, so prompt history and
orchestration-topology history are the same history. Rolling back a prompt regression means rolling
back a revision that may also contain a genuine topology fix, and answering "which prompt was this
mission run under" means reconstructing it from a revision snapshot.

*It cannot be varied.* There is no way to run two prompt variants against the same topology, which
makes 12.4 — measuring whether a prompt change helped — impossible to act on even once you can
measure it. Measurement without the ability to vary the input is not much use.

There is also a practical authoring cost: prompts embedded in `.rrx` cannot be linted, spell-checked,
or edited with the tooling people normally use for prose, and multi-paragraph guidance gets
compressed into one line because the alternative is unreadable. The flint harness needs to carry
roughly forty lines of repository conventions into three separate phases; as an inline literal that
is unmaintainable, and duplicating it across phases guarantees drift.

**Approach.** Three steps, each independently useful:

1. **Adopt `file()` for prompts in the shipped packs** — a documentation and example change, no code.
   This alone fixes reviewability and is available today.
2. **Add a `prompts` block to the pack format** so a pack declares named prompt assets that are
   content-addressed, imported alongside `settings`, and recorded per revision. A mission's evidence
   can then cite the prompt digest it ran under.
3. **Allow a prompt asset to be overridden per execution profile or per organization**, so one pack
   serves several repositories with different conventions without forking the workflows.

**Boundary notes.** Prompt assets are pack-scoped content, so they belong with the existing
artifact-digest machinery in `runinator-pack` rather than in the settings store — they are code, not
configuration. Interpolation still has to work; a prompt asset is a template with the same
`${...}` expression surface as an inline string, which means the substitution happens at codegen,
not at read time.

---

## 12.4 Agent decision evaluation harness

- **Owning crates:** `runinator-workflows`, `runinator-ctl`, `runinator-database`,
  `runinator-command-center`.
- **Band:** P2.

**Verified 2026-09-16.** `runinatorctl workflows test` simulates the state machine offline against
`tests` blocks with mocked task outputs, asserting on the branch taken and the final outputs — see
`packs/hello-world/hello-world.rrx` and `packs/ai-missions/ai-missions.rrx:424-477`. This is
genuinely valuable and unusually well done. It tests **routing**. It mocks away the model.

**Why this is necessary.** The existing test facility answers "given that the reviewer says
`next_member: implement`, does the pipeline loop correctly?" It cannot answer "does the reviewer say
`implement` when it should?"

That second question is the one that determines whether a harness is worth running. An agent harness
has two failure modes and they look nothing alike. The first is mechanical — a phase crashes, a
parse fails, a lease expires — and the platform already surfaces it loudly through epochs, budgets,
and operator handoff. The second is silent: every phase succeeds, every transition is valid, the
mission reaches `close`, and the work is bad. The reviewer approved a change it should have sent
back. The verifier ran the wrong test project and reported green. The QA agent wrote an acceptance
test that asserts nothing. Nothing in the platform notices, because from the orchestrator's
perspective these runs are indistinguishable from good ones.

This matters most under change. Prompts get edited, the underlying model gets updated, tool
allowlists get widened, `max_turns` gets tuned. Each of those can degrade decision quality without
producing a single failed run. Without a corpus to replay against, a team's only signal is the
gradual sense that the agent has gotten worse, arriving weeks after the change that caused it, with
no way to attribute it.

The platform is unusually close to being able to do this. Every phase already projects durable
evidence, missions already record epochs, and runs are already replayable. What is missing is a way
to say "here are forty past cases with known-good outcomes; run the current pack against all of them
and tell me the decision-agreement rate."

**Approach.**

1. **Define a case corpus format** — a directory of fixtures, each pinning the inputs a mission
   phase would see (ticket text, diff, prior evidence) and the expected decision, at whatever
   granularity is checkable: the `next_member` chosen, whether a specific finding was reported,
   whether an acceptance criterion was marked met.
2. **Add `runinatorctl workflows eval <pack> --corpus <dir>`** that runs the real AI phase against
   each case and reports per-case outcomes plus an aggregate agreement rate. Unlike `workflows test`
   this needs a server and costs real tokens, so it is a deliberate command, not part of `apply`.
3. **Persist eval runs** so agreement rate is a time series attributable to a pack revision and a
   prompt digest (12.3), and a regression is visible as a step change rather than a vibe.
4. **Support a judge phase** as a first-class corpus outcome for cases where the expected result is
   prose rather than an enum — a review summary, a test plan — scored by a separate bounded agent
   call rather than by string equality.

**Boundary notes.** This is the natural extension of **9.6 editor-first workflow tests** and should
share its fixture plumbing rather than growing a parallel one. Keep eval strictly out of the
`workflows apply` path: it is expensive and non-deterministic, and conflating it with validation
would make deployment flaky. The corpus itself is user content and belongs beside the pack in the
user's repository, not in the platform database; only the *results* are platform state.

---

## 12.5 Extensible adapter profile schemas

- **Owning crates:** `runinator-adapter-contract`, `runinator-adapter-sdk`,
  `runinator-adapter-host`.
- **Band:** P2.

**Verified 2026-09-16** against a live instance. The Jira adapter's `sdlc_profile` configuration
field is a closed struct. `runinatorctl orchestrations adapters kinds --json` reports its schema as
accepting exactly:

```
automation { branch_prefix, local_check_command, merge_method }
deployment { ref, workflow_id }
jira       { base_url, email, done_transition_id, done_status }
repository { owner, name, remote, base_branch, base_ref, local_path, github_scope }
slack      { search_query }
```

and nothing else. The same call shows the GitHub adapter's identity field is `repositories` and that
it requires `execution_profile_required_labels: {"runner": "desktop"}` for profile-authenticated
polling.

**Why this is necessary.** `sdlc_profile` is the delivery profile injected into every admitted
mission as `params.profile`. It is, by design, the per-project configuration surface of the whole
SDLC pattern — and it is the least extensible thing in the system. The fields it accepts are
compiled into Rust, so adding one is a code change, a release, and a redeploy.

The schema encodes a specific opinion about what a project looks like, and real projects disagree
with it in both directions. The flint harness needed a Jira transition map (six ids, because the
workflow has six meaningful states), a list of deploy workflows an agent may dispatch, a staging API
base URL, and a set of solution filters for scoping builds. None of those fit. Meanwhile
`slack.search_query` is required by the schema and the design does not use Slack at all, so it has to
be populated with a string nobody reads.

The field names also carry assumptions that become misleading. `jira.done_transition_id` and
`done_status` presume the pipeline terminates at Done. Flint's dev mission terminates at Ready for
Testing and hands off to a separate QA mission that owns Done. The configuration is expressible —
set `done_transition_id` to the Ready-for-Testing id — but the name now lies, permanently, for
everyone reading that adapter's config.

The workaround is to move policy into `config.*` settings slots and read it in the workflows. That
works and is what the flint design does. But it splits one project's configuration across two
stores with different lifecycles, different access control, and no shared validation, and it means
the adapter injects a profile that is deliberately incomplete. The thing that should be most
configurable per installation is the most rigid, and the escape hatch fragments the configuration
it was meant to hold.

**Approach.**

1. **Allow an adapter to declare a pass-through region** — an `extensions: any` field, or a
   declared-schema slot the adapter validates but does not interpret — so an installation can attach
   project-specific configuration to the profile the adapter already injects, without a Rust change.
2. **Let an adapter kind ship its schema as data** rather than a compiled struct, so a plugin
   adapter can define its own profile shape through `runinator-adapter-sdk` on the same terms as a
   builtin.
3. **Version profile schemas** so a field can be added without invalidating stored adapter
   configurations.

**Boundary notes.** `sdlc_profile` flows from adapter metadata through the admission ledger into
mission parameters, so widening it touches `runinator-adapter-contract` and every consumer that
types it. Validation should stay at the adapter boundary: the point of a declared schema is that a
bad profile is rejected at `adapters apply` rather than at epoch three of a mission. Note also that
`execution_profile_required_labels` on the GitHub adapter is a silent failure mode worth surfacing —
an adapter with no matching worker simply never polls, and `adapters poll-status` should say so in
those terms.

---

## 12.6 Conversational steering ingress

- **Owning crates:** `runinator-adapter-host`, `runinator-provider-slack`, `runinator-engine`.
- **Band:** P3.

**Verified 2026-09-16.** `runinatorctl missions steer <id> "<message>"` delivers a bounded message
into a mission's current steerable AI phase through the provider's structured protocol, and
`POST /orchestrations/{id}/steer` is the HTTP equivalent. The Slack provider offers `send_message`
plus six read actions. There is no inbound path from a Slack reply to a steer call; the builtin
adapter kinds are `github`, `jira`, and `generic_webhook`.

**Why this is necessary.** Steering is the platform's answer to the most common thing that happens
when a human watches an agent work: they notice it is heading somewhere wrong and want to say so
without killing the run. The mechanism is well designed — bounded, structured, accepted only while a
harnessed phase owns a steerable effect, and never injected as shell input.

But it is reachable only from a terminal. A mission can already post to Slack when it parks, when it
needs an approval, or when it finishes; the person who reads that message and knows the answer has
to leave Slack, find the mission id, and run a CLI command. In practice that means they do not, and
the correction arrives as a rejected PR twenty minutes later instead of a redirect in the moment.

The same gap applies to approvals. `notify on parked -> slack "#oncall"` tells a channel that
something is waiting; resolving it still requires `runinatorctl approvals approve <effect-id>`. For
an approval gating a staging migration — exactly the case where you want a fast, attributable
human decision — the round trip is the cost.

**Approach.** A Slack ingress adapter that maps a threaded reply on a mission's notification message
to a steer call on that mission, and an approval-response mapping for the approve and reject cases.
Reuse the existing correlation machinery: the outbound notification records the mission binding, so
the thread timestamp is a correlation key like any other. Authorization is the hard part and should
be explicit — a Slack user identity has to map to a Runinator principal with the right capability
before a reply can move a mission, and the audit record should name the human, not the adapter.

**Boundary notes.** Lower priority than the rest of this list: it is convenience over capability,
and everything it enables is already possible from the CLI. It is listed because it is the
difference between steering being a feature and steering being something people actually use, and
because the correlation and notification plumbing it needs is already built.

---

## Verification (per item, when implemented)

- **12.1** — a workflow issues an authenticated `PUT` against a test server with a secret supplied as
  a header value, asserts on a non-2xx status without failing the effect, and the secret appears in
  no log line, journal entry, or effect record.
- **12.2** — a phase whose agent returns a fenced code block, and one that returns prose before the
  JSON, both settle successfully; a phase whose agent returns a schema-violating object fails with a
  typed validation error and does not consume a deterministic budget attempt.
- **12.3** — a prompt edit produces a readable line-level diff; two pack revisions differing only in
  prompt text are distinguishable in mission evidence by digest.
- **12.4** — `workflows eval` against a corpus with known outcomes reports an agreement rate; an
  intentionally degraded prompt lowers it; results are attributable to a pack revision.
- **12.5** — an installation adds a project-specific field to an adapter profile and reads it in a
  workflow with no Rust change; an invalid profile is rejected at `adapters apply`.
- **12.6** — a threaded Slack reply steers a live mission, the audit record names the human who sent
  it, and a reply from an unauthorized user is rejected and logged.

## Note

Items here are advisory and independently landable. 12.1 and 12.2 are the two that materially change
what can be built: 12.1 widens what an agent can reach, and 12.2 makes what it decides reliable.
12.3 and 12.4 are what make a harness improvable over time rather than merely operable, and they
compose — 12.4 is hard to act on without 12.3. 12.5 is friction rather than a wall, with a working
if unsatisfying workaround. 12.6 is convenience.
