import { compileRexRap, importPackArchive } from "../api/commandCenterApi";
import type { JsonRecord, JsonValue, Pipeline, WorkflowDefinition } from "../domain/models";
import { createZip } from "../utils/zip";

export type MissionPresetId = "blank" | "coding" | "research_report";
export type MissionInputKind = "string" | "integer" | "number" | "boolean" | "any";
export type MissionRouteMode = "fixed" | "result" | "terminal";

export interface MissionInputFieldDraft {
  path: string;
  label: string;
  description: string;
  kind: MissionInputKind;
  required: boolean;
  defaultValue?: JsonValue;
}

export interface MissionPhaseDraft {
  id: string;
  name: string;
  role: string;
  provider: string;
  action: string;
  promptParameter: string;
  profile: string;
  prompt: string;
  timeoutSeconds: number;
  actionParameters: JsonRecord;
  workspace: boolean;
  retainEvidence: boolean;
  routeMode: MissionRouteMode;
  nextPhase: string;
  allowedNextPhases: string[];
  responseTextPointer: string;
}

export interface MissionRecipeDraft {
  schemaVersion: 1;
  preset: MissionPresetId;
  name: string;
  key: string;
  namespace: string;
  description: string;
  maxEpochs: number;
  workspaceScope: string;
  workspaceLeaseSeconds: number;
  inputs: MissionInputFieldDraft[];
  phases: MissionPhaseDraft[];
}

interface PipelineBundle {
  pipelines: {
    name: string;
    key: string;
    namespace: string;
    description: string;
    defaults: JsonRecord;
    members: { name: string }[];
    links: unknown[];
    joins: unknown[];
    concurrency: { max_concurrent_runs: number; on_conflict: "allow" };
    metadata: JsonRecord;
    triggers: unknown[];
  }[];
}

const GOAL_INPUT = field(
  "request.goal",
  "Objective",
  "The outcome, constraints, and checks that matter.",
  "string",
);

const SOURCE_INPUTS: MissionInputFieldDraft[] = [
  GOAL_INPUT,
  field(
    "mission.source.repository",
    "Repository URL",
    "A repository the worker can reach.",
    "string",
  ),
  field("mission.source.revision", "Revision", "A commit SHA, branch, or tag.", "string"),
];

export function missionRecipePreset(preset: MissionPresetId): MissionRecipeDraft {
  if (preset === "research_report") {
    return recipe(
      preset,
      "Research and report mission",
      "research_report_mission",
      "Investigate, independently critique, and report with a bounded evidence loop.",
      8,
      [
        preparePhase("prepare", "investigate"),
        agentPhase(
          "investigate",
          "Investigator",
          "Investigate ${params.request.goal}. Gather evidence, record unknowns, and return a structured research note.",
          "critique",
        ),
        decisionPhase(
          "critique",
          "Independent critic",
          "Critique the evidence for ${params.request.goal}. Return JSON with next_member set to either the investigate or report workflow and explain material gaps.",
          ["investigate", "report"],
        ),
        terminalPhase(
          "report",
          "Reporter",
          "Produce the final evidence-backed report for ${params.request.goal}.",
        ),
      ],
    );
  }

  if (preset === "coding") {
    return recipe(
      preset,
      "Coding mission",
      "coding_mission",
      "Implement, independently review, verify, and summarize a bounded code change.",
      10,
      [
        preparePhase("prepare", "implement"),
        agentPhase(
          "implement",
          "Implementer",
          "Implement ${params.request.goal} in the assigned workspace. Read repository guidance, make the smallest complete change, and run relevant checks.",
          "review",
        ),
        decisionPhase(
          "review",
          "Independent reviewer",
          "Review the current diff for ${params.request.goal}. Return JSON with next_member set to either the implement or verify workflow.",
          ["implement", "verify"],
        ),
        decisionPhase(
          "verify",
          "Verifier",
          "Verify ${params.request.goal}. Run the narrowest relevant checks and return JSON with next_member set to either the implement or summary workflow.",
          ["implement", "summary"],
        ),
        terminalPhase(
          "summary",
          "Reporter",
          "Summarize the completed work, verification, review outcome, and remaining risks for ${params.request.goal}.",
        ),
      ],
    );
  }

  return recipe(
    preset,
    "Custom mission",
    "custom_mission",
    "A reusable bounded mission recipe.",
    8,
    [terminalPhase("work", "Agent", "Complete ${params.request.goal} and report the outcome.")],
    [GOAL_INPUT],
  );
}

