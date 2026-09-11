# Bounded Claude Code missions

This pack provides two durable, workspace-bound AI loops:

- `runinator.missions.coding_mission`: implement → independent review → verification → handoff.
- `runinator.missions.research_report_mission`: investigation → independent critique → report.

Each phase is a separately durable orchestration epoch. The pipeline revision and allowed phase
set are pinned when the mission starts. A reviewer may choose only one declared next phase through
its structured result; it cannot edit graph edges, start an arbitrary workflow, or escape the
epoch budget. The coding mission allows at most ten epochs and research/report at most eight.

## Prerequisites

Install Claude Code, Git, `rg`, and `runinatorctl` on a worker that advertises the
`capability=git` workspace label. The Kubernetes worker image already includes these tools. Create
and publish an enabled execution profile named `claude`; every Claude phase is explicitly bound to
that profile with `@profile("claude")`. The profile must provide a valid Claude Code login, and the
worker must be able to clone the requested repository without an interactive credential prompt.
The checked-in `packs/claude-availability` pack shows the supported profile collection and approval
flow.

Apply this pack explicitly after the profile exists:

```bash
runinatorctl workflows apply packs/ai-missions
```

The pack deliberately does not commit, push, open pull requests, deploy, alter credentials, or
start another mission. Its Claude Code tool allowlists admit only file inspection/editing plus
narrow status, diff, search, and verification commands. Reviewers and critics receive fresh
sessions; implementers and investigators resume their own recorded session when the loop returns.

## Starting missions

Supply the requested work and an immutable source repository plus revision. A revision may be a
commit SHA, tag, or branch as understood by Git; the first phase resolves it to a commit and records
that SHA in mission resources. The workspace is cloned once and reused across all phases, including
repair loops. The repository must be reachable from the selected worker.

```json
{
  "request": {
    "goal": "Add a durable progress view to the Command Center"
  },
  "mission": {
    "source": {
      "repository": "https://github.com/example/project.git",
      "revision": "0123456789abcdef0123456789abcdef01234567"
    }
  }
}
```

```bash
runinatorctl missions start runinator.missions.coding_mission \
  --kind coding --correlation cc-123 --json-file mission.json

runinatorctl missions start runinator.missions.research_report_mission \
  --kind research-report --correlation research-123 --json-file mission.json
```

The Command Center **Missions** page offers the same recipe selection, correlation key, input,
epochs, evidence, and declared intent controls.

## Claude Code and Codex interaction

An external controller, including Claude Code or Codex, uses the normal MCP server or
`runinatorctl missions steer <mission-id> <message>` to send a new instruction to the current
harnessed phase. The server resolves the current effect from the mission binding and fences the
message to the worker replica that owns it, so callers do not discover or race raw effect IDs. The
worker encodes accepted input as a Claude `stream-json` user message and retains the complete
message as an ordered progress event; it is never written as arbitrary shell input.

Each mission phase injects a fixed MCP process equivalent to:

```json
{
  "mcpServers": {
    "runinator-mission": {
      "command": "runinatorctl",
      "args": ["mcp", "--mission-only", "--mission-id", "<current-mission-id>"]
    }
  }
}
```

That reduced surface exposes only `missions show`, `missions evidence`, and `missions intent` and
rewrites every tool call to the current mission ID. It omits raw execution, server resources,
mission creation, listing, and steering. Mission input cannot replace this configuration with an
arbitrary MCP command, and strict MCP mode prevents a checked-out repository from adding another
server. Restricted mode ignores repository-supplied settings and confines file tools to the assigned
workspace. The `dontAsk` mode plus `--permission-prompts none` auto-denies anything outside each
phase's explicit tool allowlist, so a non-interactive harness cannot hang on an approval prompt.
Authorization comes from the worker service credential inherited by the subprocess.

## Operational notes

The harness stores parsed Claude protocol lines once as durable workflow progress and bounds line
size, retained stderr, total output bytes, and event count. Final outputs, session IDs, pinned source
metadata, accepted steering, and phase evidence are recorded durably. Use Mission evidence and
epoch history for audit and recovery. A malformed reviewer route or an exhausted epoch budget fails
the mission explicitly rather than silently looping.

Run the pack's offline source and transition tests without contacting Claude or a Runinator server:

```bash
runinatorctl workflows test packs/ai-missions
```

The opt-in local-stack test `mission_orchestration_transitions_smoke` covers ingress admission,
multi-epoch routing, resource/evidence reduction, completion, and server-side mission filtering.
