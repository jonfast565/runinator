// every command the console answers, and the two that describe the console itself.

import { done, table, text } from "./format";
import { functionCommands } from "./commands/functions";
import {
  agentCommands,
  nodeCommands,
  orgCommands,
  replicaCommands,
} from "./commands/infrastructure";
import { operationCommands } from "./commands/operations";
import { runCommands } from "./commands/runs";
import { sessionCommands } from "./commands/session";
import { settingsCommands } from "./commands/settings";
import { freezeCommands, triggerCommands } from "./commands/triggers";
import { rexrapCommands } from "./commands/rexrap";
import { workflowCommands } from "./commands/workflows";
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
export const COMMANDS: ConsoleCommand[] = [
  helpCommand,
  clearCommand,
  ...sessionCommands,
  ...operationCommands,
  ...workflowCommands,
  ...runCommands,
  ...triggerCommands,
  ...freezeCommands,
  ...functionCommands,
  ...settingsCommands,
  ...rexrapCommands,
  ...nodeCommands,
  ...orgCommands,
  ...replicaCommands,
  ...agentCommands,
];
