// the notebook verbs: sessions, the durable cell history, the scope, and starting a run by hand.
//
// these are the commands that only mean something inside a console session, which is why they have
// no `runinatorctl` subcommand to defer to — they are the console's own vocabulary.

import { createPipelineRun, createWorkflowRun, retryPipelineMember } from "../../api/commandCenterApi";
import { cellReference } from "../../domain/models";
import { cell, done, json, table, text, time, truncate } from "../format";
import { resolvePipeline, resolveWorkflowId } from "../lookup";
import { keyValueFlags, parseJson, requiredArg } from "../options";
import type { ConsoleCell } from "../../domain/models";
import type { ConsoleCommand, ConsoleOutput, ConsoleSessionPort } from "../types";

export const sessionCommands: ConsoleCommand[] = [
  {
    path: ["sessions"],
    usage: "sessions",
    summary: "list your console sessions",
    run: async ({ session, json: raw, print }) => {
      await session.refresh();
      const sessions = session.list();

      if (raw) {
        print(json(sessions));
        return;
      }

      const activeId = session.current()?.id;
      print(
        table(
          ["", "id", "name", "updated"],
          sessions.map((entry) => [
            entry.id === activeId ? "*" : " ",
            truncate(entry.id, 14),
            truncate(entry.name, 28),
            time(entry.updated_at),
          ]),
        ),
      );
    },
  },
  {
    path: ["new"],
    usage: "new [name]",
    summary: "create a session and switch to it",
    run: async ({ args, session, print }) => {
      const created = await session.create(args[0]);
      print(done(`using session ${created.name} (${created.id})`));
    },
  },
  {
    path: ["use"],
    usage: "use <name|id>",
    summary: "switch sessions",
    run: async ({ args, session, print }) => {
      const reference = requiredArg(args, 0, "session name or id");
      await session.refresh();
      const found = session
        .list()
        .find((entry) => entry.id === reference || entry.name === reference);

      if (!found) {
        throw new Error(`console session '${reference}' not found`);
      }

      await session.open(found.id);
      print(done(`using session ${found.name} (${found.id})`));
    },
  },
  {
    path: ["history"],
    usage: "history",
    summary: "show the durable cells in this session",
    run: ({ session, json: raw, print }) => {
      const cells = session.cells();

      if (raw) {
        print(json(cells));
        return;
      }

      print(
        table(
          ["#", "status", "binds", "source"],
          cells.map((entry) => [
            String(entry.position),
            entry.status,
            cellReference(entry),
            truncate(entry.source.split(/\s+/).join(" "), 56),
          ]),
        ),
      );
    },
  },
  {
    path: ["bindings"],
    usage: "bindings",
    summary: "show what this session's scope resolves to",
    run: ({ session, json: raw, print }) => {
      const bindings = session.current()?.bindings ?? [];

      if (raw) {
        print(json(bindings));
        return;
      }

      if (bindings.length === 0) {
        print(text("empty scope", "muted"));
        return;
      }

      print(
        table(
          ["name", "value"],
          [...bindings]
            .sort((left, right) => left.name.localeCompare(right.name))
            .map((binding) => [binding.name, truncate(binding.value, 72)]),
        ),
      );
    },
  },
  cellCommand(
    "cancel",
    "cancel the durable run behind an effectful cell",
    async (session, cell) => {
      await session.cancelCell(cell.id);
      return done(`canceled cell ${String(cell.position)}`);
    },
  ),
  cellCommand(
    "replay",
    "run a settled cell again against the current scope",
    async (session, cell) => {
      const replayed = await session.replayCell(cell.id);
      return text(`[${replayed.status}] cell ${String(replayed.position)}`);
    },
  ),
  {
    path: ["run", "workflow"],
    usage: "run workflow <workflow> [--param KEY=VALUE] [--debug] | with <json>",
    summary: "start a workflow run",
    booleans: ["debug"],
    run: async ({ args, flags, print }) => {
      const workflowId = await resolveWorkflowId(requiredArg(args, 0, "workflow"));
      const created = await createWorkflowRun(workflowId, {
        debug: flags.debug !== undefined,
        parameters: parameters(args, flags),
      });
      print(done(`workflow run ${created.id}`));
    },
  },
  {
    path: ["run", "pipeline"],
    usage: "run pipeline <pipeline> [--param KEY=VALUE] | with <json>",
    summary: "start a pipeline run",
    run: async ({ args, flags, print }) => {
      const pipeline = await resolvePipeline(requiredArg(args, 0, "pipeline"));

      if (!pipeline.id) {
        throw new Error(`pipeline '${pipeline.name}' has no id`);
      }

      const created = await createPipelineRun(pipeline.id, parameters(args, flags));
      print(done(`pipeline run ${cell(created.id)}`));
    },
  },
  {
    path: ["pipelines", "retry"],
    usage: "pipelines retry <run-id> <member-key> [--param KEY=VALUE] | with <json>",
    summary: "retry a failed frontier member in the same pipeline run",
    run: async ({ args, flags, print }) => {
      const runId = requiredArg(args, 0, "pipeline run id");
      const memberKey = requiredArg(args, 1, "member key");
      const attempt = await retryPipelineMember(runId, memberKey, parameters(args, flags));
      print(done(`pipeline member ${attempt.member_key} retry #${String(attempt.attempt)}`));
    },
  },
];

// `:cancel` and `:replay` both act on a cell: the one named by id or position, else the last one,
// which is what makes a bare `:cancel` mean "the thing i just started".
function cellCommand(
  name: string,
  summary: string,
  act: (session: ConsoleSessionPort, cell: ConsoleCell) => Promise<ConsoleOutput>,
): ConsoleCommand {
  return {
    path: [name],
    usage: `${name} [cell-id|position]`,
    summary,
    run: async ({ args, session, print }) => {
      const cells = session.cells();
      const reference = args[0];
      const chosen = reference
        ? cells.find((cell) => cell.id === reference || String(cell.position) === reference)
        : cells.at(-1);

      if (!chosen) {
        throw new Error(reference ? `no cell '${reference}'` : "this session has no cells yet");
      }

      print(await act(session, chosen));
    },
  };
}

// run parameters, from either spelling: `--param k=v` pairs, or the terminal console's
// `… with {"k": "v"}` tail, which is what `runinatorctl`'s repl accepts.
function parameters(args: string[], flags: Parameters<typeof keyValueFlags>[0]): unknown {
  const separator = args.indexOf("with");

  if (separator >= 0) {
    return parseJson(args.slice(separator + 1).join(" "), "the `with` payload");
  }

  return keyValueFlags(flags, "param");
}
