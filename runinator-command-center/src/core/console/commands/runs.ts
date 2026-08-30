// `:runs …` — driving and observing what is running.

import {
  cancelWorkflowRun,
  fetchWorkflowEffectOutput,
  fetchWorkflowRun,
  fetchWorkflowRunArtifacts,
  fetchWorkflowRuns,
  pauseWorkflowRun,
  renameWorkflowRun,
  replayWorkflowRun,
  resumeWorkflowRun,
} from "../../api/commandCenterApi";
import type { RunSummary, WorkflowRunDetail } from "../../domain/models";
import { cell, done, json, table, text, time, truncate } from "../format";
import { isUuid, resolveWorkflowId } from "../lookup";
import { flag, requiredArg } from "../options";
import type { ConsoleCommand, ConsoleCommandContext, ConsoleOutput } from "../types";
import { delay } from "../wait";

// statuses a run can still leave on its own. `--open` is the union of them, which is what makes
// "what is in flight right now" one command rather than seven.
const OPEN_STATUSES = [
  "queued",
  "running",
  "paused",
  "debug_paused",
  "waiting",
  "parked",
  "sleeping",
  "approval_required",
  "input_required",
  "blocked",
];

// how often `runs watch` re-reads. matches runinatorctl's default.
const WATCH_INTERVAL_MS = 2000;

export const runCommands: ConsoleCommand[] = [
  {
    path: ["runs", "list"],
    usage: "runs list [--status STATUS] [--workflow WORKFLOW] [--open]",
    summary: "list recent or filtered workflow runs",
    booleans: ["open"],
    run: async ({ flags, json: raw, print }) => {
      const workflow = flag(flags, "workflow");
      const workflowId = workflow
        ? isUuid(workflow)
          ? workflow
          : await resolveWorkflowId(workflow)
        : undefined;
      let runs = await fetchWorkflowRuns(workflowId);
      const status = flag(flags, "status");

      if (status) {
        runs = runs.filter((run) => run.status === status);
      } else if (flags.open !== undefined) {
        runs = runs.filter((run) => OPEN_STATUSES.includes(run.status));
      }

      if (raw) {
        print(json(runs));
        return;
      }

      print(runTable(runs));
    },
  },
  {
    path: ["runs", "show"],
    usage: "runs show <run-id>",
    summary: "show a workflow run with its VM continuations and effects",
    run: async ({ args, json: raw, print }) => {
      const detail = await fetchWorkflowRun(requiredArg(args, 0, "run id"));

      if (raw) {
        print(json(detail));
        return;
      }

      printRunDetail(detail, print);
    },
  },
  {
    path: ["runs", "watch"],
    usage: "runs watch <run-id>",
    summary: "re-read a run until it settles or the command is stopped",
    run: async ({ args, print, signal }) => {
      const runId = requiredArg(args, 0, "run id");
      let previous = "";

      while (!signal.aborted) {
        const detail = await fetchWorkflowRun(runId);
        const stamp = `${detail.run.status}:${cell(detail.run.active_node_id)}`;

        // only the changes are printed: a watch that reprinted an unchanged run every two seconds
        // would bury the transition that matters.
        if (stamp !== previous) {
          previous = stamp;
          printRunDetail(detail, print);
        }

        if (isTerminal(detail.run.status)) {
          return;
        }

        await delay(WATCH_INTERVAL_MS, signal);
      }
    },
  },
  {
    path: ["runs", "logs"],
    usage: "runs logs <effect-id>",
    summary: "print streamed chunks for a workflow effect",
    run: async ({ args, json: raw, print }) => {
      const output = await fetchWorkflowEffectOutput(requiredArg(args, 0, "effect id"));
      const chunks = output.filter((event) => event.output.type === "chunk");

      if (raw) {
        print(json(chunks));
        return;
      }

      if (chunks.length === 0) {
        print(
          text(
            "No streamed output was recorded. Provider stdout/stderr appears here only when the worker emits output chunks.",
            "muted",
          ),
        );
        return;
      }

      print(
        text(
          chunks
            .flatMap((event) => {
              if (event.output.type !== "chunk") {
                return [];
              }

              const timestamp = new Date(event.created_at * 1000);
              const stamp = Number.isNaN(timestamp.getTime())
                ? String(event.created_at)
                : timestamp.toISOString();
              const prefix = `${stamp} · ${event.output.stream} · attempt ${String(event.attempt)} · effect ${event.effect_id.slice(0, 8)}`;
              return event.output.content
                .replaceAll("\r\n", "\n")
                .split("\n")
                .map((line) => `${prefix} ${line}`);
            })
            .join("\n"),
        ),
      );
    },
  },
  {
    path: ["runs", "artifacts"],
    usage: "runs artifacts <run-id>",
    summary: "list the run-level artifacts a workflow run produced",
    run: async ({ args, json: raw, print }) => {
      const artifacts = await fetchWorkflowRunArtifacts(requiredArg(args, 0, "run id"));

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
  control("pause", "pause a workflow run", pauseWorkflowRun),
  control("resume", "resume a workflow run", resumeWorkflowRun),
  control("cancel", "cancel a workflow run", cancelWorkflowRun),
  {
    path: ["runs", "replay"],
    usage: "runs replay <run-id> [--from-step STEP]",
    summary: "replay a workflow run",
    run: async ({ args, flags, print }) => {
      const created = await replayWorkflowRun(requiredArg(args, 0, "run id"), {
        fromStepId: flag(flags, "from-step"),
      });
      print(done(`replayed as run ${created.id}`));
    },
  },
  {
    path: ["runs", "rename"],
    usage: "runs rename <run-id> [name]",
    summary: "rename a workflow run, or clear its name",
    run: async ({ args, print }) => {
      const runId = requiredArg(args, 0, "run id");
      const name = args[1] ?? null;
      await renameWorkflowRun(runId, name === "" ? null : name);
      print(done(name ? `renamed run ${runId} to ${name}` : `cleared the name of run ${runId}`));
    },
  },
];

function control(
  name: string,
  summary: string,
  operation: (runId: string) => Promise<{ message?: string }>,
): ConsoleCommand {
  return {
    path: ["runs", name],
    usage: `runs ${name} <run-id>`,
    summary,
    run: async ({ args, print }: ConsoleCommandContext) => {
      const runId = requiredArg(args, 0, "run id");
      const response = await operation(runId);
      print(done(response.message ?? `${name}d run ${runId}`));
    },
  };
}

function runTable(runs: RunSummary[]): ConsoleOutput {
  return table(
    ["id", "status", "workflow", "active_node", "created"],
    runs.map((run) => [
      cell(run.id),
      cell(run.status),
      truncate(run.workflow_id, 14),
      truncate(run.active_node_id, 22),
      time(run.created_at),
    ]),
  );
}

function printRunDetail(detail: WorkflowRunDetail, print: (output: ConsoleOutput) => void) {
  print(
    text(
      `run ${detail.run.id} status=${detail.run.status} active_node=${cell(detail.run.active_node_id)}`,
    ),
  );

  if (detail.run.message) {
    print(text(detail.run.message, "muted"));
  }

  print(
    table(
      ["id", "node_id", "status", "attempt", "message"],
      detail.nodes.map((node) => [
        cell(node.id),
        truncate(node.node_id, 28),
        cell(node.status),
        cell(node.attempt),
        truncate(node.message, 48),
      ]),
    ),
  );
}

function isTerminal(status: string): boolean {
  return !OPEN_STATUSES.includes(status);
}
