use serde::Deserialize;

runinator_provider_support::provider_parse_params!(crate::error::INVALID_PARAMS);

mod jira_search_params;
pub(crate) use jira_search_params::JiraSearchParams;

mod jira_issue_key_params;
pub(crate) use jira_issue_key_params::JiraIssueKeyParams;

mod jira_comment_params;
pub(crate) use jira_comment_params::JiraCommentParams;

mod jira_ensure_comment_params;
pub(crate) use jira_ensure_comment_params::JiraEnsureCommentParams;

mod jira_transition_params;
pub(crate) use jira_transition_params::JiraTransitionParams;

mod jira_ensure_transition_params;
pub(crate) use jira_ensure_transition_params::JiraEnsureTransitionParams;

mod jira_comments_params;
pub(crate) use jira_comments_params::JiraCommentsParams;
