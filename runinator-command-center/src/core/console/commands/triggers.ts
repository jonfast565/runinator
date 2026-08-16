// `:triggers …` and `:freeze …` — what fires, and what stops it firing.

import {
  backfillWorkflowTrigger,
  createFreezeWindow,
  createTriggerRun,
  deleteFreezeWindow,
  fetchDueTriggers,
  fetchFreezeWindows,
  fetchWorkflowTriggers,
} from "../../api/commandCenterApi";
import type { WorkflowTrigger } from "../../domain/models";
import { cell, done, json, table, text, time, truncate } from "../format";
import { resolveWorkflowId } from "../lookup";
import { flag, keyValueFlags, numberFlag, requiredArg, requiredFlag } from "../options";
import type { ConsoleCommand, ConsoleOutput } from "../types";

export const triggerCommands: ConsoleCommand[] = [
  {
    path: ["triggers", "list"],
    usage: "triggers list <workflow>",
    summary: "list a workflow's triggers",
    run: async ({ args, json: raw, print }) => {
      const workflowId = await resolveWorkflowId(requiredArg(args, 0, "workflow"));
      const triggers = await fetchWorkflowTriggers(workflowId);
      print(raw ? json(triggers) : triggerTable(triggers));
    },
  },
  {
    path: ["triggers", "due"],
    usage: "triggers due",
    summary: "list triggers due for execution",
    run: async ({ json: raw, print }) => {
      const triggers = await fetchDueTriggers();
      print(raw ? json(triggers) : triggerTable(triggers));
    },
  },
  {
    path: ["triggers", "run"],
    usage: "triggers run <trigger-id> [--param KEY=VALUE] [--debug]",
    summary: "create a run from a trigger",
    booleans: ["debug"],
    run: async ({ args, flags, print }) => {
      const created = await createTriggerRun(
        requiredArg(args, 0, "trigger id"),
        keyValueFlags(flags, "param"),
        flags.debug !== undefined,
      );
      print(json(created));
    },
  },
  {
    path: ["triggers", "backfill"],
    usage: "triggers backfill <trigger-id> --from RFC3339 [--to RFC3339] [--limit N] [--dry-run]",
    summary: "replay a cron trigger's slots across a past range",
    booleans: ["dry-run"],
    run: async ({ args, flags, print }) => {
      const response = await backfillWorkflowTrigger(requiredArg(args, 0, "trigger id"), {
        from: requiredFlag(flags, "from"),
        // the range is exclusive of `from` and inclusive of `to`; a missing `to` means "up to now",
        // which is what runinatorctl defaults to.
        to: flag(flags, "to") ?? new Date().toISOString(),
        limit: numberFlag(flags, "limit") ?? null,
        dry_run: flags["dry-run"] !== undefined,
      });
      print(json(response));
    },
  },
];

export const freezeCommands: ConsoleCommand[] = [
  {
    path: ["freeze", "list"],
    usage: "freeze list [--active]",
    summary: "list freeze windows",
    booleans: ["active"],
    run: async ({ flags, json: raw, print }) => {
      const windows = await fetchFreezeWindows(flags.active !== undefined);

      if (raw) {
        print(json(windows));
        return;
      }

      print(
        table(
          ["id", "name", "from", "to", "scope"],
          windows.map((window) => [
            cell(window.id),
            truncate(window.name, 28),
            time(window.starts_at),
            time(window.ends_at),
            window.workflow_id ? `workflow ${truncate(window.workflow_id, 12)}` : "platform",
          ]),
        ),
      );
    },
  },
  {
    path: ["freeze", "create"],
    usage: "freeze create <name> --from RFC3339 --to RFC3339 [--workflow-id ID] [--reason TEXT]",
    summary: "suspend trigger firing over a time range",
    run: async ({ args, flags, print }) => {
      const created = await createFreezeWindow({
        name: requiredArg(args, 0, "name"),
        starts_at: requiredFlag(flags, "from"),
        ends_at: requiredFlag(flags, "to"),
        workflow_id: flag(flags, "workflow-id") ?? null,
        org_id: flag(flags, "org-id") ?? null,
        reason: flag(flags, "reason") ?? null,
        enabled: true,
      });
      print(done(`created freeze window ${cell(created.id)}`));
    },
  },
  {
    path: ["freeze", "delete"],
    usage: "freeze delete <window-id>",
    summary: "remove a freeze window",
    run: async ({ args, print }) => {
      const windowId = requiredArg(args, 0, "window id");
      await deleteFreezeWindow(windowId);
      print(done(`deleted freeze window ${windowId}`));
    },
  },
];

function triggerTable(triggers: WorkflowTrigger[]): ConsoleOutput {
  if (triggers.length === 0) {
    return text("no triggers", "muted");
  }

  return table(
    ["id", "workflow", "kind", "enabled", "next_execution"],
    triggers.map((trigger) => [
      cell(trigger.id),
      truncate(trigger.workflow_id, 14),
      cell(trigger.kind),
      cell(trigger.enabled),
      time(trigger.next_execution),
    ]),
  );
}
