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
import { wdlCommands } from "./commands/wdl";
import { workflowCommands } from "./commands/workflows";
import type { ConsoleCommand } from "./types";

const helpCommand: ConsoleCommand = {
  path: ["help"],
  usage: "help [command]",
  summary: "list the commands, or show one command's arguments",
  run: ({ args, print }) => {
    const topic = args.join(" ");

    if (topic) {
      const matches = COMMANDS.filter((command) => command.path.join(" ").startsWith(topic));

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

    print(text("a bare line is WDL; a `:` line is a command. `:help <command>` for arguments."));
    print(
      table(
        ["command", "what it does"],
        COMMANDS.map((command) => [`:${command.path.join(" ")}`, command.summary]),
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
  ...wdlCommands,
  ...nodeCommands,
  ...orgCommands,
  ...replicaCommands,
  ...agentCommands,
];

/// the command a line selects, and the arguments left over.
export interface CommandMatch {
  command: ConsoleCommand;
  rest: string[];
}

/// match the longest command path that prefixes the tokens.
///
/// longest-first is what lets `runs list` and `run workflow` coexist with a bare `run`: a shorter
/// path never shadows a longer one that also matches.
export function matchCommand(tokens: string[]): CommandMatch | null {
  let best: CommandMatch | null = null;

  for (const command of COMMANDS) {
    const matches = command.path.every((word, index) => tokens[index] === word);

    if (matches && (!best || command.path.length > best.command.path.length)) {
      best = { command, rest: tokens.slice(command.path.length) };
    }
  }

  return best;
}

/// the first word closest to `word`, when it is close enough to be a typo rather than a different
/// word entirely.
export function nearestCommand(word: string): string | null {
  const limit = 1 + Math.floor(word.length / 3);
  const ranked = [...new Set(COMMANDS.map((command) => command.path[0]))]
    .map((candidate) => ({ candidate, distance: editDistance(word, candidate) }))
    .filter((entry) => entry.distance <= limit)
    .sort((left, right) => left.distance - right.distance || left.candidate.localeCompare(right.candidate));

  return ranked.at(0)?.candidate ?? null;
}

function editDistance(left: string, right: string): number {
  let previous = [...Array(right.length + 1).keys()];

  for (let row = 0; row < left.length; row += 1) {
    const current = [row + 1];

    for (let column = 0; column < right.length; column += 1) {
      const cost = left[row] === right[column] ? 0 : 1;
      current.push(Math.min(previous[column] + cost, previous[column + 1] + 1, current[column] + 1));
    }

    previous = current;
  }

  return previous[right.length];
}

/// the words that may follow what has been typed, for tab completion.
export function completions(tokens: string[], prefix: string): string[] {
  const offered = new Set<string>();

  for (const command of COMMANDS) {
    const prefixes = command.path.slice(0, tokens.length);
    const alignsSoFar = prefixes.every((word, index) => tokens[index] === word);
    const next = command.path.at(tokens.length);

    if (alignsSoFar && next?.startsWith(prefix)) {
      offered.add(next);
    }
  }

  return [...offered].sort();
}
