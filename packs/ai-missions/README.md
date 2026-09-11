# Bounded Claude Code missions

This pack provides two durable, workspace-bound AI loops:

- `runinator.missions.coding_mission`: implement → independent review → verification → handoff.
- `runinator.missions.research_report_mission`: investigation → independent critique → report.

Each phase is a separately durable orchestration epoch. The pipeline revision and allowed phase
set are pinned when the mission starts. A reviewer may choose only one declared next phase through
its structured result; it cannot edit graph edges, start an arbitrary workflow, or escape the
epoch budget. The coding mission allows at most ten epochs and research/report at most eight.

## Prerequisites

Install Claude Code on a worker that has the `capability=git` workspace label and authenticate it
through the worker's execution profile. The pack does not add an authentication profile because
credential collection and mounting are deployment-specific. Apply it explicitly:

```bash
runinatorctl workflows apply packs/ai-missions
```

The pack deliberately does not commit, push, open pull requests, deploy, alter credentials, or
start another mission. Its Claude Code tool allowlists admit only file inspection/editing plus
narrow status, diff, search, and verification commands. Reviewers and critics receive fresh
sessions; implementers and investigators resume their own recorded session when the loop returns.

## Starting missions

Supply an object with the requested work. `mcp_config` is optional and should name an MCP config
file already mounted where Claude Code runs.

```json
{
  "request": {
    "goal": "Add a durable progress view to the Command Center"
  },
  "mission": {
    "mcp_config": "/workspace/config/runinator-mission-mcp.json"
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
`runinatorctl missions steer <effect-id> <message>` to send a new instruction to an active
harnessed phase. The worker encodes that instruction as a Claude `stream-json` user message and
records it as an ordered provider event; it is never written as shell input.

For an MCP server injected *inside* a mission phase, configure Claude Code with a process like:

```json
{
  "mcpServers": {
    "runinator-mission": {
      "command": "runinatorctl",
      "args": ["mcp", "--mission-only"]
    }
  }
}
```

That reduced profile exposes only `missions show`, `missions evidence`, and `missions intent`.
It omits raw execution, server resources, mission creation, listing, and arbitrary effect steering.
Authorization still comes from the Runinator credential used by this process; use a least-privilege
service principal for the mission worker.

## Operational notes

The harness streams each Claude protocol event as durable workflow progress, while final outputs,
session IDs, and phase evidence are recorded in the orchestration binding. Use Mission evidence
and epoch history for audit and recovery. A malformed reviewer route or an exhausted epoch budget
fails the mission explicitly rather than silently looping.
