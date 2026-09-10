import { defaultApi, type ConsoleApi } from "../api/ports/console";
// every command the console answers, and the two that describe the console itself.

import { done, table, text } from "./format";
import { createWorkspacesCommands } from "./commands/workspaces";
import { createFunctionsCommands } from "./commands/functions";
import { createInfrastructureCommands } from "./commands/infrastructure";
import { createOperationsCommands } from "./commands/operations";
import { createRunsCommands } from "./commands/runs";
import { createSessionCommands } from "./commands/session";
import { createSettingsCommands } from "./commands/settings";
import { createTriggersCommands } from "./commands/triggers";
import { createRexrapCommands } from "./commands/rexrap";
import { createWorkflowsCommands } from "./commands/workflows";
import type { ConsoleCommand } from "./types";
import { ctlCatalog } from "./wasm-engine";

const helpCommand: ConsoleCommand = {
  path: ["help"],
  usage: "help [command]",
  summary: "list the commands, or show one command's arguments",
  run: ({ args, print }) => {
    const catalog = ctlCatalog();
    const topic = args.join(" ");

    if (topic) {
      const matches = catalog.filter((command) => command.path.join(" ").startsWith(topic));

      if (matches.length === 0) {
        throw new Error(`no console command '${topic}'`);
      }

      print(
        table(
          ["usage", "what it does"],
          matches.map((command) => [command.usage, command.summary]),
        ),
      );
      return;
    }

    print(text("a bare line is REXRAP; a `:` line is a command. `:help <command>` for arguments."));
    print(
      table(
        ["command", "what it does"],
        catalog.map((command) => [`:${command.path.join(" ")}`, command.summary]),
      ),
    );
  },
};

const clearCommand: ConsoleCommand = {
  path: ["clear"],
  usage: "clear",
  summary: "clear the screen; the session's cells and scope are untouched",
  run: ({ terminal, print }) => {
    terminal.clear();
    print(done("cleared"));
  },
};

/// every console command, in the order `:help` lists them.
export function createCommands(api: ConsoleApi = defaultApi): ConsoleCommand[] {
  const { functionCommands } = createFunctionsCommands(api);
  const { nodeCommands, replicaCommands, orgCommands, agentCommands } =
    createInfrastructureCommands(api);
  const { operationCommands } = createOperationsCommands(api);
  const { rexrapCommands } = createRexrapCommands(api);
  const { runCommands } = createRunsCommands(api);
  const { sessionCommands } = createSessionCommands(api);
  const { settingsCommands } = createSettingsCommands(api);
  const { triggerCommands, freezeCommands } = createTriggersCommands(api);
  const { workflowCommands } = createWorkflowsCommands(api);
  const { workspaceCommands } = createWorkspacesCommands(api);
  return [
    helpCommand,
    clearCommand,
    ...sessionCommands,
    ...operationCommands,
    ...workflowCommands,
    ...runCommands,
    ...triggerCommands,
    ...freezeCommands,
    ...functionCommands,
    ...workspaceCommands,
    ...settingsCommands,
    ...rexrapCommands,
    ...nodeCommands,
    ...orgCommands,
    ...replicaCommands,
    ...agentCommands,
  ];
}

export const COMMANDS = createCommands();
