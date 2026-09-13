# AI missions

Runinator missions are reusable, builder-authored orchestration recipes. Command Center includes
two editable starting presets:

- `runinator.missions.coding_mission` implements, independently reviews, verifies, and summarizes
  a code change.
- `runinator.missions.research_report_mission` investigates, independently critiques, and writes a
  final report without changing the source by default.

A mission is a durable orchestration, not one long subprocess. Each phase is recorded as an epoch,
the source revision and pipeline policy are frozen when the mission starts, and every retry or
review loop remains visible in the Missions page and CLI.

Each preset can use Claude Code or Codex CLI. The Codex variants are also available from the
explicit `packs/codex-missions` pack as `runinator.missions.codex_coding_mission` and
`runinator.missions.codex_research_report_mission`.

Runinator also ships an opt-in `runinator.sdlc.sdlc_mission` pack for a complete label-driven
software-delivery lifecycle. A Jira polling adapter admits an issue when it sees the `runinator`
label; smaller mission phases then gather Jira and Slack context, plan, implement, review, verify,
publish and repair a pull request, enforce an exact-SHA merge gate, assess deployment impact,
monitor a named deployment workflow when needed, and close the ticket. See
[`packs/sdlc-missions/README.md`](../../packs/sdlc-missions/README.md) for the adapter and project
profile contract. Install this pack explicitly; the ordinary mission setup helper does not apply it.

## Quick setup

The setup helper validates both packs, creates the `claude` execution-profile definition when it
does not exist, and installs both mission pipelines. It does not deploy the cluster, sign in to
Claude, approve credentials, or start billable model work.

In Command Center, open **Missions** and choose **New recipe**. Start blank or seed the builder with
the Coding or Research/report preset, then edit its typed inputs, phase graph, provider actions,
prompts, workspace policy, routes, and execution bound. Saving compiles the generated workflows and
ordinary `mission.*` pipeline in one transaction; it does not start billable work.

For Codex, first install the auth profile and mission pack:

```bash
runinatorctl workflows apply packs/codex-availability
runinatorctl workflows apply packs/codex-missions
```

`bash scripts/setup-codex-missions.sh` validates and applies both packs against the configured API.

For a local supervisor stack:

```bash
bash scripts/run-local.sh start
runinatorctl login
bash scripts/setup-ai-missions.sh
```

For Kubernetes, keep the web-service forward running in one terminal:

```bash
bash scripts/port-forward-ws.sh --port 8081
```

Then authenticate and run setup in another terminal:

```bash
RUNINATOR_API_BASE_URL=http://127.0.0.1:8081/ runinatorctl login
bash scripts/setup-ai-missions.sh --api-base-url http://127.0.0.1:8081/
```

The helper uses `runinatorctl` from `PATH`, then `target/debug/runinatorctl`, and builds the debug
CLI when neither exists. Authentication can also come from `RUNINATOR_API_KEY`; do not put a
password or token in the script. Run `bash scripts/setup-ai-missions.sh --help` for overrides.

## Make the Claude profile ready

The Kubernetes worker image already contains Claude Code, Git, GitHub CLI, and `rg`. The execution
profile securely transfers the desktop's Claude login to a worker for one effect; credentials are
not stored in the image or mission input.

On the desktop that runs `runinator-desktop-agent`:

1. Install Claude Code and run `claude` once to complete login.
2. Start the agent with `bash scripts/start-desktop-agent.sh` or open the packaged desktop agent.
3. Select the `claude` profile and press `a` to approve the displayed configuration digest. A
   changed collection configuration requires a new approval.
4. Wait for collection and publication, then verify it from another terminal.

```bash
runinatorctl execution-profiles list
runinatorctl execution-profiles status <claude-profile-id>
```

The profile is usable when its health and publication state are both `ready`. An approved profile
can still be unready when Claude is logged out, macOS Keychain access was denied, the desktop agent
cannot reach the API, or no credential revision has been published yet. After repairing the local
Claude login, request rotation in Command Center or with `runinatorctl execution-profiles rotate
<claude-profile-id>`.

The optional availability probe makes one small authenticated Claude request:

```bash
runinatorctl workflows run runinator.tests.claude.claude_availability
```

Its final output is `yes` on success. Pack installation itself never runs this probe.

## Make the Codex profile ready

