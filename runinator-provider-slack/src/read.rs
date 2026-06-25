// read-only Slack Web API actions (channels, history, threads, search). all are
// GET calls that take a bearer token plus query parameters and return the raw
// Slack JSON body. the action table drives both metadata and execution so the
// two cannot drift.

use runinator_models::errors::SendableError;
use runinator_models::providers::{
    ActionMetadata, ParameterMetadata, ResultMetadata, RuninatorType,
};
use runinator_models::runs::{ProviderExecutionRequest, TaskExecutionResult};
use runinator_models::value::Value;

use crate::errors::INVALID_PARAMS;
use crate::{build_client, parse_slack_ok, token_param};

const API_BASE: &str = "https://slack.com/api/";

// the scalar shape of a query parameter; controls metadata typing and how the
// JSON value is rendered into the GET query string.
#[derive(Clone, Copy)]
pub(crate) enum ParamKind {
    Str,
    Int,
    Bool,
    // comma-separated list of strings (e.g. conversations.list `types`).
    StrList,
}

pub(crate) struct ReadParam {
    pub name: &'static str,
    pub kind: ParamKind,
    pub required: bool,
    pub description: &'static str,
}

pub(crate) struct ReadAction {
    pub function: &'static str,
    pub summary: &'static str,
    pub endpoint: &'static str,
    pub params: &'static [ReadParam],
    // the principal collection/object key in the response, advertised as a result.
    pub result_key: &'static str,
    pub result_is_array: bool,
}

// shared pagination params for cursor-based conversations.* endpoints.
const LIMIT: ReadParam = ReadParam {
    name: "limit",
    kind: ParamKind::Int,
    required: false,
    description: "Maximum items to return (Slack caps per method).",
};
const CURSOR: ReadParam = ReadParam {
    name: "cursor",
    kind: ParamKind::Str,
    required: false,
    description: "Pagination cursor from a prior response_metadata.next_cursor.",
};

pub(crate) const READ_ACTIONS: &[ReadAction] = &[
    ReadAction {
        function: "conversations_list",
        summary: "List channels/conversations the token can see",
        endpoint: "conversations.list",
        params: &[
            ReadParam {
                name: "types",
                kind: ParamKind::StrList,
                required: false,
                description: "Channel types: public_channel, private_channel, mpim, im.",
            },
            ReadParam {
                name: "exclude_archived",
                kind: ParamKind::Bool,
                required: false,
                description: "Omit archived channels.",
            },
            LIMIT,
            CURSOR,
        ],
        result_key: "channels",
        result_is_array: true,
    },
    ReadAction {
        function: "conversations_history",
        summary: "Fetch messages in a channel",
        endpoint: "conversations.history",
        params: &[
            ReadParam {
                name: "channel",
                kind: ParamKind::Str,
                required: true,
                description: "Channel ID to read.",
            },
            ReadParam {
                name: "oldest",
                kind: ParamKind::Str,
                required: false,
                description: "Only messages after this ts (inclusive with `inclusive`).",
            },
            ReadParam {
                name: "latest",
                kind: ParamKind::Str,
                required: false,
                description: "Only messages before this ts.",
            },
            ReadParam {
                name: "inclusive",
                kind: ParamKind::Bool,
                required: false,
                description: "Include messages exactly at oldest/latest.",
            },
            LIMIT,
            CURSOR,
        ],
        result_key: "messages",
        result_is_array: true,
    },
    ReadAction {
        function: "conversations_replies",
        summary: "Fetch a thread's replies",
        endpoint: "conversations.replies",
        params: &[
            ReadParam {
                name: "channel",
                kind: ParamKind::Str,
                required: true,
                description: "Channel ID containing the thread.",
            },
            ReadParam {
                name: "ts",
                kind: ParamKind::Str,
                required: true,
                description: "Thread parent message ts.",
            },
            ReadParam {
                name: "oldest",
                kind: ParamKind::Str,
                required: false,
                description: "Only replies after this ts.",
            },
            ReadParam {
                name: "latest",
                kind: ParamKind::Str,
                required: false,
                description: "Only replies before this ts.",
            },
            ReadParam {
                name: "inclusive",
                kind: ParamKind::Bool,
                required: false,
                description: "Include messages exactly at oldest/latest.",
            },
            LIMIT,
            CURSOR,
        ],
        result_key: "messages",
        result_is_array: true,
    },
    ReadAction {
        function: "conversations_info",
        summary: "Fetch a single channel's metadata",
        endpoint: "conversations.info",
        params: &[
            ReadParam {
                name: "channel",
                kind: ParamKind::Str,
                required: true,
                description: "Channel ID.",
            },
            ReadParam {
                name: "include_num_members",
                kind: ParamKind::Bool,
                required: false,
                description: "Include the member count.",
            },
        ],
        result_key: "channel",
        result_is_array: false,
    },
    ReadAction {
        function: "users_info",
        summary: "Fetch a single user's profile",
        endpoint: "users.info",
        params: &[ReadParam {
            name: "user",
            kind: ParamKind::Str,
            required: true,
            description: "User ID.",
        }],
        result_key: "user",
        result_is_array: false,
    },
    ReadAction {
        function: "search_messages",
        summary: "Search messages (requires a user token with search:read)",
        endpoint: "search.messages",
        params: &[
            ReadParam {
                name: "query",
                kind: ParamKind::Str,
                required: true,
                description: "Search query, e.g. \"in:#bugs after:2026-06-01\".",
            },
            ReadParam {
                name: "count",
                kind: ParamKind::Int,
                required: false,
                description: "Results per page.",
            },
            ReadParam {
                name: "page",
                kind: ParamKind::Int,
                required: false,
                description: "Page number (1-based).",
            },
            ReadParam {
                name: "sort",
                kind: ParamKind::Str,
                required: false,
                description: "score or timestamp.",
            },
            ReadParam {
                name: "sort_dir",
                kind: ParamKind::Str,
                required: false,
                description: "asc or desc.",
            },
        ],
        result_key: "messages",
        result_is_array: false,
    },
];

