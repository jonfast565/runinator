-- per-member (per workflow) pipeline failure mode override, keyed by workflow id in a json object:
-- {"<workflow_id>": "stop" | "continue" | "silently_continue" | "inquire"}. a member absent from the
-- map uses defaults.default_failure_mode (folded into the existing `defaults` json column).
ALTER TABLE pipelines ADD COLUMN member_failure_modes LONGTEXT NULL;
