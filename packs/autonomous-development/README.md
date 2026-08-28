# Autonomous development orchestration

This pack is the integration-specific layer over Runinator's generic correlated-orchestration
features. It admits one canonical `ticket.lifecycle` identity, runs a linear plan/implement/check/
publish/handoff pipeline, and deliberately has no merge or deployment step.

Submit the admission event through a generic webhook adapter whose normalized payload contains:

- `ticket`: `key`, `summary`, `jira_base_url`, and `jira_email`;
- `repository`: `local_path`, `owner`, `name`, `base_ref`, `base_branch`, `remote`, and the exact
  GitHub adapter `github_scope` (for example `github:repository:123456`);
- `automation`: a generation-unique `branch`, `local_check_command`, `review_transition_id`, and
  `review_status`.

Configure `secret.github.token` and `secret.jira.token` in the target organization before applying
the pack. Configure Jira and GitHub adapters separately. The publish phase emits a normalized GitHub
PR correlation alias, so later GitHub check/PR webhooks are routed to the original ticket binding.

Apply with:

```bash
runinatorctl workflows apply packs/autonomous-development
```
