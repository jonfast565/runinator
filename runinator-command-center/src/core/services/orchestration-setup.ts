import { asJsonRecord, type JsonRecord } from "../domain/json";
import type {
  IngressAction,
  IngressPolicy,
  IngressRoute,
  BudgetPolicy,
  IntentPolicy,
  OrchestrationPolicy,
  PhasePolicy,
  Pipeline,
  WorkspacePolicy,
} from "../domain/models";

export type OrchestrationSetupPreset =
  "run_once" | "queue_per_key" | "latest_wins" | "live_update" | "pause_for_review" | "mission";

export type ActiveEventBehavior = "record" | "queue" | "restart" | "signal";
export type TerminalEventBehavior = "ignore" | "record" | "reopen";
export type MissionRecipeKind = "coding" | "research_report";

export interface OrchestrationPhaseDraft {
  member: string;
  terminal: boolean;
  retainResults: boolean;
  workspace: boolean;
}

export interface OrchestrationSetupDraft {
  schemaVersion: 1;
  preset: OrchestrationSetupPreset;
  scope: string;
  startEvent: string;
  updateEvent: string;
  activeBehavior: ActiveEventBehavior;
  terminalBehavior: TerminalEventBehavior;
  entryMember: string;
  maxEpochs: number;
  signalName: string;
  retryAttempts: number;
  retryExhaustion: "fail" | "pause" | "terminate";
  retryHandoff: string;
  sharedWorkspace: boolean;
  workspaceScope: string;
  workspaceLeaseSeconds: number;
  missionKind: MissionRecipeKind;
  phases: OrchestrationPhaseDraft[];
  adapterIds: string[];
}

export interface CompiledOrchestrationSetup {
  ingress: IngressPolicy;
  orchestration: OrchestrationPolicy;
  authoring: JsonRecord;
}

const CONTROL_INTENTS: Record<string, IntentPolicy> = {
  cancel: { effect: "terminate", priority: 100 },
  pause: {
    effect: "suspend",
    priority: 80,
    stop: "pause",
    restart: { kind: "current" },
  },
  resume: { effect: "resume", priority: 70, restart: { kind: "current" } },
};

export function defaultSetupDraft(
  pipeline: Pipeline,
  preset: OrchestrationSetupPreset = "run_once",
): OrchestrationSetupDraft {
  const memberKeys = pipeline.graph.members.map((member) => member.key);
  const mission = preset === "mission";
  const activeBehavior = activeBehaviorForPreset(preset);
  const pipelineKey = firstNonEmpty(pipeline.key?.trim(), slug(pipeline.name), "pipeline");
  const missionKind: MissionRecipeKind = "coding";
  const scope = mission ? `mission.${pipelineKey}` : `orchestration.${pipelineKey}`;

  return {
    schemaVersion: 1,
    preset,
    scope,
    startEvent: "start",
    updateEvent: activeBehavior === "signal" ? "updated" : "start",
    activeBehavior,
    terminalBehavior: preset === "run_once" ? "record" : "reopen",
    entryMember: memberKeys[0] ?? "",
    maxEpochs: mission ? 10 : activeBehavior === "restart" ? 10 : 1,
    signalName: "external_update",
    retryAttempts: preset === "pause_for_review" ? 1 : 0,
    retryExhaustion: preset === "pause_for_review" ? "pause" : "fail",
    retryHandoff: "",
    sharedWorkspace: mission,
    workspaceScope: mission ? "mission-source" : "orchestration-workspace",
    workspaceLeaseSeconds: mission ? 7200 : 300,
    missionKind,
    phases: memberKeys.map((member, index) => ({
      member,
      terminal: index === memberKeys.length - 1,
      retainResults: mission,
      workspace: mission,
    })),
    adapterIds: [],
  };
}

export function applyPreset(
  pipeline: Pipeline,
  current: OrchestrationSetupDraft,
  preset: OrchestrationSetupPreset,
): OrchestrationSetupDraft {
  const defaults = defaultSetupDraft(pipeline, preset);
  return {
    ...defaults,
    adapterIds: [...current.adapterIds],
    startEvent: current.startEvent || defaults.startEvent,
    updateEvent: current.updateEvent || defaults.updateEvent,
  };
}

