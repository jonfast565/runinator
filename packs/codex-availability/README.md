# Codex availability

This pack defines the supported `codex` execution profile and a small availability workflow.
Configure Codex to store credentials in a file, run `codex login`, approve and publish the profile
from a desktop agent, then apply the pack explicitly:

```bash
runinatorctl workflows apply packs/codex-availability
```

Only `~/.codex/auth.json` is collected. Personal configuration, plugins, skills, and MCP servers are
not included. Prefer a stored `CODEX_API_KEY` secret for unattended automation.
