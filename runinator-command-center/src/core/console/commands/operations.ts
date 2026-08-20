// `:status`, `:approvals …`, `:providers …`, `:artifacts …` — the small operational verbs.

import {
  approveApproval,
  fetchAllArtifacts,
  fetchApprovals,
  fetchWorkflowEffectOutput,
  fetchProviders,
  fetchSupervisorStatus,
  fetchWorkflowRuns,
  fetchWorkflows,
  rejectApproval,
} from "../../api/commandCenterApi";
import type { JsonRecord } from "../../domain/json";
import { cell, done, json, table, text, time, truncate } from "../format";
import { flag, parseJson, requiredArg } from "../options";
import type { ConsoleCommand } from "../types";
import { UnavailableCommandError } from "../types";

// statuses a run can still leave on its own, counted by `:status`.
const OPEN_STATUSES = [
  "queued",
  "running",
  "paused",
  "debug_paused",
  "waiting",
  "approval_required",
  "blocked",
];

export const operationCommands: ConsoleCommand[] = [
  {
    path: ["status"],
    usage: "status",
    summary: "show api, supervisor, and active-run health",
    run: async ({ json: raw, print }) => {
      const workflows = await fetchWorkflows();
      const runs = await fetchWorkflowRuns();
      const supervisor = await fetchSupervisorStatus().catch(() => null);
      const counts = OPEN_STATUSES.map((status) => ({
        status,
        runs: runs.filter((run) => run.status === status).length,
      }));

      if (raw) {
        print(
          json({
            api: { reachable: true, workflow_count: workflows.length },
            supervisor: supervisor as unknown as JsonRecord,
            workflow_runs: Object.fromEntries(counts.map(({ status, runs }) => [status, runs])),
          }),
        );
        return;
      }

      print(text(`api: reachable`, "success"));
      print(text(`workflows: ${String(workflows.length)}`));
      print(
        text(
          supervisor?.configured
            ? `supervisor: configured, stale_seconds=${cell(supervisor.stale_seconds)}`
            : "supervisor: unavailable",
        ),
      );
      print(
        table(
          ["status", "runs"],
          counts.map(({ status, runs }) => [status, String(runs)]),
        ),
      );
    },
  },
  {
    path: ["approvals", "list"],
    usage: "approvals list [--workflow-run-id ID] [--open]",
    summary: "list approval requests",
    booleans: ["open"],
    run: async ({ flags, json: raw, print }) => {
      let approvals = await fetchApprovals(flag(flags, "workflow-run-id"));

      if (flags.open !== undefined) {
        approvals = approvals.filter((approval) => approval.status === "pending");
      }

      if (raw) {
        print(json(approvals));
        return;
      }

      print(
        table(
          ["id", "status", "run", "node", "prompt"],
          approvals.map((approval) => [
            cell(approval.id),
            cell(approval.status),
            truncate(approval.workflow_run_id, 14),
            truncate(approval.node_id, 24),
            truncate(approval.prompt, 48),
          ]),
        ),
      );
    },
  },
  resolution("approve", approveApproval),
  resolution("reject", rejectApproval),
  {
    path: ["providers", "list"],
    usage: "providers list",
    summary: "list providers and how many actions each advertises",
    run: async ({ json: raw, print }) => {
      const providers = await fetchProviders();

      if (raw) {
        print(json(providers));
        return;
      }

      print(
        table(
          ["name", "actions", "credential_scopes"],
          providers.map((provider) => [
            truncate(provider.name, 28),
            String(provider.actions.length),
            cell(provider.metadata.credential_scopes.join(",")),
          ]),
        ),
      );
    },
  },
  {
    path: ["providers", "show"],
    usage: "providers show <name>",
    summary: "show one provider's actions and their parameters",
    run: async ({ args, json: raw, print }) => {
      const name = requiredArg(args, 0, "provider name");
      const provider = (await fetchProviders()).find((candidate) => candidate.name === name);

      if (!provider) {
        throw new Error(`provider '${name}' not found`);
      }

      if (raw) {
        print(json(provider));
        return;
      }

      print(text(`name: ${provider.name}`));
      print(
        table(
          ["action", "parameters"],
          provider.actions.map((action) => [
            truncate(action.function_name, 32),
            action.parameters
              .map((parameter) => (parameter.required ? `${parameter.name}*` : parameter.name))
              .join(","),
          ]),
        ),
      );
    },
  },
  {
    path: ["artifacts", "list"],
    usage: "artifacts list [--effect ID]",
    summary: "list artifacts, all of them or one workflow effect's",
    run: async ({ flags, json: raw, print }) => {
      const effect = flag(flags, "effect");
      const artifacts = effect
        ? (await fetchWorkflowEffectOutput(effect))
            .filter((event) => event.output.type === "artifact")
            .map((event) =>
              event.output.type === "artifact" ? (event.output.artifact as JsonRecord) : {},
            )
        : await fetchAllArtifacts();

      if (raw) {
        print(json(artifacts));
        return;
      }

      print(
        table(
          ["id", "name", "mime_type", "size", "created"],
          artifacts.map((artifact) => [
            cell(artifact.id),
            truncate(artifact.name, 36),
            cell(artifact.mime_type),
            cell(artifact.size_bytes),
            time(artifact.created_at),
          ]),
        ),
      );
    },
  },
  {
    path: ["artifacts", "download"],
    usage: "artifacts download <id> [--out PATH]",
    summary: "download an artifact to a file (runinatorctl only)",
    run: () => {
      throw new UnavailableCommandError(
        "artifacts download",
        "it writes to a path on disk; run it with runinatorctl, or use the Artifacts tab",
      );
    },
  },
];

// approve and reject differ only in which endpoint they call, so they are one shape.
function resolution(
  verb: string,
  operation: (
    approvalId: string,
    resolution: { by?: string | null; message?: string | null; output?: unknown },
  ) => Promise<unknown>,
): ConsoleCommand {
  return {
    path: ["approvals", verb],
    usage: `approvals ${verb} <approval-id> [--by NAME] [--message TEXT] [--output JSON]`,
    summary: `${verb} an approval request`,
    run: async ({ args, flags, print }) => {
      const approvalId = requiredArg(args, 0, "approval id");
      const output = flag(flags, "output");
      await operation(approvalId, {
        by: flag(flags, "by") ?? null,
        message: flag(flags, "message") ?? null,
        output: output === undefined ? null : parseJson(output, "--output"),
      });
      print(done(`${verb}d approval ${approvalId}`));
    },
  };
}