export function compileOrchestrationSetup(
  draft: OrchestrationSetupDraft,
): CompiledOrchestrationSetup {
  const routes: IngressRoute[] = [route(draft.startEvent, "unbound", "start")];
  const intents: Record<string, IntentPolicy> = clone(CONTROL_INTENTS);

  if (draft.activeBehavior === "restart") {
    intents.refresh = {
      effect: "supersede",
      priority: 60,
      stop: "cancel",
      restart: { kind: "entry" },
    };
    routes.push(dispatchRoute(draft.updateEvent, "active", "refresh"));
  } else if (draft.activeBehavior === "signal") {
    intents.update = {
      effect: "signal",
      priority: 60,
      stop: "none",
      signal_name: draft.signalName.trim() || "external_update",
    };
    routes.push(dispatchRoute(draft.updateEvent, "active", "update"));
  } else {
    routes.push(route(draft.updateEvent, "active", draft.activeBehavior));
  }

  routes.push(dispatchRoute("cancel", "active", "cancel"));
  routes.push(dispatchRoute("pause", "active", "pause"));
  routes.push(dispatchRoute("resume", "active", "resume"));

  if (draft.terminalBehavior === "record") {
    routes.push(route(draft.updateEvent, "terminal", "record"));
  } else if (draft.terminalBehavior === "reopen") {
    routes.push(route(draft.updateEvent, "terminal", "requeue"));
  }

  const phases = Object.fromEntries(
    draft.phases.map((phase) => [phase.member, compilePhase(draft, phase)]),
  );
  const budgets: Record<string, BudgetPolicy> =
    draft.retryAttempts > 0
      ? {
          failure: {
            attempts: draft.retryAttempts,
            exhausted: draft.retryExhaustion,
            ...(draft.retryHandoff ? { handoff: draft.retryHandoff } : {}),
          },
        }
      : {};

  return {
    ingress: { scope: draft.scope.trim(), routes },
    orchestration: {
      intents,
      phases,
      budgets,
      entry_member: draft.entryMember || null,
      max_epochs: Math.max(1, Math.trunc(draft.maxEpochs)),
      defaults: {},
    },
    authoring: {
      schema_version: 1,
      preset: draft.preset,
      adapter_ids: [...draft.adapterIds],
      mission_kind: draft.preset === "mission" ? draft.missionKind : null,
    },
  };
}

export function mergeSetupMetadata(
  metadata: JsonRecord,
  compiled: CompiledOrchestrationSetup,
): JsonRecord {
  return {
    ...metadata,
    ingress: compiled.ingress,
    orchestration: compiled.orchestration,
    orchestration_authoring: compiled.authoring,
  };
}

export function classifyOrchestrationSetup(
  pipeline: Pipeline,
): OrchestrationSetupPreset | "custom" | null {
  const ingress = asJsonRecord(pipeline.metadata.ingress);
  const orchestration = asJsonRecord(pipeline.metadata.orchestration);

  if (!Object.keys(ingress).length || !Object.keys(orchestration).length) {
    return null;
  }

  const hint = asJsonRecord(pipeline.metadata.orchestration_authoring);
  const preset = hint.preset;

  if (typeof preset !== "string" || !isPreset(preset)) {
    return "custom";
  }

  const draft = draftFromPipeline(pipeline, preset);

  if (!draft) {
    return "custom";
  }

  const compiled = compileOrchestrationSetup(draft);
  return stable(compiled.ingress) === stable(ingress) &&
    stable(compiled.orchestration) === stable(orchestration)
    ? preset
    : "custom";
}

export function draftFromPipeline(
  pipeline: Pipeline,
  preset?: OrchestrationSetupPreset,
): OrchestrationSetupDraft | null {
  const ingress = pipeline.metadata.ingress as IngressPolicy | undefined;
  const orchestration = pipeline.metadata.orchestration as OrchestrationPolicy | undefined;

  if (!ingress || !orchestration) {
    return null;
  }

  const hint = asJsonRecord(pipeline.metadata.orchestration_authoring);
  const hintedPreset =
    preset ?? (typeof hint.preset === "string" && isPreset(hint.preset) ? hint.preset : null);

  if (!hintedPreset) {
    return null;
  }

  const start = ingress.routes.find(
    (candidate) => candidate.lifecycle === "unbound" && candidate.action === "start",
  );
  const active = ingress.routes.find(
    (candidate) =>
      candidate.lifecycle === "active" &&
      !["cancel", "pause", "resume"].includes(candidate.event_type),
  );
  const terminal = ingress.routes.find((candidate) => candidate.lifecycle === "terminal");
  const activeIntent = active?.intent
    ? Object.entries(orchestration.intents).find(([name]) => name === active.intent)?.[1]
    : undefined;
  const activeBehavior: ActiveEventBehavior =
    active?.action === "queue"
      ? "queue"
      : activeIntent?.effect === "supersede"
        ? "restart"
        : activeIntent?.effect === "signal"
          ? "signal"
          : "record";
  const workspace = Object.values(orchestration.phases).find((phase) => phase.workspace)?.workspace;
  const failureBudget = Object.entries(orchestration.budgets).find(
    ([name]) => name === "failure",
  )?.[1];

  return {
    schemaVersion: 1,
    preset: hintedPreset,
    scope: ingress.scope,
    startEvent: start?.event_type ?? "start",
    updateEvent: active?.event_type ?? "start",
    activeBehavior,
    terminalBehavior:
      terminal?.action === "requeue"
        ? "reopen"
        : terminal?.action === "record"
          ? "record"
          : "ignore",
    entryMember: orchestration.entry_member ?? pipeline.graph.members.at(0)?.key ?? "",
    maxEpochs: orchestration.max_epochs ?? 1,
    signalName: activeIntent?.signal_name ?? "external_update",
    retryAttempts: failureBudget?.attempts ?? 0,
    retryExhaustion: failureBudget?.exhausted ?? "fail",
    retryHandoff: failureBudget?.handoff ?? "",
    sharedWorkspace: Boolean(workspace),
    workspaceScope: workspace?.scope ?? "orchestration-workspace",
    workspaceLeaseSeconds: workspace?.lease_seconds ?? 300,
    missionKind: hint.mission_kind === "research_report" ? "research_report" : "coding",
    phases: pipeline.graph.members.map((member) => {
      const policy = Object.entries(orchestration.phases).find(
        ([name]) => name === member.key,
      )?.[1];
      return {
        member: member.key,
        terminal: !policy?.result.next_member,
        retainResults: [policy?.result.resources_patch, policy?.result.evidence].some(Boolean),
        workspace: Boolean(policy?.workspace),
      };
    }),
    adapterIds: Array.isArray(hint.adapter_ids)
      ? hint.adapter_ids.filter((value): value is string => typeof value === "string")
      : [],
  };
}