pub(crate) fn find_action(function: &str) -> Option<&'static ReadAction> {
    READ_ACTIONS
        .iter()
        .find(|action| action.function == function)
}

// builds the advertised metadata for every read action from the table.
pub(crate) fn read_action_metadata() -> Vec<ActionMetadata> {
    READ_ACTIONS
        .iter()
        .map(|action| {
            let mut params = vec![token_param()];
            for param in action.params {
                let meta = if param.required {
                    ParameterMetadata::required(param.name, param.kind.ty())
                } else {
                    ParameterMetadata::optional(param.name, param.kind.ty())
                };
                params.push(meta.with_description(param.description));
            }

            let mut results = vec![ResultMetadata::new("ok", RuninatorType::Boolean)];
            if !action.result_key.is_empty() {
                let ty = if action.result_is_array {
                    RuninatorType::array(RuninatorType::map(RuninatorType::Any))
                } else {
                    RuninatorType::map(RuninatorType::Any)
                };
                results.push(ResultMetadata::new(action.result_key, ty));
            }
            results.push(ResultMetadata::new(
                "response_metadata",
                RuninatorType::map(RuninatorType::Any),
            ));

            ActionMetadata::new(action.function, action.summary)
                .with_parameters(params)
                .with_results(results)
        })
        .collect()
}

impl ParamKind {
    fn ty(self) -> RuninatorType {
        match self {
            ParamKind::Str | ParamKind::StrList => RuninatorType::String,
            ParamKind::Int => RuninatorType::Integer,
            ParamKind::Bool => RuninatorType::Boolean,
        }
    }
}

pub(crate) fn execute_read(
    action: &ReadAction,
    request: &ProviderExecutionRequest,
) -> Result<TaskExecutionResult, SendableError> {
    let params = &request.parameters;
    let token = params
        .get("token")
        .and_then(Value::as_str)
        .ok_or_else(|| INVALID_PARAMS.error("token is required"))?
        .to_string();

    let query = build_query(action, params)?;

    let mut url = reqwest::Url::parse(&format!("{API_BASE}{}", action.endpoint))
        .map_err(|err| INVALID_PARAMS.error(format!("bad Slack url: {err}")))?;
    url.query_pairs_mut().extend_pairs(query.iter());

    let client = build_client(request.timeout_secs)?;
    let response = client
        .get(url)
        .bearer_auth(token)
        .header("Accept", "application/json")
        .send()?;
    let output = parse_slack_ok(response)?;

    Ok(TaskExecutionResult {
        message: Some(format!("slack {} ok", action.function)),
        output_json: Some(output),
        chunks: Vec::new(),
        artifacts: Vec::new(),
    })
}

// renders the declared params present in the request into GET query pairs,
// enforcing required params and scalar typing.
pub(crate) fn build_query(
    action: &ReadAction,
    params: &Value,
) -> Result<Vec<(String, String)>, SendableError> {
    let mut query = Vec::new();
    for param in action.params {
        let value = params.get(param.name);
        let Some(value) = value.filter(|value| !value.is_null()) else {
            if param.required {
                return Err(INVALID_PARAMS.error(format!("{} is required", param.name)));
            }
            continue;
        };
        query.push((param.name.to_string(), scalarize(param, value)?));
    }
    Ok(query)
}

fn scalarize(param: &ReadParam, value: &Value) -> Result<String, SendableError> {
    match param.kind {
        ParamKind::Str => value
            .as_str()
            .map(str::to_string)
            .ok_or_else(|| INVALID_PARAMS.error(format!("{} must be a string", param.name))),
        ParamKind::Int => value
            .as_i64()
            .map(|n| n.to_string())
            .or_else(|| value.as_str().map(str::to_string))
            .ok_or_else(|| INVALID_PARAMS.error(format!("{} must be an integer", param.name))),
        ParamKind::Bool => value
            .as_bool()
            .map(|b| b.to_string())
            .ok_or_else(|| INVALID_PARAMS.error(format!("{} must be a boolean", param.name))),
        ParamKind::StrList => {
            let items = value
                .as_array()
                .ok_or_else(|| INVALID_PARAMS.error(format!("{} must be an array", param.name)))?;
            let mut parts = Vec::with_capacity(items.len());
            for item in items {
                let part = item.as_str().ok_or_else(|| {
                    INVALID_PARAMS.error(format!("{} entries must be strings", param.name))
                })?;
                parts.push(part.to_string());
            }
            Ok(parts.join(","))
        }
    }
}
