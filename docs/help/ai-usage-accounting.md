# AI usage accounting

Runinator records machine-readable token usage reported by Claude Code and Codex on each terminal
effect attempt. The record is accounting metadata: it does not become workflow output, and broker
redelivery cannot create a duplicate charge. Interactive or older CLI sessions that do not expose
usage remain unaccounted instead of being recorded as zero.

Costs are stored as integer micro-USD. A provider-reported total is authoritative. If none is
reported, the platform AI rate card first matches the exact provider and model, then a `*` model
entry for that provider. With no match, the attempt is **Unpriced**. Changing a rate only affects
future attempts; stored history is never repriced.

Platform administrators manage fallback rates in **Resources & Billing → AI pricing**. Rates are
micro-USD per million tokens for input, cached input, cache-creation input, output, and reasoning
categories. The default card contains no AI prices so vendor pricing changes cannot silently alter
billing.

Run detail shows total tokens, priced cost, unpriced attempts, and each node/effect/model record.
Workflow overview rolls retained records up by run, node, provider, and model. The corresponding API
endpoints are `GET /workflow_runs/{id}/ai-usage` and `GET /workflows/{id}/ai-usage`; the workflow
endpoint accepts optional RFC 3339 `since` and `until` bounds.
