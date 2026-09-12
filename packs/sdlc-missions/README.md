# Label-driven SDLC missions

This pack admits one durable `mission.sdlc` orchestration for each Jira issue carrying the
`runinator` label. It divides delivery into bounded ticket selection, Slack context, planning,
implementation, review, verification, pull-request publication, feedback repair, exact-revision
merge, deployment-impact analysis, named deployment, and Jira closure jobs.

## Adapter setup

Create a Jira polling adapter in **review** mode first. Use a 60-second cadence, set
`routing_scope` to `mission.sdlc`, and use JQL shaped like:

```text
project = EXAMPLE AND labels = runinator
```

The first poll establishes a high-water mark and does not replay existing tagged issues. Inspect
the routing previews, then approve representative deliveries and enable the adapter. Removing the
label after admission does not cancel the mission; cancellation is an explicit lifecycle intent.

The adapter's `sdlc_profile` is injected as `params.profile`. Configure one installed copy of this
pack per repository so the pipeline's `concurrency 2 on_conflict queue` is a per-repository FIFO
limit. The profile shape is:

```json
{
  "repository": {
    "owner": "acme",
    "name": "service",
    "remote": "origin",
    "base_branch": "main",
    "base_ref": "origin/main",
    "local_path": "/workspace/service",
    "github_scope": "github:repository:123456"
  },
  "automation": {
    "branch_prefix": "runinator/",
    "local_check_command": "cargo test --workspace",
    "merge_method": "squash"
  },
  "jira": {
    "base_url": "https://acme.atlassian.net",
    "email": "automation@acme.example",
    "done_transition_id": "31",
    "done_status": "Done"
  },
  "slack": {
    "search_query": "in:engineering EXAMPLE"
  },
  "deployment": {
    "workflow_id": "deploy.yml",
    "ref": "main"
  }
}
```

Configure provider settings named `jira`, `github`, and `slack`, plus a Claude execution profile
named `claude`. Configure a GitHub webhook adapter for pull request reviews and issue comments;
the publish phase records both GitHub pull-request database-id and number aliases.

Merge is guarded twice: the feedback mission must classify required review as approved, and the
merge job requires completed successful checks for the binding's exact candidate SHA before
calling GitHub's SHA-fenced squash-merge API. Deployment is dispatched only when an independent impact
mission selects the configured named workflow, then polled until success or operator handoff.