export function missionRecipeFromPipeline(pipeline: Pipeline): MissionRecipeDraft | null {
  const value = pipeline.metadata.mission_authoring;

  if (!value || typeof value !== "object" || Array.isArray(value)) {
    return null;
  }

  const record = value as Record<string, unknown>;

  if (record.schemaVersion !== 1 || !Array.isArray(record.phases)) {
    return null;
  }

  return structuredClone(value) as unknown as MissionRecipeDraft;
}

export function validateMissionRecipe(draft: MissionRecipeDraft): string[] {
  const issues: string[] = [];

  if (!draft.name.trim()) {
    issues.push("Recipe name is required.");
  }

  if (!identifier(draft.key)) {
    issues.push("Recipe key must use letters, numbers, and underscores.");
  }

  if (!draft.namespace.trim() || !draft.namespace.split(".").every(identifier)) {
    issues.push("Namespace must contain dot-separated identifiers.");
  }

  if (!Number.isInteger(draft.maxEpochs) || draft.maxEpochs < 1) {
    issues.push("Maximum epochs must be a positive integer.");
  }

  if (
    draft.phases.some((phase) => phase.workspace) &&
    (!draft.workspaceScope.trim() || draft.workspaceLeaseSeconds < 1)
  ) {
    issues.push("Workspace phases need a scope and positive lease duration.");
  }

  if (!draft.phases.length) {
    issues.push("Add at least one phase.");
  }

  const ids = new Set<string>();

  for (const phase of draft.phases) {
    if (!identifier(phase.id)) {
      issues.push(`Phase '${phase.name || "unnamed"}' needs a valid ID.`);
    }

    if (!phase.name.trim()) {
      issues.push(`Phase '${phase.id}' needs a name.`);
    }

    if (ids.has(phase.id)) {
      issues.push(`Phase ID '${phase.id}' is duplicated.`);
    }

    ids.add(phase.id);

    if (!phase.provider || !phase.action) {
      issues.push(`Phase '${phase.id}' needs a provider action.`);
    }

    if (phase.timeoutSeconds < 1) {
      issues.push(`Phase '${phase.id}' needs a positive timeout.`);
    }
  }

  for (const phase of draft.phases) {
    const targets = phase.routeMode === "fixed" ? [phase.nextPhase] : phase.allowedNextPhases;

    for (const target of targets) {
      if (!ids.has(target)) {
        issues.push(`Phase '${phase.id}' routes to unknown phase '${target}'.`);
      }
    }

    if (phase.routeMode === "result" && !phase.allowedNextPhases.length) {
      issues.push(`Phase '${phase.id}' must allow at least one next phase.`);
    }

    if (phase.routeMode === "result" && (!phase.promptParameter || !phase.responseTextPointer)) {
      issues.push(`Phase '${phase.id}' needs an agent-capable action for result routing.`);
    }
  }

  if (!draft.phases.some((phase) => phase.routeMode === "terminal")) {
    issues.push("At least one phase must be terminal.");
  }

  if (draft.phases.length) {
    const reachable = reachablePhaseIds(draft.phases);

    for (const phase of draft.phases) {
      if (!reachable.has(phase.id)) {
        issues.push(`Phase '${phase.id}' is unreachable from the entry phase.`);
      }
    }

    if (!draft.phases.some((phase) => reachable.has(phase.id) && phase.routeMode === "terminal")) {
      issues.push("The entry phase cannot reach a terminal phase.");
    }
  }

  const paths = new Set<string>();
  const systemPaths = [
    "mission.correlation_key",
    "mission.requested_by",
    "orchestration.binding_id",
    "orchestration.workspace_affinity",
    "orchestration.resources",
  ];

  for (const input of draft.inputs) {
    if (
      !input.path.trim() ||
      !input.path.split(".").every(identifier) ||
      systemPaths.some((systemPath) => pathsOverlap(input.path, systemPath))
    ) {
      issues.push(`Input path '${input.path}' is reserved or invalid.`);
    }

    if ([...paths].some((path) => pathsOverlap(path, input.path))) {
      issues.push(`Input path '${input.path}' duplicates or overlaps another input.`);
    }

    paths.add(input.path);
  }

  return issues;
}

