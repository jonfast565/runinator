import type {
  WorkflowNodeKindMetadata,
  WorkflowTriggerKindMetadata,
} from "../../domain/models";

const endRef = () => ({ $node: "end" });

function nodeMeta(
  kind: WorkflowNodeKindMetadata["kind"],
  default_template: WorkflowNodeKindMetadata["default_template"],
  edge_slots: WorkflowNodeKindMetadata["edge_slots"] = [],
): WorkflowNodeKindMetadata {
  return {
    kind,
    label: kind,
    icon: "box",
    description: `${kind} node`,
    category: "task",
    protected: false,
    terminal: false,
    addable: true,
    supports_predicate_edges: edge_slots.length === 0,
    fields: [],
    edge_slots,
    default_template,
  };
}

// minimal catalog entries used by workflow unit tests when the backend catalog is not fetched.
export const testNodeKindCatalog: WorkflowNodeKindMetadata[] = [
  nodeMeta("action", {
    kind: "action",
    action: { provider: "", function: "", timeout_seconds: 300, configuration: {} },
    parameters: {},
    retry: { max_attempts: 1 },
    transitions: {},
  }),
  nodeMeta("approval", {
    kind: "approval",
    parameters: { approval_type: "generic", prompt: "Approval required" },
    retry: { max_attempts: 1 },
    transitions: { on_success: endRef(), on_reject: endRef() },
  }),
  nodeMeta(
    "condition",
    {
      kind: "condition",
      condition: {},
      transitions: {
        branches: [{ when: { equals: true }, target: endRef() }],
        next: endRef(),
      },
      parameters: {},
      retry: { max_attempts: 1 },
    },
    [
      {
        key: "branches",
        label: "Condition branch",
        taxonomy: "branch",
        target: { base: "transitions", path: ["branches"] },
        multiple: true,
        editable_label: true,
        editable_condition: true,
        orderable: true,
      },
    ],
  ),
  nodeMeta("wait", {
    kind: "wait",
    wait: { seconds: 60 },
    parameters: {},
    retry: { max_attempts: 1 },
    transitions: {},
  }),
  nodeMeta(
    "switch",
    {
      kind: "switch",
      parameters: { value: true, cases: [], default: endRef() },
      retry: { max_attempts: 1 },
      transitions: {},
    },
    [
      {
        key: "cases",
        label: "Switch case",
        taxonomy: "control",
        target: { base: "parameters", path: ["cases"] },
        multiple: true,
        editable_label: false,
        editable_condition: false,
        orderable: true,
      },
      {
        key: "default",
        label: "Switch default",
        taxonomy: "control",
        target: { base: "parameters", path: ["default"] },
        multiple: false,
        editable_label: false,
        editable_condition: false,
        orderable: false,
      },
    ],
  ),
  nodeMeta(
    "toggle",
    {
      kind: "toggle",
      parameters: { value: true, on: endRef(), off: endRef() },
      retry: { max_attempts: 1 },
      transitions: {},
    },
    [
      {
        key: "on",
        label: "Toggle on",
        taxonomy: "control",
        target: { base: "parameters", path: ["on"] },
        multiple: false,
        editable_label: false,
        editable_condition: false,
        orderable: false,
      },
      {
        key: "off",
        label: "Toggle off",
        taxonomy: "control",
        target: { base: "parameters", path: ["off"] },
        multiple: false,
        editable_label: false,
        editable_condition: false,
        orderable: false,
      },
    ],
  ),
  nodeMeta(
    "percentage",
    {
      kind: "percentage",
      parameters: { key: "user", buckets: [], default: endRef() },
      retry: { max_attempts: 1 },
      transitions: {},
    },
    [
      {
        key: "buckets",
        label: "Percentage bucket",
        taxonomy: "control",
        target: { base: "parameters", path: ["buckets"] },
        multiple: true,
        editable_label: false,
        editable_condition: false,
        orderable: true,
      },
      {
        key: "default",
        label: "Percentage default",
        taxonomy: "control",
        target: { base: "parameters", path: ["default"] },
        multiple: false,
        editable_label: false,
        editable_condition: false,
        orderable: false,
      },
    ],
  ),
  nodeMeta("parallel", {
    kind: "parallel",
    parameters: { branches: [] },
    retry: { max_attempts: 1 },
    transitions: {},
  }, [
    {
      key: "branches",
      label: "Parallel branch",
      taxonomy: "control",
      target: { base: "parameters", path: ["branches"] },
      multiple: true,
      editable_label: false,
      editable_condition: false,
      orderable: true,
    },
  ]),
  nodeMeta("join", {
    kind: "join",
    parameters: { wait_for: [], mode: "all" },
    retry: { max_attempts: 1 },
    transitions: {},
  }, [
    {
      key: "wait_for",
      label: "Join dependency",
      taxonomy: "control",
      target: { base: "parameters", path: ["wait_for"] },
      multiple: true,
      editable_label: false,
      editable_condition: false,
      orderable: true,
    },
  ]),
  nodeMeta("try", {
    kind: "try",
    parameters: { body: endRef(), catch: endRef(), finally: endRef() },
    retry: { max_attempts: 1 },
    transitions: {},
  }, [
    {
      key: "body",
      label: "Try body",
      taxonomy: "control",
      target: { base: "parameters", path: ["body"] },
      multiple: false,
      editable_label: false,
      editable_condition: false,
      orderable: false,
    },
    {
      key: "catch",
      label: "Try catch",
      taxonomy: "control",
      target: { base: "parameters", path: ["catch"] },
      multiple: false,
      editable_label: false,
      editable_condition: false,
      orderable: false,
    },
    {
      key: "finally",
      label: "Try finally",
      taxonomy: "control",
      target: { base: "parameters", path: ["finally"] },
      multiple: false,
      editable_label: false,
      editable_condition: false,
      orderable: false,
    },
  ]),
  nodeMeta("loop", {
    kind: "loop",
    parameters: { items: [], target: endRef() },
    max_iterations: 10,
    retry: { max_attempts: 1 },
    transitions: {},
  }, [
    {
      key: "target",
      label: "Loop target",
      taxonomy: "control",
      target: { base: "parameters", path: ["target"] },
      multiple: false,
      editable_label: false,
      editable_condition: false,
      orderable: false,
    },
  ]),
  nodeMeta("map", {
    kind: "map",
    parameters: { items: [], target: endRef(), concurrency: 1 },
    retry: { max_attempts: 1 },
    transitions: {},
  }, [
    {
      key: "target",
      label: "Map target",
      taxonomy: "control",
      target: { base: "parameters", path: ["target"] },
      multiple: false,
      editable_label: false,
      editable_condition: false,
      orderable: false,
    },
  ]),
  nodeMeta("race", {
    kind: "race",
    parameters: { branches: [] },
    retry: { max_attempts: 1 },
    transitions: {},
  }, [
    {
      key: "branches",
      label: "Race branch",
      taxonomy: "control",
      target: { base: "parameters", path: ["branches"] },
      multiple: true,
      editable_label: false,
      editable_condition: false,
      orderable: true,
    },
  ]),
  nodeMeta("output", {
    kind: "output",
    parameters: { event_type: "workflow.output", data: {} },
    retry: { max_attempts: 1 },
    transitions: {},
  }),
  nodeMeta("input", {
    kind: "input",
    parameters: { prompt: "Provide input" },
    retry: { max_attempts: 1 },
    transitions: {},
  }),
  nodeMeta("subflow", {
    kind: "subflow",
    subflow_id: "",
    parameters: {},
    retry: { max_attempts: 1 },
    transitions: {},
  }),
];

export const testMatchKindEnumCatalog = [
  {
    name: "match_kind",
    options: [
      { value: "equals", label: "equals" },
      { value: "not_equals", label: "not_equals" },
      { value: "exists", label: "exists" },
      { value: "when", label: "when" },
    ],
  },
];

export const testTriggerKindCatalog: WorkflowTriggerKindMetadata[] = [
  {
    kind: "cron",
    label: "Cron",
    icon: "clock",
    description: "Fires on a cron schedule.",
    fields: [
      {
        name: "cron",
        ty: { type: "string" },
        required: true,
        secret: false,
        widget: "cron",
      },
    ],
    default_configuration: { cron: "0 * * * *", parameters: {} },
  },
  {
    kind: "manual",
    label: "Manual",
    icon: "play",
    description: "Started manually.",
    fields: [],
    default_configuration: {},
  },
];
