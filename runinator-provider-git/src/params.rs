use serde::Deserialize;

runinator_provider_support::provider_parse_params!(crate::errors::INVALID_PARAMS);

mod worktree_params;
pub(crate) use worktree_params::WorktreeParams;

mod attempt_worktree_params;
pub(crate) use attempt_worktree_params::AttemptWorktreeParams;

mod prepare_checkout_params;
pub(crate) use prepare_checkout_params::PrepareCheckoutParams;

mod workspace_params;
pub(crate) use workspace_params::WorkspaceParams;

mod commit_params;
pub(crate) use commit_params::CommitParams;

mod cleanup_params;
pub(crate) use cleanup_params::CleanupParams;

mod push_params;
pub(crate) use push_params::PushParams;

mod archive_patch_params;
pub(crate) use archive_patch_params::ArchivePatchParams;

mod promote_revision_params;
pub(crate) use promote_revision_params::PromoteRevisionParams;
