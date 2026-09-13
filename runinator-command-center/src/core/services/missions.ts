import type {
  IngressResponse,
  JsonRecord,
  OrchestrationBinding,
  OrchestrationEpoch,
  Pipeline,
  WorkflowEffect,
  WorkflowEffectOutputEvent,
} from "../domain/models";
import { asJsonRecord } from "../domain/json";
import {
  admitPipelineIngress,
  fetchOrchestrations,
  fetchOrchestration,
  fetchOrchestrationEpochs,
  fetchOrchestrationEvidence,
  fetchPipelines,
  fetchPipelineRun,
  fetchWorkflowEffectOutput,
  fetchWorkflowEffects,
  steerMission,
  sendOrchestrationIntent,
} from "../api/commandCenterApi";
export interface StartMissionInput {
  pipelineId: string;
  correlationKey: string;
  parameters: JsonRecord;
}

export interface MissionEffectActivity {
  effect: WorkflowEffect;
  output: WorkflowEffectOutputEvent[];
}

export function isMissionPipeline(pipeline: Pipeline): boolean {
  const ingress = asJsonRecord(pipeline.metadata.ingress);
  const orchestration = asJsonRecord(pipeline.metadata.orchestration);
  return (
    typeof ingress.scope === "string" &&
    ingress.scope.startsWith("mission.") &&
    typeof orchestration.entry_member === "string"
  );
}

export function isMission(binding: OrchestrationBinding): boolean {
  return binding.scope.startsWith("mission.");
}

export async function fetchMissionPipelines(): Promise<Pipeline[]> {
  return (await fetchPipelines()).filter(isMissionPipeline);
}

export async function fetchMissions(
  filters: Record<string, unknown> = {},
): Promise<OrchestrationBinding[]> {
  return (await fetchOrchestrations({ ...filters, scope_prefix: "mission." })).filter(isMission);
}

export async function startMission(input: StartMissionInput): Promise<IngressResponse> {
  const pipeline = (await fetchMissionPipelines()).find(
    (candidate) => candidate.id === input.pipelineId,
  );

  if (!pipeline) {
    throw new Error("The selected mission recipe is no longer available.");
  }

  const mission: JsonRecord = {
    ...asJsonRecord(input.parameters.mission),
    correlation_key: input.correlationKey,
    requested_by: "command_center",
  };

  if (
    !pipeline.metadata.mission_authoring &&
    typeof asJsonRecord(input.parameters.mission).kind !== "string"
  ) {
    mission.kind =
      asJsonRecord(pipeline.metadata.ingress)
        .scope?.toString()
        .replace(/^mission\./, "") ?? "mission";
  }

  return admitPipelineIngress(input.pipelineId, {
    source: "runinator.command_center",
    eventId: crypto.randomUUID(),
    eventType: "start",
    correlationKey: input.correlationKey,
    payload: { ...input.parameters, mission },
    provenance: {
      origin: "command_center",
      mission_scope: asJsonRecord(pipeline.metadata.ingress).scope,
    },
  });
}

export async function fetchMissionDetail(missionId: string) {
  const [binding, epochs, evidence] = await Promise.all([
    fetchOrchestration(missionId),
    fetchOrchestrationEpochs(missionId),
    fetchOrchestrationEvidence(missionId),
  ]);

  if (!isMission(binding)) {
    throw new Error("The selected orchestration is not a mission.");
  }

  const effects = await fetchCurrentMissionEffects(binding, epochs);
  return { binding, epochs, evidence, effects };
}

export async function sendMissionIntent(
  missionId: string,
  intent: string,
  reason: string,
  payload: unknown = {},
): Promise<void> {
  await sendOrchestrationIntent(missionId, intent, reason, payload);
}

export async function sendMissionSteering(missionId: string, message: string): Promise<void> {
  if (!message.trim()) {
    throw new Error("Mission steering requires a non-empty message.");
  }

  await steerMission(missionId, message);
}

async function fetchCurrentMissionEffects(
  binding: OrchestrationBinding,
  epochs: OrchestrationEpoch[],
): Promise<MissionEffectActivity[]> {
  const epoch = epochs.find((candidate) => candidate.epoch === binding.current_epoch);

  if (!epoch?.pipeline_run_id) {
    return [];
  }

  const pipeline = await fetchPipelineRun(epoch.pipeline_run_id).catch(() => null);
  const attempt = pipeline?.attempts
    .filter(
      (candidate) =>
        candidate.member_key === binding.current_phase && candidate.workflow_run_id !== null,
    )
    .sort((left, right) => right.attempt - left.attempt)[0];

  if (!attempt?.workflow_run_id) {
    return [];
  }

  const effects = await fetchWorkflowEffects(attempt.workflow_run_id).catch(() => []);
  return Promise.all(
    effects
      .filter((effect) => !isTerminalEffect(effect.status) && isHarnessedClaudeEffect(effect))
      .map(async (effect) => ({
        effect,
        output: await fetchWorkflowEffectOutput(effect.id).catch(() => []),
      })),
  );
}

function isTerminalEffect(status: string): boolean {
  return ["succeeded", "failed", "rejected", "timed_out", "canceled"].includes(status);
}

function isHarnessedClaudeEffect(effect: WorkflowEffect): boolean {
  const request = asJsonRecord(effect.request);
  const input = asJsonRecord(request.input);
  return (
    request.type === "action" &&
    request.provider === "ai-command" &&
    request.function === "claude_code" &&
    input.harnessed === true
  );
}
