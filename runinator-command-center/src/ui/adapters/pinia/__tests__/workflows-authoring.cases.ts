import { expect, it, vi } from "vitest";
import { nextTick, watch } from "vue";
import { useWorkflowsStore } from "../workflows";
import { useProvidersStore } from "../providers";
import { decompileToWdl, fetchWorkflows, saveWorkflowWdl } from "../../../../core/api/commandCenterApi";
import { WORKFLOW_ID, TRIGGER_ID, workflowDefinition, graphCentroid, workflowTrigger, nestedWorkflowInputProvider, untypedActionProvider } from "./workflows-fixtures";

export function registerWorkflowAuthoringTests() {
  it("saves workflow edits as wdl and reloads workflow triggers", async () => {
    const workflows = useWorkflowsStore();
    const draft = workflowDefinition(WORKFLOW_ID, "bundle draft");
    draft.definition.ui = {
      layout: { nodes: { start: { x: 0, y: 0 }, end: { x: 270, y: 0 } } },
      edge_handles: { "start:next": { edgeStyle: "square", labelAnchor: { position: 0.25 } } },
    };
    Object.assign(workflows.workflowDraft, draft);
    workflows.workflowJson = JSON.stringify(draft.definition);
    workflows.workflowTriggers = [workflowTrigger(TRIGGER_ID, WORKFLOW_ID, "0 * * * *")];
    vi.mocked(decompileToWdl).mockResolvedValue("workflow bundle_draft { start -> end }");
    vi.mocked(saveWorkflowWdl).mockResolvedValue({
      workflows: [workflowDefinition(WORKFLOW_ID, "bundle saved")],
      triggers: [workflowTrigger(TRIGGER_ID, WORKFLOW_ID, "30 * * * *")],
    });
    vi.mocked(fetchWorkflows).mockResolvedValue([workflowDefinition(WORKFLOW_ID, "bundle saved")]);

    await workflows.saveSelectedWorkflow();

    expect(decompileToWdl).toHaveBeenCalledWith(
      expect.objectContaining({ id: WORKFLOW_ID, name: "bundle draft" }),
    );
    expect(saveWorkflowWdl).toHaveBeenCalledWith({
      source: "workflow bundle_draft { start -> end }",
      enabled: true,
      workflow_id: WORKFLOW_ID,
      ui: draft.definition.ui,
      triggers: [
        expect.objectContaining({
          id: TRIGGER_ID,
          workflow_id: WORKFLOW_ID,
          configuration: { cron: "0 * * * *", parameters: {} },
        }),
      ],
    });
    expect(workflows.workflowDraft.name).toBe("bundle saved");
    expect(workflows.workflowTriggers).toEqual([
      workflowTrigger(TRIGGER_ID, WORKFLOW_ID, "30 * * * *"),
    ]);
  });

  it("validates nested typed workflow input shaped step parameters", async () => {
    const workflows = useWorkflowsStore();
    const providers = useProvidersStore();
    providers.providers = [nestedWorkflowInputProvider()];
    await workflows.selectWorkflow(workflowDefinition(WORKFLOW_ID, "nested input"));
    (workflows.workflowDraft.definition as any).nodes.splice(1, 0, {
      id: "prepare",
      kind: "action",
      action: {
        provider: "workflow-input",
        function: "prepare",
        timeout_seconds: 300,
        configuration: {},
      },
      parameters: {},
      transitions: { next: { $node: "end" } },
    });
    workflows.openStepEditor("prepare");

    (workflows.stepEditor.nodeDraft as any).action.configuration = {
      workflow_input: {
        target: "prod",
        environments: {
          prod: { url: "https://example.test", retries: "twice" },
        },
        strategy: { manual: true },
      },
    };

    expect(workflows.applyStepEditor()).toBe(false);
    expect(workflows.stepEditorError).toBe(
      "Workflow Input.environments.prod.retries must be an integer",
    );

    (workflows.stepEditor.nodeDraft as any).action.configuration = {
      workflow_input: {
        target: "prod",
        environments: {
          prod: { url: "https://example.test", retries: 2 },
        },
        strategy: { manual: true },
      },
    };

    expect(workflows.applyStepEditor()).toBe(true);
    expect(
      (workflows.ensureWorkflowNodes().find((node) => node.id === "prepare") as any)?.action
        .configuration,
    ).toEqual({
      workflow_input: {
        target: "prod",
        environments: {
          prod: { url: "https://example.test", retries: 2 },
        },
        strategy: { manual: true },
      },
    });
  });

  it("applies untyped action parameter objects and WDL expressions into action configuration", async () => {
    const workflows = useWorkflowsStore();
    const providers = useProvidersStore();
    providers.providers = [untypedActionProvider()];
    await workflows.selectWorkflow(workflowDefinition(WORKFLOW_ID, "untyped action"));
    (workflows.workflowDraft.definition as any).nodes.splice(1, 0, {
      id: "notify",
      kind: "action",
      action: { provider: "webhook", function: "send", timeout_seconds: 300, configuration: {} },
      parameters: {},
      transitions: { next: { $node: "end" } },
    });
    workflows.openStepEditor("notify");

    (workflows.stepEditor.nodeDraft as any).action.configuration = {
      url: "https://example.test/hook",
      payload: {
        message: { $concat: ["ticket ", { $ref: { params: ["ticket_id"] } }] },
        urgent: true,
      },
    };

    expect(workflows.applyStepEditor()).toBe(true);
    expect(
      (workflows.ensureWorkflowNodes().find((node) => node.id === "notify") as any)?.action
        .configuration,
    ).toEqual({
      url: "https://example.test/hook",
      payload: {
        message: { $concat: ["ticket ", { $ref: { params: ["ticket_id"] } }] },
        urgent: true,
      },
    });
  });

  it("exits inline node editing after a successful apply", () => {
    const workflows = useWorkflowsStore();
    Object.assign(workflows.workflowDraft, workflowDefinition(WORKFLOW_ID, "inline edit"));
    (workflows.workflowDraft.definition as any).nodes.splice(1, 0, {
      id: "task-1",
      kind: "action",
      action: { provider: "console", function: "run", timeout_seconds: 300, configuration: {} },
      parameters: {},
      transitions: { next: { $node: "end" } },
    });
    workflows.populateStepEditor("task-1");
    workflows.selectedGraphEdgeId = "edge-1";

    expect(workflows.submitInlineNodeEdit("task-1", "renamed", "Friendly Name")).toBe(true);

    // inline edits set the display name and never touch the configured action.
    const node = workflows.ensureWorkflowNodes().find((item) => item.id === "renamed");
    expect(node).toMatchObject({
      name: "Friendly Name",
      action: { provider: "console", function: "run" },
    });
    expect(workflows.selectedStepId).toBe("");
    expect(workflows.selectedGraphEdgeId).toBe("");
  });

  it("keeps inline node editing open when apply fails", () => {
    const workflows = useWorkflowsStore();
    Object.assign(workflows.workflowDraft, workflowDefinition(WORKFLOW_ID, "inline edit"));
    (workflows.workflowDraft.definition as any).nodes.splice(1, 0, {
      id: "task-1",
      kind: "action",
      action: { provider: "console", function: "run", timeout_seconds: 300, configuration: {} },
      parameters: {},
      transitions: { next: { $node: "end" } },
    });
    workflows.populateStepEditor("task-1");

    expect(workflows.submitInlineNodeEdit("task-1", "end", "console.echo")).toBe(false);

    expect(workflows.selectedStepId).toBe("task-1");
  });

  it("does not remove protected terminal and entry nodes", () => {
    const workflows = useWorkflowsStore();
    Object.assign(workflows.workflowDraft, workflowDefinition(WORKFLOW_ID, "protected nodes"));

    workflows.populateStepEditor("start");

    expect(workflows.selectedStepKindLocked).toBe(true);
    expect(workflows.canRemoveSelectedStep).toBe(false);

    workflows.removeWorkflowNode("start");
    workflows.removeWorkflowNode("end");
    workflows.removeWorkflowNode("fail");

    expect(workflows.ensureWorkflowNodes().map((node) => node.id)).toEqual([
      "start",
      "end",
      "fail",
    ]);
  });

  it("does not allow protected node kinds to be changed", () => {
    const workflows = useWorkflowsStore();
    Object.assign(workflows.workflowDraft, workflowDefinition(WORKFLOW_ID, "protected nodes"));
    workflows.populateStepEditor("start");

    workflows.stepEditor.kind = "action";

    expect(workflows.applyStepEditor()).toBe(false);
    expect(workflows.stepEditorError).toBe("start node kind cannot be changed");
    expect((workflows.ensureWorkflowNodes().find((node) => node.id === "start") as any)?.kind).toBe(
      "start",
    );
  });

  it("creates new graph nodes without wiring them immediately", () => {
    const workflows = useWorkflowsStore();
    Object.assign(workflows.workflowDraft, workflowDefinition(WORKFLOW_ID, "standalone node"));
    workflows.workflowJson = JSON.stringify(workflows.workflowDraft.definition);
    const centroid = graphCentroid(workflows.graphNodes);

    workflows.addWorkflowNode("approval");

    const created = workflows
      .ensureWorkflowNodes()
      .find((node) => node.kind === "approval" && String(node.id).startsWith("approval"));
    expect(created).toMatchObject({
      kind: "approval",
      parameters: { approval_type: "generic", prompt: "Approval required" },
      transitions: {},
    });
    expect(
      (workflows.workflowDraft.definition as any).ui?.layout?.nodes?.[created!.id as string],
    ).toEqual(centroid);
    expect(workflows.graphEdges.some((edge) => edge.target === created?.id)).toBe(false);
    expect(workflows.selectedStepId).toBe(created?.id);
  });

  it("treats the connected-node action as a standalone node creation", () => {
    const workflows = useWorkflowsStore();
    Object.assign(workflows.workflowDraft, workflowDefinition(WORKFLOW_ID, "standalone node"));
    workflows.workflowJson = JSON.stringify(workflows.workflowDraft.definition);
    workflows.selectedStepId = "start";
    const centroid = graphCentroid(workflows.graphNodes);

    workflows.addConnectedWorkflowNode("output");

    const created = workflows
      .ensureWorkflowNodes()
      .find((node) => node.kind === "output" && String(node.id).startsWith("output"));
    expect(created).toMatchObject({
      kind: "output",
      parameters: { event_type: "workflow.output", data: {} },
      transitions: {},
    });
    expect(
      (workflows.workflowDraft.definition as any).ui?.layout?.nodes?.[created!.id as string],
    ).toEqual(centroid);
    expect(workflows.graphEdges.some((edge) => edge.target === created?.id)).toBe(false);
    expect(workflows.selectedStepId).toBe(created?.id);
  });

  it("keeps output payloads as validated raw json", () => {
    const workflows = useWorkflowsStore();
    Object.assign(workflows.workflowDraft, workflowDefinition(WORKFLOW_ID, "output payload"));
    (workflows.workflowDraft.definition as any).nodes.splice(1, 0, {
      id: "output-1",
      kind: "output",
      parameters: { event_type: "workflow.output", data: null },
      transitions: {},
    });

    workflows.populateStepEditor("output-1");

    // set output data directly in nodeDraft.parameters (the catalog field editor writes here).
    (workflows.stepEditor.nodeDraft as any).parameters = {
      event_type: "workflow.output",
      data: { message: "hello", retries: [1, 2], nested: { ok: true } },
    };

    expect(workflows.applyStepEditor()).toBe(true);
    expect(
      (workflows.ensureWorkflowNodes().find((node) => node.id === "output-1") as any)?.parameters,
    ).toEqual({
      event_type: "workflow.output",
      data: { message: "hello", retries: [1, 2], nested: { ok: true } },
    });
  });

  it("keeps WDL-lowered output payload expressions valid", () => {
    const workflows = useWorkflowsStore();
    Object.assign(workflows.workflowDraft, workflowDefinition(WORKFLOW_ID, "output expression"));
    (workflows.workflowDraft.definition as any).nodes.splice(1, 0, {
      id: "output-1",
      kind: "output",
      parameters: { event_type: "workflow.output", data: {} },
      transitions: {},
    });

    workflows.populateStepEditor("output-1");
    // write expression value directly into nodeDraft (as the catalog field editor would).
    (workflows.stepEditor.nodeDraft as any).parameters.data = { $ref: { params: ["message"] } };

    expect(workflows.applyStepEditor()).toBe(true);
    expect(workflows.stepEditorError).toBe("");
    expect(
      (workflows.ensureWorkflowNodes().find((node) => node.id === "output-1") as any)?.parameters
        ?.data,
    ).toEqual({ $ref: { params: ["message"] } });
  });

  it("applies config node WDL fields without validation errors", () => {
    const workflows = useWorkflowsStore();
    Object.assign(workflows.workflowDraft, workflowDefinition(WORKFLOW_ID, "config editor"));
    (workflows.workflowDraft.definition as any).nodes.splice(1, 0, {
      id: "config-1",
      kind: "config",
      parameters: {
        name: "release",
        metadata: { owner: "platform" },
      },
      transitions: {},
    });

    workflows.populateStepEditor("config-1");
    // write config fields directly into nodeDraft.parameters (as the catalog field editor would).
    (workflows.stepEditor.nodeDraft as any).parameters = {
      name: { $ref: { params: ["release_name"] } },
      metadata: {
        source: { $ref: { prev: ["artifact"] } },
        approved: true,
      },
    };

    expect(workflows.applyStepEditor()).toBe(true);
    expect(workflows.stepEditorError).toBe("");
    expect(
      (workflows.ensureWorkflowNodes().find((node) => node.id === "config-1") as any)?.parameters,
    ).toEqual({
      name: { $ref: { params: ["release_name"] } },
      metadata: {
        source: { $ref: { prev: ["artifact"] } },
        approved: true,
      },
    });
  });

  // production symptom: the form only ever showed `max_attempts`, and applying a step rebuilt the
  // whole `retry` object from it — so opening any wdl-authored step and pressing Apply silently
  // reverted its backoff, jitter, and retry class to the defaults.
  it("keeps the rest of the retry policy when a step is applied", () => {
    const workflows = useWorkflowsStore();
    useProvidersStore().providers = [untypedActionProvider()];
    Object.assign(workflows.workflowDraft, workflowDefinition(WORKFLOW_ID, "retry policy"));
    workflows.ensureWorkflowNodes().push({
      id: "flaky",
      kind: "action",
      action: { provider: "webhook", function: "send", configuration: {} },
      retry: {
        max_attempts: 3,
        backoff_base_seconds: 10,
        backoff_max_seconds: 600,
        jitter: true,
        retry_on: "timeout",
      },
      transitions: {},
    });

    workflows.populateStepEditor("flaky");
    expect(workflows.stepEditor.max_attempts).toBe(3);
    expect(workflows.stepEditor.backoff_base_seconds).toBe(10);
    expect(workflows.stepEditor.jitter).toBe(true);
    expect(workflows.stepEditor.retry_on).toBe("timeout");

    workflows.stepEditor.max_attempts = 5;

    expect(workflows.applyStepEditor()).toBe(true);
    expect(
      (workflows.ensureWorkflowNodes().find((node) => node.id === "flaky") as any)?.retry,
    ).toEqual({
      max_attempts: 5,
      backoff_base_seconds: 10,
      backoff_max_seconds: 600,
      jitter: true,
      retry_on: "timeout",
    });
  });

  // the node deadline and the action's call deadline are different fields. reading the action's as
  // a fallback made the box show 60 and then write that 60 to the node, inventing a node timeout
  // nobody asked for while the worker deadline the operator was actually editing never moved.
  it("does not confuse the node timeout with the action call timeout", () => {
    const workflows = useWorkflowsStore();
    useProvidersStore().providers = [untypedActionProvider()];
    Object.assign(workflows.workflowDraft, workflowDefinition(WORKFLOW_ID, "timeouts"));
    workflows.ensureWorkflowNodes().push({
      id: "call",
      kind: "action",
      action: { provider: "webhook", function: "send", timeout_seconds: 60, configuration: {} },
      transitions: {},
    });

    workflows.populateStepEditor("call");
    expect(workflows.stepEditor.timeout_seconds).toBe(0);

    expect(workflows.applyStepEditor()).toBe(true);
    const applied = workflows.ensureWorkflowNodes().find((node) => node.id === "call") as any;
    expect(applied.timeout_seconds).toBeUndefined();
    expect(applied.action.timeout_seconds).toBe(60);
  });

  // compensation has no catalog field and no step-editor state of its own; it rides along in
  // `nodeDraft`, so the thing worth pinning is that applying a step neither drops it nor invents one.
  it("carries a compensation through an applied step", () => {
    const workflows = useWorkflowsStore();
    useProvidersStore().providers = [untypedActionProvider()];
    Object.assign(workflows.workflowDraft, workflowDefinition(WORKFLOW_ID, "saga"));
    workflows.ensureWorkflowNodes().push({
      id: "deploy",
      kind: "action",
      action: { provider: "webhook", function: "send", configuration: { url: "deploy" } },
      compensation: {
        provider: "webhook",
        function: "send",
        timeout_seconds: 300,
        configuration: { url: "rollback" },
      },
      transitions: {},
    });

    workflows.populateStepEditor("deploy");
    workflows.stepEditor.max_attempts = 2;

    expect(workflows.applyStepEditor()).toBe(true);
    expect(
      (workflows.ensureWorkflowNodes().find((node) => node.id === "deploy") as any)?.compensation,
    ).toEqual({
      provider: "webhook",
      function: "send",
      timeout_seconds: 300,
      configuration: { url: "rollback" },
    });
  });

  // a half-filled compensation lowers to `compensate .()`, which fails to parse on the next save
  // with an error pointing at generated text the author never wrote. reject it at the step instead.
  it("refuses a compensation that names no provider action", () => {
    const workflows = useWorkflowsStore();
    useProvidersStore().providers = [untypedActionProvider()];
    Object.assign(workflows.workflowDraft, workflowDefinition(WORKFLOW_ID, "saga"));
    workflows.ensureWorkflowNodes().push({
      id: "deploy",
      kind: "action",
      action: { provider: "webhook", function: "send", configuration: {} },
      transitions: {},
    });

    workflows.populateStepEditor("deploy");
    workflows.stepEditor.nodeDraft = {
      ...workflows.stepEditor.nodeDraft,
      compensation: { provider: "", function: "", timeout_seconds: 60, configuration: {} },
    };

    expect(workflows.applyStepEditor()).toBe(false);
    expect(workflows.stepEditorError).toBe("Compensation: Select a valid task provider action");
  });

  // production symptom: the trigger dialog's per-field editors read a snapshot taken when the dialog
  // mounted. the service hydrates the draft by writing through the state object, and those writes
  // reached the raw object without ever notifying a watcher — so opening a second trigger showed the
  // first one's values, and editing one field wrote them back over the real ones.
  it("notifies watchers when the service hydrates the trigger draft", async () => {
    const workflows = useWorkflowsStore();
    const seen: string[] = [];
    watch(() => workflows.triggerJson.configuration, (json) => seen.push(json));

    workflows.editWorkflowTrigger(workflowTrigger(TRIGGER_ID, WORKFLOW_ID, "0 * * * *"));
    await nextTick();
    workflows.editWorkflowTrigger(workflowTrigger("other", WORKFLOW_ID, "30 2 * * *"));
    await nextTick();

    expect(seen).toHaveLength(2);
    expect(JSON.parse(seen[0]).cron).toBe("0 * * * *");
    expect(JSON.parse(seen[1]).cron).toBe("30 2 * * *");
    expect(workflows.triggerDraft.id).toBe("other");
  });

}