export function setupBehaviorSummary(draft: OrchestrationSetupDraft): string[] {
  const active = {
    record: "Repeated events are recorded without changing active work.",
    queue: "Repeated events wait until active work settles.",
    restart: "A repeated event replaces active work from the entry phase.",
    signal: `Repeated events send the ${draft.signalName || "external_update"} signal.`,
  }[draft.activeBehavior];
  const terminal = {
    ignore: "Events after completion are ignored.",
    record: "Events after completion are retained for audit.",
    reopen: "Events after completion start a new generation.",
  }[draft.terminalBehavior];
  return [
    `The ${draft.startEvent} event starts one orchestration for each correlation key.`,
    active,
    terminal,
    `Execution is limited to ${String(Math.max(1, draft.maxEpochs))} epoch${draft.maxEpochs === 1 ? "" : "s"}.`,
  ];
}

function compilePhase(draft: OrchestrationSetupDraft, phase: OrchestrationPhaseDraft): PhasePolicy {
  const result = phase.retainResults
    ? {
        resources_patch: "/resources_patch",
        evidence: "/evidence",
        ...(!phase.terminal ? { next_member: "/next_member" } : {}),
      }
    : {};
  const workspace = phase.workspace ? compileWorkspace(draft) : null;
  return { result, ...(workspace ? { workspace } : {}) };
}

function compileWorkspace(draft: OrchestrationSetupDraft): WorkspacePolicy {
  return {
    scope: draft.workspaceScope.trim() || "orchestration-workspace",
    requirements: draft.preset === "mission" ? { capability: "git" } : {},
    lease_seconds: Math.max(1, Math.trunc(draft.workspaceLeaseSeconds)),
    reuse: true,
    recovery: draft.preset === "mission" ? "wait" : "replace",
  };
}

function activeBehaviorForPreset(preset: OrchestrationSetupPreset): ActiveEventBehavior {
  if (preset === "queue_per_key") {
    return "queue";
  }

  if (preset === "latest_wins") {
    return "restart";
  }

  if (preset === "live_update") {
    return "signal";
  }

  return "record";
}

function route(
  event_type: string,
  lifecycle: "unbound" | "active" | "terminal",
  action: IngressAction,
): IngressRoute {
  return { event_type, lifecycle, action, predicates: [] };
}

function dispatchRoute(event_type: string, lifecycle: "active", intent: string): IngressRoute {
  return { ...route(event_type, lifecycle, "dispatch"), intent };
}

function isPreset(value: string): value is OrchestrationSetupPreset {
  return [
    "run_once",
    "queue_per_key",
    "latest_wins",
    "live_update",
    "pause_for_review",
    "mission",
  ].includes(value);
}

function slug(value: string): string {
  return value
    .trim()
    .toLowerCase()
    .replace(/[^a-z0-9_]+/g, "_")
    .replace(/^_+|_+$/g, "");
}

function firstNonEmpty(...values: (string | null | undefined)[]): string {
  return values.find((value) => Boolean(value)) ?? "";
}

function clone<T>(value: T): T {
  return JSON.parse(JSON.stringify(value)) as T;
}

function stable(value: unknown): string {
  if (Array.isArray(value)) {
    return `[${value.map(stable).join(",")}]`;
  }

  if (value && typeof value === "object") {
    return `{${Object.entries(value as Record<string, unknown>)
      .filter(([, item]) => item !== undefined)
      .sort(([left], [right]) => left.localeCompare(right))
      .map(([key, item]) => `${JSON.stringify(key)}:${stable(item)}`)
      .join(",")}}`;
  }

  return JSON.stringify(value);
}
