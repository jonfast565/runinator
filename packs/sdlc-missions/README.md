# Label-driven SDLC missions

This pack admits one durable `mission.sdlc` orchestration for each Jira issue carrying the label
configured in `sdlc.admission_label` (default: `autodev`). It divides delivery into bounded ticket
selection, Slack context, planning,
implementation, review, verification, pull-request publication, feedback repair, exact-revision
merge, deployment-impact analysis, named deployment, and Jira closure jobs.

## Adapter setup

Create a Jira polling adapter in **review** mode first. Use a 60-second cadence, set
`routing_scope` to `mission.sdlc`, and use broad project/status JQL such as:

```text
project = EXAMPLE AND statusCategory != Done
```

The first poll establishes a high-water mark and does not replay existing issues. JQL is retrieval
scope only: a restrictive JQL can hide an issue before pipeline admission, but it never defines the
admission policy. Configure the required label with:

```bash
runinatorctl settings set sdlc admission_label '"autodev"' --kind config
```

Inspect routing previews, then approve representative deliveries and enable the adapter. Changing
or removing the label does not cancel an admitted mission, but later nonmatching Jira updates and
comments are not recorded; cancellation remains an explicit lifecycle intent.

## Migration from pack version 1

Reapply this pack, configure `sdlc.admission_label` to the label your team uses, and broaden each
Jira polling adapter's JQL to a project/status retrieval scope. Existing missions remain active,
but Jira updates and comments that do not carry the configured label stop being recorded.

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