Configure Codex with `cli_auth_credentials_store = "file"`, run `codex login`, and select the
Codex template on the Execution Profiles page. The profile collects only `~/.codex/auth.json`; it
does not copy personal configuration, plugins, skills, or MCP servers. Approve and publish the
profile as above, then optionally run:

```bash
runinatorctl workflows run runinator.tests.codex.codex_availability
```

For unattended automation, a stored secret bound to the action's `api_key` parameter is preferred.
It is injected as `CODEX_API_KEY` and never appears in the durable action parameters. When both an
API key and profile are supplied, the API key is authoritative. Republish file-backed profiles
after login credentials rotate.

## Start the first mission

Create `mission.json`. Use a repository URL the selected worker can clone non-interactively and a
commit SHA for the most reproducible run. A branch or tag also works, but Runinator resolves it once
and records the resulting commit before Claude edits anything.

```json
{
  "request": {
    "goal": "Add validation for empty display names and cover it with tests"
  },
  "mission": {
    "source": {
      "repository": "https://github.com/example/project.git",
      "revision": "0123456789abcdef0123456789abcdef01234567"
    }
  }
}
```

Start a coding mission:

```bash
runinatorctl missions start runinator.missions.coding_mission \
  --correlation coding-display-name-validation \
  --json-file mission.json
```

Or start a research/report mission from the same input shape:

```bash
runinatorctl missions start runinator.missions.research_report_mission \
  --correlation research-display-name-validation \
  --json-file mission.json
```

Choose a stable, unique correlation key for the logical job. Reusing it is an admission decision,
not a way to create an unrelated run. For retry-safe automation, also supply a stable `--event-id`;
submitting the same event ID again returns the original admission outcome.

## Watch and steer a mission

The start command prints the mission UUID. Use that UUID for all later controls:

```bash
runinatorctl missions list
runinatorctl missions show <mission-id>
runinatorctl missions evidence <mission-id>
runinatorctl missions steer <mission-id> "Focus on the failing parser regression test"
```

Steering is accepted only while a harnessed Claude or Codex phase owns a steerable effect. It is
delivered through the provider's structured protocol, never as shell input. If a mission is between phases or already terminal, wait
for the next active AI phase or inspect its final evidence instead.

Lifecycle intents are policy-defined controls, not arbitrary graph jumps. Inspect the mission's
frozen policy before submitting one:

```bash
runinatorctl missions intent <mission-id> cancel \
  --reason "The source branch was superseded"
```

An intent name that the installed pipeline does not declare is rejected. The Command Center
**Missions** page provides a guided recipe, objective, source, and revision editor, plus current
phase, epoch history, evidence, steering, and declared-intent controls. Optional recipe-specific
JSON is available only as an advanced input.

## Claude Code and Codex as external controllers

Claude Code or Codex outside a mission can use Runinator's general MCP server to start, inspect,
and steer missions. Register it with the matching helper:

```bash
scripts/install-mcp-claude-code.sh --local
scripts/install-mcp-codex.sh --local
```

For a Kubernetes forward on port 8081, use `--k8s --port 8081` instead. These registrations expose
the caller-authorized control surface. AI processes launched *inside* mission phases do not
inherit that broad MCP registration: Runinator injects a fixed mission-only server that can inspect
only the current mission, read its evidence, and submit its declared lifecycle intents.

## Troubleshooting

- **No mission recipes appear:** choose **New recipe** in Command Center and save a preset, or rerun
  `bash scripts/setup-ai-missions.sh` against the same API URL. Packs are installed explicitly and
  are not imported by Kubernetes deployment.
- **The profile is pending or unavailable:** keep the desktop agent running, approve the current
  profile digest, allow the required credential-file or Keychain access, and verify the selected
  CLI is logged in locally.
- **Checkout fails:** use a reachable HTTPS or SSH URL and make its credentials available to the
  worker without an interactive prompt. Confirm the requested revision exists remotely.
- **A mission does not accept steering:** inspect `missions show`; only an active harnessed AI
  effect is steerable.
- **A loop stops:** inspect `missions evidence` and epoch history. Coding missions allow ten epochs
  and research/report missions allow eight; exhaustion fails explicitly instead of looping forever.

For the phase policies and security boundaries, see
[`packs/ai-missions/README.md`](../../packs/ai-missions/README.md). For the Jira-to-deployment
lifecycle, see [`packs/sdlc-missions/README.md`](../../packs/sdlc-missions/README.md).
