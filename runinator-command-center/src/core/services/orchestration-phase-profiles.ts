import { asJsonRecord, asJsonValue, type JsonRecord } from "../domain/json";
import type {
  OrchestrationPolicy,
  PhasePolicy,
  ResultMapping,
  WorkspacePolicy,
} from "../domain/models";

export const resultMappingKeys = [
  "subject_revision",
  "resources",
  "evidence",
  "failure_class",
  "correlations",
  "resources_patch",
  "next_member",
] as const;

export type ResultMappingKey = (typeof resultMappingKeys)[number];

export interface ResultMappingProfile {
  id: string;
  name: string;
  mapping: ResultMapping;
}

export interface WorkspacePolicyProfile {
  id: string;
  name: string;
  policy: WorkspacePolicy;
}

export interface PhaseProfileAssignment {
  result_mapping_id?: string;
  workspace_policy_id?: string;
}

export interface PhasePolicyProfiles {
  result_mappings: ResultMappingProfile[];
  workspace_policies: WorkspacePolicyProfile[];
  assignments: Record<string, PhaseProfileAssignment>;
}

export function loadPhasePolicyProfiles(
  members: string[],
  phases: OrchestrationPolicy["phases"],
  authoring: unknown,
): PhasePolicyProfiles {
  const saved = parseSavedProfiles(members, authoring);

  if (saved && phasesMatch(members, phases, saved)) {
    return saved;
  }

  return derivePhasePolicyProfiles(members, phases);
}

export function derivePhasePolicyProfiles(
  members: string[],
  phases: OrchestrationPolicy["phases"],
): PhasePolicyProfiles {
  const result_mappings: ResultMappingProfile[] = [];
  const workspace_policies: WorkspacePolicyProfile[] = [];
  const assignments: Record<string, PhaseProfileAssignment> = {};
  const resultIds = new Map<string, string>();
  const workspaceIds = new Map<string, string>();

  for (const member of members) {
    const phase = normalizePhase(phases[member]);
    const assignment: PhaseProfileAssignment = {};

    if (Object.keys(phase.result).length) {
      const key = stable(phase.result);
      let id = resultIds.get(key);

      if (!id) {
        id = `result-${String(result_mappings.length + 1)}`;
        resultIds.set(key, id);
        result_mappings.push({
          id,
          name: `Mapping ${String(result_mappings.length + 1)}`,
          mapping: phase.result,
        });
      }

      assignment.result_mapping_id = id;
    }

    if (phase.workspace) {
      const key = stable(phase.workspace);
      let id = workspaceIds.get(key);

      if (!id) {
        id = `workspace-${String(workspace_policies.length + 1)}`;
        workspaceIds.set(key, id);
        workspace_policies.push({
          id,
          name: `Workspace ${String(workspace_policies.length + 1)}`,
          policy: phase.workspace,
        });
      }

      assignment.workspace_policy_id = id;
    }

    assignments[member] = assignment;
  }

  return { result_mappings, workspace_policies, assignments };
}

export function expandPhasePolicyProfiles(
  members: string[],
  profiles: PhasePolicyProfiles,
): OrchestrationPolicy["phases"] {
  const results = new Map(profiles.result_mappings.map((profile) => [profile.id, profile.mapping]));
  const workspaces = new Map(
    profiles.workspace_policies.map((profile) => [profile.id, profile.policy]),
  );

  return Object.fromEntries(
    members.map((member) => {
      const assignment = profiles.assignments[member] ?? {};
      const result = assignment.result_mapping_id
        ? results.get(assignment.result_mapping_id)
        : undefined;
      const workspace = assignment.workspace_policy_id
        ? workspaces.get(assignment.workspace_policy_id)
        : undefined;

      return [
        member,
        {
          result: normalizeResult(result),
          ...(workspace ? { workspace: normalizeWorkspace(workspace) } : {}),
        },
      ];
    }),
  );
}

export function phasePolicyProfilesMetadata(profiles: PhasePolicyProfiles): JsonRecord {
  return {
    result_mappings: profiles.result_mappings.map((profile) => ({
      id: profile.id,
      name: profile.name,
      mapping: normalizeResult(profile.mapping),
    })),
    workspace_policies: profiles.workspace_policies.map((profile) => ({
      id: profile.id,
      name: profile.name,
      policy: normalizeWorkspace(profile.policy),
    })),
    assignments: Object.fromEntries(
      Object.entries(profiles.assignments).map(([member, assignment]) => [
        member,
        {
          ...(assignment.result_mapping_id
            ? { result_mapping_id: assignment.result_mapping_id }
            : {}),
          ...(assignment.workspace_policy_id
            ? { workspace_policy_id: assignment.workspace_policy_id }
            : {}),
        },
      ]),
    ),
  };
}