export async function saveMissionRecipe(draft: MissionRecipeDraft): Promise<Pipeline> {
  const issues = validateMissionRecipe(draft);

  if (issues.length) {
    throw new Error(issues.join(" "));
  }

  const workflows: WorkflowDefinition[] = [];

  for (const phase of draft.phases) {
    workflows.push(await compileRexRap(missionPhaseSource(draft, phase), true));
  }

  const bundle = pipelineBundle(draft);
  const archive = createZip([
    { name: "workflows.json", content: JSON.stringify({ workflows, triggers: [] }) },
    { name: "pipelines.json", content: JSON.stringify(bundle) },
  ]);
  const result = await importPackArchive(await archive.arrayBuffer(), true);
  const saved = result.pipelines.find(
    (pipeline) => pipeline.key === draft.key && pipeline.namespace === draft.namespace,
  );

  if (!saved) {
    throw new Error("The recipe was imported but its pipeline was not returned.");
  }

  return saved;
}

export function missionPhaseSource(draft: MissionRecipeDraft, phase: MissionPhaseDraft): string {
  const path = phasePath(draft, phase.id);
  const variable = "phase_result";
  const parameters: JsonRecord = { ...phase.actionParameters };

  if (phase.promptParameter) {
    parameters[phase.promptParameter] =
      phase.routeMode === "result"
        ? `${phase.prompt}\nReturn only JSON with next_member set to one of: ${phase.allowedNextPhases.map((id) => phasePath(draft, id)).join(", ")}.`
        : phase.prompt;
  }

  if (phase.provider === "ai-command" && phase.action === "claude_code") {
    parameters.harnessed ??= true;
    parameters.role ??= phase.role;
    parameters.mission_mcp ??= true;
    parameters.mission_id ??= "=params.orchestration.binding_id";
  }

  const args = (Object.entries(parameters) as [string, JsonValue][])
    .map(([name, value]) => `            ${name}: ${expression(value)}`)
    .join(",\n");
  const annotations = [
    phase.workspace ? "        @workspace(params.orchestration.workspace_affinity)" : "",
    `        @timeout(${String(Math.trunc(phase.timeoutSeconds))}s)`,
    phase.profile ? `        @profile(${JSON.stringify(phase.profile)})` : "",
  ]
    .filter(Boolean)
    .join("\n");
  const outcome = outcomeSource(draft, phase, variable);
  return `language rexrap-1\n\nnamespace ${draft.namespace} {\nworkflow ${JSON.stringify(phase.name)} v1 {\n    params ${inputTypeSource(draft.inputs)}\n    key ${draft.key}_${phase.id}\n\n    do {\n${annotations}\n        let ${variable} = ${phase.provider}.${phase.action}(\n${args}\n        )\n\n        compute {\n${outcome}\n        }\n    }\n}\n}\n// ${path}\n`;
}

