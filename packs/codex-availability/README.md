# Codex availability

This pack defines the supported `codex` execution profile and a small availability workflow.
Approve the profile on a desktop agent, use **Refresh** to complete the ordinary Codex browser
login, then apply the pack explicitly:

```bash
runinatorctl workflows apply packs/codex-availability
```

The login uses `~/.runinator/execution-profiles/codex` as its private `CODEX_HOME`, so refreshing the
profile does not replace the credentials used by the Codex desktop app. Only the isolated
`auth.json` is collected; personal configuration, plugins, skills, and MCP servers are not
included. Prefer a stored `CODEX_API_KEY` secret for unattended automation.