function parseSavedProfiles(members: string[], authoring: unknown): PhasePolicyProfiles | null {
  const authoringRecord = asJsonRecord(authoring);

  if (authoringRecord.schema_version !== 2) {
    return null;
  }

  const root = asJsonRecord(authoringRecord.phase_profiles);
  const rawResults = root.result_mappings;
  const rawWorkspaces = root.workspace_policies;
  const rawAssignments = asJsonRecord(root.assignments);

  if (!Array.isArray(rawResults) || !Array.isArray(rawWorkspaces)) {
    return null;
  }

  const result_mappings: ResultMappingProfile[] = [];
  const workspace_policies: WorkspacePolicyProfile[] = [];

  for (const value of rawResults) {
    const record = asJsonRecord(value);
    const mapping = parseResult(record.mapping);

    if (typeof record.id !== "string" || typeof record.name !== "string" || !mapping) {
      return null;
    }

    result_mappings.push({ id: record.id, name: record.name, mapping });
  }

  for (const value of rawWorkspaces) {
    const record = asJsonRecord(value);
    const policy = parseWorkspace(record.policy);

    if (typeof record.id !== "string" || typeof record.name !== "string" || !policy) {
      return null;
    }

    workspace_policies.push({ id: record.id, name: record.name, policy });
  }

  if (hasDuplicateIds(result_mappings) || hasDuplicateIds(workspace_policies)) {
    return null;
  }

  const resultIds = new Set(result_mappings.map((profile) => profile.id));
  const workspaceIds = new Set(workspace_policies.map((profile) => profile.id));
  const assignments: Record<string, PhaseProfileAssignment> = {};

  for (const member of members) {
    const record = asJsonRecord(rawAssignments[member]);
    const resultId = record.result_mapping_id;
    const workspaceId = record.workspace_policy_id;

    if (
      (resultId !== undefined && (typeof resultId !== "string" || !resultIds.has(resultId))) ||
      (workspaceId !== undefined &&
        (typeof workspaceId !== "string" || !workspaceIds.has(workspaceId)))
    ) {
      return null;
    }

    assignments[member] = {
      ...(typeof resultId === "string" ? { result_mapping_id: resultId } : {}),
      ...(typeof workspaceId === "string" ? { workspace_policy_id: workspaceId } : {}),
    };
  }

  return { result_mappings, workspace_policies, assignments };
}

function phasesMatch(
  members: string[],
  phases: OrchestrationPolicy["phases"],
  profiles: PhasePolicyProfiles,
): boolean {
  const expected = Object.fromEntries(
    members.map((member) => [member, normalizePhase(phases[member])]),
  );
  return stable(expected) === stable(expandPhasePolicyProfiles(members, profiles));
}

function parseResult(value: unknown): ResultMapping | null {
  const record = asJsonRecord(value);
  const mapping: ResultMapping = {};

  for (const key of resultMappingKeys) {
    const pointer = record[key];

    if (pointer === undefined || pointer === null) {
      continue;
    }

    if (typeof pointer !== "string") {
      return null;
    }

    mapping[key] = pointer;
  }

  return mapping;
}

function parseWorkspace(value: unknown): WorkspacePolicy | null {
  const record = asJsonRecord(value);

  if (
    typeof record.scope !== "string" ||
    typeof record.lease_seconds !== "number" ||
    typeof record.reuse !== "boolean" ||
    !["replace", "wait", "fail"].includes(String(record.recovery))
  ) {
    return null;
  }

  return {
    scope: record.scope,
    requirements: asJsonValue(record.requirements ?? {}),
    lease_seconds: record.lease_seconds,
    reuse: record.reuse,
    recovery: record.recovery as WorkspacePolicy["recovery"],
  };
}

function normalizePhase(phase?: PhasePolicy): PhasePolicy {
  return {
    result: normalizeResult(phase?.result),
    ...(phase?.workspace ? { workspace: normalizeWorkspace(phase.workspace) } : {}),
  };
}

function normalizeResult(result?: ResultMapping): ResultMapping {
  return Object.fromEntries(
    resultMappingKeys.flatMap((key) => {
      const value = result?.[key];
      return typeof value === "string" ? [[key, value]] : [];
    }),
  );
}

function normalizeWorkspace(policy: WorkspacePolicy): WorkspacePolicy {
  return {
    scope: policy.scope,
    requirements: policy.requirements ?? {},
    lease_seconds: policy.lease_seconds,
    reuse: policy.reuse,
    recovery: policy.recovery,
  };
}

function hasDuplicateIds(profiles: { id: string }[]): boolean {
  return new Set(profiles.map((profile) => profile.id)).size !== profiles.length;
}

function stable(value: unknown): string {
  if (Array.isArray(value)) {
    return `[${value.map(stable).join(",")}]`;
  }

  if (value && typeof value === "object") {
    return `{${Object.entries(value)
      .sort(([left], [right]) => left.localeCompare(right))
      .map(([key, item]) => `${JSON.stringify(key)}:${stable(item)}`)
      .join(",")}}`;
  }

  return JSON.stringify(value);
}
