//! the hand-written endpoint documentation model.
//!
//! `utoipa` generates the document's skeleton from the handler annotations; these types carry the
//! prose, query parameters, and examples that are layered on top of it by [`super::enrich_operation`].
//! [`EndpointDoc`] is the unit each handler module exposes as its own `DOCS` slice.

pub(crate) use super::examples::Example;

use super::examples::UUID_EXAMPLE;

#[derive(Clone, Copy)]
pub(crate) struct EndpointDoc {
    pub(crate) method: &'static str,
    pub(crate) path: &'static str,
    pub(crate) tag: &'static str,
    pub(crate) summary: &'static str,
    pub(crate) description: &'static str,
    pub(crate) public: bool,
    pub(crate) request: Option<RequestDoc>,
    pub(crate) query: &'static [ParamDoc],
    pub(crate) success_status: u16,
    pub(crate) success_description: &'static str,
    pub(crate) response_example: Example,
}

#[derive(Clone, Copy)]
pub(crate) struct RequestDoc {
    pub(crate) description: &'static str,
    pub(crate) example: Example,
    pub(crate) content_type: &'static str,
}

#[derive(Clone, Copy)]
pub(crate) struct ParamDoc {
    pub(crate) name: &'static str,
    pub(crate) location: &'static str,
    pub(crate) description: &'static str,
    pub(crate) required: bool,
    pub(crate) example: &'static str,
}

pub(crate) const CURSOR: &[ParamDoc] = &[
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
pub(crate) const WORKFLOW_FILTERS: &[ParamDoc] = &[ParamDoc {
    name: "name",
    location: "query",
    description: "Exact workflow name to fetch.",
    required: false,
    example: "hello-world",
}];
pub(crate) const WORKFLOW_RUN_FILTERS: &[ParamDoc] = &[
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
pub(crate) const RUN_FILTERS: &[ParamDoc] = &[ParamDoc {
    name: "status",
    location: "query",
    description: "Required low-level task run status filter.",
    required: true,
    example: "running",
}];
pub(crate) const PACK_IMPORT_PARAMS: &[ParamDoc] = &[
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
pub(crate) const WORKFLOW_IMPORT_HEADERS: &[ParamDoc] = &[ParamDoc {
    name: "x-runinator-json-workflow-risk",
    location: "header",
    description: "Required acknowledgement for importing raw JSON workflow bundles.",
    required: true,
    example: "system-breakage-possible",
}];
pub(crate) const WORKFLOW_TRIGGER_FILTERS: &[ParamDoc] = &[ParamDoc {
    name: "status",
    location: "query",
    description: "Filter due triggers by status when supported by the caller.",
    required: false,
    example: "enabled",
}];
pub(crate) const REPLICA_FILTERS: &[ParamDoc] = &[
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
pub(crate) const CATALOG_FILTERS: &[ParamDoc] = &[
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
pub(crate) const AUTOMATION_FILTERS: &[ParamDoc] = &[
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
pub(crate) const GATE_FILTERS: &[ParamDoc] = &[
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
pub(crate) const IDEMPOTENCY_QUERY: &[ParamDoc] = &[
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
pub(crate) const CREDENTIAL_QUERY: &[ParamDoc] = &[
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

pub(crate) const fn endpoint(
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

pub(crate) const fn json_body(description: &'static str, example: Example) -> Option<RequestDoc> {
    Some(RequestDoc {
        description,
        example,
        content_type: "application/json",
    })
}
