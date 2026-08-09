//! the hand-written endpoint documentation model.
//!
//! `utoipa` generates the document's skeleton from the handler annotations; these types carry the
//! prose, query parameters, and examples that are layered on top of it by [`super::enrich_operation`].
//! [`EndpointDoc`] is the unit each handler module exposes as its own `DOCS` slice.

pub use super::examples::Example;

use super::examples::UUID_EXAMPLE;

#[derive(Clone, Copy)]
pub struct EndpointDoc {
    pub method: &'static str,
    pub path: &'static str,
    pub tag: &'static str,
    pub summary: &'static str,
    pub description: &'static str,
    pub public: bool,
    pub request: Option<RequestDoc>,
    pub query: &'static [ParamDoc],
    pub success_status: u16,
    pub success_description: &'static str,
    pub response_example: Example,
}

#[derive(Clone, Copy)]
pub struct RequestDoc {
    pub description: &'static str,
    pub example: Example,
    pub content_type: &'static str,
}

#[derive(Clone, Copy)]
pub struct ParamDoc {
    pub name: &'static str,
    pub location: &'static str,
    pub description: &'static str,
    pub required: bool,
    pub example: &'static str,
}

pub const CURSOR: &[ParamDoc] = &[
    ParamDoc {
        name: "cursor",
        location: "query",
        description: "Return chunks after this numeric cursor.",
        required: false,
        example: "0",
    },
    ParamDoc {
        name: "limit",
        location: "query",
        description: "Maximum number of chunks to return.",
        required: false,
        example: "100",
    },
];
pub const WORKFLOW_FILTERS: &[ParamDoc] = &[ParamDoc {
    name: "name",
    location: "query",
    description: "Exact workflow name to fetch.",
    required: false,
    example: "hello-world",
}];
pub const WORKFLOW_RUN_FILTERS: &[ParamDoc] = &[
    ParamDoc {
        name: "status",
        location: "query",
        description: "Filter runs by workflow status.",
        required: false,
        example: "running",
    },
    ParamDoc {
        name: "workflow_id",
        location: "query",
        description: "Filter runs for one workflow definition.",
        required: false,
        example: UUID_EXAMPLE,
    },
    ParamDoc {
        name: "name",
        location: "query",
        description: "Filter runs by display name.",
        required: false,
        example: "nightly deploy",
    },
    ParamDoc {
        name: "open",
        location: "query",
        description: "When filtering by name, only return open runs.",
        required: false,
        example: "true",
    },
];
pub const RUN_FILTERS: &[ParamDoc] = &[ParamDoc {
    name: "status",
    location: "query",
    description: "Required low-level task run status filter.",
    required: true,
    example: "running",
}];
pub const PACK_IMPORT_PARAMS: &[ParamDoc] = &[
    ParamDoc {
        name: "overwrite",
        location: "query",
        description: "Replace existing workflows and settings from the pack when true.",
        required: false,
        example: "true",
    },
    ParamDoc {
        name: "x-runinator-json-workflow-risk",
        location: "header",
        description: "Required only when posting raw JSON to the pack import endpoint.",
        required: false,
        example: "system-breakage-possible",
    },
];
pub const WORKFLOW_IMPORT_HEADERS: &[ParamDoc] = &[ParamDoc {
    name: "x-runinator-json-workflow-risk",
    location: "header",
    description: "Required acknowledgement for importing raw JSON workflow bundles.",
    required: true,
    example: "system-breakage-possible",
}];
pub const WORKFLOW_TRIGGER_FILTERS: &[ParamDoc] = &[ParamDoc {
    name: "status",
    location: "query",
    description: "Filter due triggers by status when supported by the caller.",
    required: false,
    example: "enabled",
}];
pub const REPLICA_FILTERS: &[ParamDoc] = &[
    ParamDoc {
        name: "replica_type",
        location: "query",
        description: "Filter replicas by kind.",
        required: false,
        example: "worker",
    },
    ParamDoc {
        name: "status",
        location: "query",
        description: "Filter replicas by current status.",
        required: false,
        example: "online",
    },
];
pub const CATALOG_FILTERS: &[ParamDoc] = &[
    ParamDoc {
        name: "item_type",
        location: "query",
        description: "Filter catalog entries by type.",
        required: false,
        example: "provider_metadata",
    },
    ParamDoc {
        name: "uri",
        location: "query",
        description: "Fetch one catalog entry by URI.",
        required: false,
        example: "runinator://providers/std",
    },
];
pub const AUTOMATION_FILTERS: &[ParamDoc] = &[
    ParamDoc {
        name: "workflow_run_id",
        location: "query",
        description: "Filter automation records for a workflow run.",
        required: false,
        example: UUID_EXAMPLE,
    },
    ParamDoc {
        name: "external_item_id",
        location: "query",
        description: "Filter automation records linked to an external item.",
        required: false,
        example: UUID_EXAMPLE,
    },
];
pub const GATE_FILTERS: &[ParamDoc] = &[
    ParamDoc {
        name: "workflow_run_id",
        location: "query",
        description: "Filter gates for a workflow run.",
        required: false,
        example: UUID_EXAMPLE,
    },
    ParamDoc {
        name: "status",
        location: "query",
        description: "Filter gates by open, closed, or waiting status.",
        required: false,
        example: "open",
    },
];
pub const IDEMPOTENCY_QUERY: &[ParamDoc] = &[
    ParamDoc {
        name: "scope",
        location: "query",
        description: "Namespace for the idempotency key.",
        required: true,
        example: "github-webhooks",
    },
    ParamDoc {
        name: "key",
        location: "query",
        description: "Caller-provided idempotency key.",
        required: true,
        example: "delivery-123",
    },
];
pub const CREDENTIAL_QUERY: &[ParamDoc] = &[
    ParamDoc {
        name: "scope",
        location: "query",
        description: "Credential or config scope.",
        required: false,
        example: "slack",
    },
    ParamDoc {
        name: "name",
        location: "query",
        description: "Credential or config name.",
        required: false,
        example: "bot_token",
    },
    ParamDoc {
        name: "kind",
        location: "query",
        description: "Setting kind: secret or config.",
        required: false,
        example: "secret",
    },
];

#[allow(clippy::too_many_arguments)] // call sites are static endpoint declarations with named positions.
pub const fn endpoint(
    method: &'static str,
    path: &'static str,
    tag: &'static str,
    summary: &'static str,
    description: &'static str,
    public: bool,
    request: Option<RequestDoc>,
    query: &'static [ParamDoc],
    success_status: u16,
    success_description: &'static str,
    response_example: Example,
) -> EndpointDoc {
    EndpointDoc {
        method,
        path,
        tag,
        summary,
        description,
        public,
        request,
        query,
        success_status,
        success_description,
        response_example,
    }
}

pub const fn json_body(description: &'static str, example: Example) -> Option<RequestDoc> {
    Some(RequestDoc {
        description,
        example,
        content_type: "application/json",
    })
}