function outcomeSource(
  draft: MissionRecipeDraft,
  phase: MissionPhaseDraft,
  variable: string,
): string {
  const fields: string[] = [];

  if (phase.retainEvidence) {
    fields.push(`evidence: { role: ${JSON.stringify(phase.role)}, response: ${variable} }`);
    fields.push(`resources_patch: { ${phase.id}: { latest: ${variable} } }`);
  }

  if (phase.routeMode === "fixed") {
    fields.push(`next_member: ${JSON.stringify(phasePath(draft, phase.nextPhase))}`);
  }

  if (phase.routeMode === "result") {
    const accessor = pointerAccessor(variable, phase.responseTextPointer);
    return `            let decision: { next_member: string } = std.encoding.parse_json(${accessor})\n            return { ${fields.join(", ")}${fields.length ? ", " : ""}next_member: decision.next_member }`;
  }

  return `            return { ${fields.join(", ")} }`;
}

function pipelineBundle(draft: MissionRecipeDraft): PipelineBundle {
  const phases = Object.fromEntries(
    draft.phases.map((phase) => [
      phasePath(draft, phase.id),
      {
        result: {
          ...(phase.retainEvidence
            ? { resources_patch: "/resources_patch", evidence: "/evidence" }
            : {}),
          ...(phase.routeMode === "terminal" ? {} : { next_member: "/next_member" }),
        },
        ...(phase.workspace
          ? {
              workspace: {
                scope: draft.workspaceScope,
                reuse: true,
                lease_seconds: draft.workspaceLeaseSeconds,
                recovery: "wait",
              },
            }
          : {}),
      },
    ]),
  );
  return {
    pipelines: [
      {
        name: draft.name,
        key: draft.key,
        namespace: draft.namespace,
        description: draft.description,
        defaults: {},
        members: draft.phases.map((phase) => ({ name: phasePath(draft, phase.id) })),
        links: [],
        joins: [],
        concurrency: { max_concurrent_runs: 0, on_conflict: "allow" },
        metadata: {
          ingress: {
            scope: `mission.${draft.key}`,
            routes: [
              { event_type: "start", lifecycle: "unbound", action: "start", predicates: [] },
              {
                event_type: "cancel",
                lifecycle: "active",
                action: "dispatch",
                intent: "cancel",
                predicates: [],
              },
              {
                event_type: "pause",
                lifecycle: "active",
                action: "dispatch",
                intent: "pause",
                predicates: [],
              },
              {
                event_type: "resume",
                lifecycle: "active",
                action: "dispatch",
                intent: "resume",
                predicates: [],
              },
              { event_type: "requeue", lifecycle: "terminal", action: "requeue", predicates: [] },
            ],
          },
          orchestration: {
            entry_member: phasePath(draft, draft.phases[0].id),
            max_epochs: draft.maxEpochs,
            intents: {
              cancel: { effect: "terminate", priority: 100 },
              pause: {
                effect: "suspend",
                priority: 80,
                stop: "pause",
                restart: { kind: "current" },
              },
              resume: { effect: "resume", priority: 70, restart: { kind: "current" } },
            },
            phases,
            budgets: {},
            defaults: {},
          },
          mission_authoring: structuredClone(draft),
        },
        triggers: [],
      },
    ],
  };
}

function inputTypeSource(inputs: MissionInputFieldDraft[]): string {
  const root: Record<string, unknown> = {};

  for (const input of inputs) {
    insertType(root, input.path.split("."), input);
  }

  insertType(root, ["mission", "correlation_key"], field("", "", "", "string"));
  insertType(root, ["mission", "requested_by"], field("", "", "", "string"));
  root.orchestration = { binding_id: "string", workspace_affinity: "any", resources: "any" };
  return typeObject(root);
}

function insertType(
  target: Record<string, unknown>,
  path: string[],
  input: MissionInputFieldDraft,
): void {
  const [head, ...tail] = path;

  if (!head) {
    return;
  }

  if (!tail.length) {
    target[`${head}${input.required ? "" : "?"}`] = input.kind;
    return;
  }

  const next = (target[head] && typeof target[head] === "object" ? target[head] : {}) as Record<
    string,
    unknown
  >;
  target[head] = next;
  insertType(next, tail, input);
}

