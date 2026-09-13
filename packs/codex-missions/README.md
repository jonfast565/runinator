# Bounded Codex missions

This pack adds coding and research/report mission loops backed by isolated Codex app-server
sessions. Each role has a separate session slot stored in the portable workspace, so loops resume
after worker reassignment without putting credentials in the snapshot.

Install Codex CLI 0.153.4 or newer, publish an enabled `codex` execution profile, and apply the pack
explicitly:

```bash
runinatorctl workflows apply packs/codex-missions
```

Start `runinator.missions.codex_coding_mission` or
`runinator.missions.codex_research_report_mission` with the same source and goal payload documented
in [`docs/help/missions.md`](../../docs/help/missions.md). Codex cannot commit, push, deploy, change
credentials, start missions, or access caller-defined MCP servers through these workflows.
