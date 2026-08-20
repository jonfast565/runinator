// `:workflows …` — the workflow definitions themselves.

import {
  createWorkflowRun,
  duplicateWorkflow,
  exportWorkflowBundle,
  fetchWorkflowRevision,
  fetchWorkflowRevisions,
  fetchWorkflows,
  restoreWorkflowRevision,
} from "../../api/commandCenterApi";
import { cell, done, json, table, text, truncate } from "../format";
import { resolveWorkflow, resolveWorkflowId } from "../lookup";
import { flag, keyValueFlags, requiredArg } from "../options";
import type { ConsoleCommand } from "../types";
import { UnavailableCommandError } from "../types";

export const workflowCommands: ConsoleCommand[] = [
  {
    path: ["workflows", "list"],
    usage: "workflows list",
    summary: "list workflow definitions",
    run: async ({ json: raw, print }) => {
      const workflows = await fetchWorkflows();

      if (raw) {
        print(json(workflows));
        return;
      }

      print(
        table(
          ["id", "name", "version", "enabled"],
          workflows.map((workflow) => [
            cell(workflow.id),
            truncate(workflow.name, 40),
            cell(workflow.version),
            cell(workflow.enabled),
          ]),
        ),
      );
    },
  },
  {
    path: ["workflows", "show"],
    usage: "workflows show <workflow>",
    summary: "show one workflow by id or name",
    run: async ({ args, print }) => {
      const workflow = await resolveWorkflow(requiredArg(args, 0, "workflow"));
      print(json(workflow));
    },
  },
  {
    path: ["workflows", "run"],
    usage: "workflows run <workflow> [--param KEY=VALUE] [--debug] [--name NAME]",
    summary: "create a workflow run",
    booleans: ["debug"],
    run: async ({ args, flags, print }) => {
      const workflowId = await resolveWorkflowId(requiredArg(args, 0, "workflow"));
      const created = await createWorkflowRun(workflowId, {
        debug: flags.debug !== undefined,
        parameters: keyValueFlags(flags, "param"),
      });
      print(text(`workflow run ${cell(created.id)}`));
      print(json(created));
    },
  },
  {
    path: ["workflows", "export"],
    usage: "workflows export [workflow]",
    summary: "export one workflow, or the whole bundle",
    run: async ({ args, print }) => {
      const reference = args[0];
      const workflowId = reference ? await resolveWorkflowId(reference) : undefined;
      print(json(await exportWorkflowBundle(workflowId)));
    },
  },
  {
    path: ["workflows", "revisions"],
    usage: "workflows revisions <workflow> [--limit N]",
    summary: "list a workflow's revision history, newest first",
    run: async ({ args, flags, json: raw, print }) => {
      const workflowId = await resolveWorkflowId(requiredArg(args, 0, "workflow"));
      const limit = Number(flag(flags, "limit") ?? 20);
      const revisions = await fetchWorkflowRevisions(workflowId, limit);

      if (raw) {
        print(json(revisions));
        return;
      }

      print(
        table(
          ["rev", "version", "source", "name", "author"],
          revisions.map((revision) => [
            cell(revision.revision),
            cell(revision.version),
            cell(revision.source),
            truncate(revision.name, 32),
            truncate(revision.actor_id ?? revision.actor_kind, 28),
          ]),
        ),
      );
    },
  },
  {
    path: ["workflows", "revision"],
    usage: "workflows revision <workflow> <revision>",
    summary: "show one revision, including the definition it captured",
    run: async ({ args, print }) => {
      const workflowId = await resolveWorkflowId(requiredArg(args, 0, "workflow"));
      const revision = Number(requiredArg(args, 1, "revision"));
      print(json(await fetchWorkflowRevision(workflowId, revision)));
    },
  },
  {
    path: ["workflows", "rollback"],
    usage: "workflows rollback <workflow> <revision>",
    summary: "restore an earlier revision as the current definition",
    run: async ({ args, print }) => {
      const workflowId = await resolveWorkflowId(requiredArg(args, 0, "workflow"));
      const revision = Number(requiredArg(args, 1, "revision"));
      const restored = await restoreWorkflowRevision(workflowId, revision);
      print(done(`restored revision ${String(revision)} as ${restored.name} v${restored.version}`));
    },
  },
  {
    path: ["workflows", "duplicate"],
    usage: "workflows duplicate <workflow> [--bump major|minor|patch]",
    summary: "duplicate a workflow into a new version sharing its name",
    run: async ({ args, flags, print }) => {
      const workflowId = await resolveWorkflowId(requiredArg(args, 0, "workflow"));
      const bump = (flag(flags, "bump") ?? "minor") as "major" | "minor" | "patch";

      if (!["major", "minor", "patch"].includes(bump)) {
        throw new Error("--bump takes major, minor, or patch");
      }

      const duplicate = await duplicateWorkflow(workflowId, bump);
      print(done(`created ${duplicate.name} v${duplicate.version}`));
    },
  },
  // the pack commands read a working tree, which a browser tab does not have. they stay in the
  // catalog so `:help` explains where to run them rather than pretending they do not exist.
  ...localOnly([
    ["apply", "workflows apply [path]", "import a pack from disk"],
    ["validate", "workflows validate <file>", "validate a workflow definition json file"],
    ["test", "workflows test <file> [--tests PATH]", "dry-run a pack against .rexrapt suites"],
    ["dev", "workflows dev [path] [--run WORKFLOW]", "watch a pack and re-apply it on change"],
  ]),
];

function localOnly(entries: [string, string, string][]): ConsoleCommand[] {
  return entries.map(([name, usage, summary]) => ({
    path: ["workflows", name],
    usage,
    summary: `${summary} (runinatorctl only)`,
    run: () => {
      throw new UnavailableCommandError(
        `workflows ${name}`,
        "it reads files from disk; run it with runinatorctl",
      );
    },
  }));
}