function typeObject(value: Record<string, unknown>): string {
  return `{ ${Object.entries(value)
    .map(
      ([key, nested]) =>
        `${key}: ${typeof nested === "string" ? nested : typeObject(nested as Record<string, unknown>)}`,
    )
    .join(", ")} }`;
}

function expression(value: JsonValue): string {
  return typeof value === "string" && value.startsWith("=")
    ? value.slice(1)
    : JSON.stringify(value);
}

function pointerAccessor(variable: string, pointer: string): string {
  return pointer
    .split("/")
    .filter(Boolean)
    .reduce((value, part) => `${value}.${part}`, variable);
}

function phasePath(draft: MissionRecipeDraft, id: string): string {
  return `${draft.namespace}.${draft.key}_${id}`;
}

function identifier(value: string): boolean {
  return /^[A-Za-z_][A-Za-z0-9_]*$/.test(value);
}

function pathsOverlap(left: string, right: string): boolean {
  return left === right || left.startsWith(`${right}.`) || right.startsWith(`${left}.`);
}

function reachablePhaseIds(phases: MissionPhaseDraft[]): Set<string> {
  const byId = new Map(phases.map((phase) => [phase.id, phase]));
  const reachable = new Set<string>();
  const pending = phases[0] ? [phases[0].id] : [];

  while (pending.length) {
    const id = pending.pop();

    if (!id || reachable.has(id)) {
      continue;
    }

    reachable.add(id);
    const phase = byId.get(id);

    if (!phase || phase.routeMode === "terminal") {
      continue;
    }

    const targets = phase.routeMode === "fixed" ? [phase.nextPhase] : phase.allowedNextPhases;
    pending.push(...targets.filter((target) => byId.has(target)));
  }

  return reachable;
}

function field(
  path: string,
  label: string,
  description: string,
  kind: MissionInputKind,
): MissionInputFieldDraft {
  return { path, label, description, kind, required: true };
}

function recipe(
  preset: MissionPresetId,
  name: string,
  key: string,
  description: string,
  maxEpochs: number,
  phases: MissionPhaseDraft[],
  inputs: MissionInputFieldDraft[] = SOURCE_INPUTS,
): MissionRecipeDraft {
  return {
    schemaVersion: 1,
    preset,
    name,
    key,
    namespace: "runinator.missions",
    description,
    maxEpochs,
    workspaceScope: "mission-source",
    workspaceLeaseSeconds: 7200,
    inputs: structuredClone(inputs),
    phases,
  };
}

function basePhase(id: string, role: string, prompt: string): MissionPhaseDraft {
  return {
    id,
    name: `${role} phase`,
    role,
    provider: "ai-command",
    action: "claude_code",
    promptParameter: "prompt",
    profile: "claude",
    prompt,
    timeoutSeconds: 3600,
    actionParameters: { max_turns: 6, permission_mode: "dontAsk" },
    workspace: true,
    retainEvidence: true,
    routeMode: "terminal",
    nextPhase: "",
    allowedNextPhases: [],
    responseTextPointer: "/response/result",
  };
}

function agentPhase(id: string, role: string, prompt: string, next: string): MissionPhaseDraft {
  return { ...basePhase(id, role, prompt), routeMode: "fixed", nextPhase: next };
}

function decisionPhase(
  id: string,
  role: string,
  prompt: string,
  allowed: string[],
): MissionPhaseDraft {
  return { ...basePhase(id, role, prompt), routeMode: "result", allowedNextPhases: allowed };
}

function terminalPhase(id: string, role: string, prompt: string): MissionPhaseDraft {
  return basePhase(id, role, prompt);
}

function preparePhase(id: string, next: string): MissionPhaseDraft {
  return {
    ...basePhase(id, "Source", ""),
    name: "Prepare source",
    provider: "git",
    action: "prepare_checkout",
    promptParameter: "",
    profile: "",
    timeoutSeconds: 900,
    actionParameters: {
      repository: "=params.mission.source.repository",
      revision: "=params.mission.source.revision",
    },
    routeMode: "fixed",
    nextPhase: next,
    responseTextPointer: "",
  };
}
