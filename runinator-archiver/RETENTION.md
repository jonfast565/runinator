# Database retention

The archiver is the lifecycle owner for runtime-generated database history. Each pass drains all
currently eligible rows in bounded batches, writes them to compressed JSONL, and then deletes the
source rows. A retention value can be set to `off`, `none`, or `disabled`, but the defaults below
keep every runtime growth path bounded.

| Data | Default | Eligibility |
| --- | ---: | --- |
| workflow runs, continuations, effects, journal/output/firing history | 90 days | terminal workflow; dependency rows are archived leaf-first |
| completed ready nodes | 30 days | completed, or stranded under a terminal workflow |
| action dispatch outbox | 7 days | published, or attempted with a persisted error |
| pipeline runs and trigger firings | 90 days | terminal pipeline; member workflows age out first |
| notifications and delivery attempts | 30 days | deliveries first; stale read and unread notifications both age out |
| resolved automation records and gates | 90 days | resolved, or owned by a terminal workflow |
| organization usage ledger | 365 days | age-based |
| workflow revisions | 365 days | age-based, while always retaining each workflow's newest revision |
| completed agent directives | 30 days | completed, failed, or expired |
| dead letters | 90 days | age-based |
| audit log | 365 days | age-based |
| idempotency keys | 7 days | age-based |

The archiver also permanently prunes its completed mark ledger after 30 days, expired/revoked auth
sessions and consumed/expired enrollment tokens after 7 days, and inactive cooldown keys after 30
days. Replica telemetry samples are separately pruned by the engine after 24 hours, and expired
replicas (with their provider registrations, samples, and directives) are reaped by the engine.

Mutable catalog and identity tables—workflows, pipelines, triggers, users, teams, organizations,
keys, policies, settings, and grants—are user-managed resources rather than generated history and
are removed through their explicit delete/revoke lifecycle. Live workflow state is deliberately
never removed by retention; the terminal-state checks prevent cleanup from corrupting resumable
execution.

Cold archive files are not database tables and are intentionally retained for the storage system's
lifecycle policy. Deployments must size or lifecycle `/var/lib/runinator/archive` according to their
compliance requirements.
