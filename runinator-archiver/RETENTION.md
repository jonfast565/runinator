# Database retention

The archiver is the lifecycle owner for runtime-generated database history. Each pass drains all
currently eligible rows in bounded batches, writes them to compressed JSONL, and then deletes the
source rows. A retention value can be set to `off`, `none`, or `disabled`, but the defaults below
keep every runtime growth path bounded.

| Data | Default | Eligibility |
| --- | ---: | --- |
| task runs, chunks, and artifact metadata | 90 days | terminal task; children archive before the task row |
| workflow runs, continuations, effects, journal/output/firing history | 90 days | terminal workflow; dependency rows are archived leaf-first |
| run-scoped files and inactive library/staged file revisions | 90 days | terminal owning workflow, archived/non-current library revision, or abandoned staging row |
| action dispatch outbox | 7 days | published, or attempted with a persisted error |
| pipeline runs, member attempts, and trigger firings | 90 days | terminal pipeline; member workflows and attempts age out first |
| correlated ingress and orchestration history | 90 days | terminal admission/binding; evidence, commands, operations, epochs, events, aliases, and leases archive leaf-first |
| notifications and delivery attempts | 30 days | deliveries first; stale read and unread notifications both age out |
| resolved automation records and gates | 90 days | resolved, or owned by a terminal workflow |
| organization usage ledger | 365 days | age-based |
| workflow and pipeline revisions | 365 days | age-based, while always retaining each definition's newest revision |
| completed agent directives | 30 days | completed, failed, unsupported, or expired |
| dead letters | 90 days | age-based |
| audit log | 365 days | age-based |
| idempotency keys | 7 days | age-based |

The archiver also permanently prunes its completed mark ledger after 30 days, expired/revoked auth
sessions and consumed/expired enrollment tokens after 7 days, and inactive cooldown/mutex keys
after 30 days. Replica telemetry samples are separately pruned by the engine after 24 hours.
Expired replica rows remain until their telemetry samples and agent directives have completed their
own retention policies, preventing the replica foreign-key cascade from bypassing cold archival.

All of these durations, the pass interval, claim lease, batch size, and dry-run switch are persisted
server policy under the **Archiver** section in Command Center. A value of zero disables an optional
retention policy. Existing command-line/environment values remain the bootstrap policy until an
administrator first saves server settings; after that, archiver replicas reload the shared policy
at least every 30 seconds while waiting between passes.

Mutable catalog and identity tables—workflows, pipelines, triggers, users, teams, organizations,
keys, policies, settings, and grants—are user-managed resources rather than generated history and
are removed through their explicit delete/revoke lifecycle. Live workflow state is deliberately
never removed by retention; the terminal-state checks prevent cleanup from corrupting resumable
execution.

`runinator-store::archive::DATABASE_TABLE_POLICIES` assigns every migrated table one of five
lifecycles: cold archive, parent cascade, service retention, bounded state, or explicit resource
lifecycle. The database test suite compares that inventory with the fully migrated schema and also
compares every cold-archive column mapping with the source table. Adding a table or column without
updating its retention/serialization policy therefore fails tests instead of silently growing or
dropping data.

Cold archive files and the blobs referenced by archived artifact/file metadata are not database
tables and are intentionally retained for the storage system's lifecycle policy. Deployments must
size or lifecycle `/var/lib/runinator/archive` and their blob backend according to compliance
requirements.
