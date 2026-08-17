// running one `:` line.

import { ConsoleParseError } from "./tokenize";
import { parseArguments } from "./options";
import { matchCommand, nearestCommand } from "./registry";
import { tokenize } from "./tokenize";
import { acceptedFlags, switchFlags, usageShape } from "./usage";
import type { ConsoleCommand, ConsoleFlags } from "./types";
import type { ConsoleOutput, ConsoleSessionPort, ConsoleTerminalPort } from "./types";

export interface ConsoleExecution {
  session: ConsoleSessionPort;
  terminal: ConsoleTerminalPort;
  signal: AbortSignal;
  print(output: ConsoleOutput): void;
}

/// execute a command line, with the leading `:` already stripped.
///
/// nothing here catches: a failure is the caller's to render, because the terminal is what knows
/// which transcript entry it belongs to.
export async function executeCommand(line: string, execution: ConsoleExecution): Promise<void> {
  const tokens = tokenize(line);

  if (tokens.length === 0) {
    return;
  }

  const match = matchCommand(tokens);

  if (!match) {
    throw new ConsoleParseError(unknownCommand(tokens[0]));
  }

  // the usage line already says what this command accepts, so it is what a switch is recognised
  // from and what an unrecognised flag is reported against.
  const { command } = match;
  const booleans = [...(command.booleans ?? []), ...switchFlags(command.usage, command.path)];
  const { args, flags } = parseArguments(match.rest, booleans);
  check(command, flags);

  await command.run({
    args,
    flags,
    json: flags.json !== undefined,
    signal: execution.signal,
    session: execution.session,
    terminal: execution.terminal,
    print: (output) => {
      execution.print(output);
    },
  });
}

// a mistyped flag used to be ignored in silence, which made `--stauts failed` read as "every run".
// it is now the failure it always was.
function check(command: ConsoleCommand, flags: ConsoleFlags) {
  const accepted = acceptedFlags(command.usage, command.path);
  const name = command.path.join(" ");

  for (const flag of Object.keys(flags)) {
    if (!accepted.includes(flag)) {
      throw new ConsoleParseError(
        `:${name} does not take --${flag}; it takes ${accepted.map((value) => `--${value}`).join(", ")}`,
      );
    }
  }

  for (const declared of usageShape(command.usage, command.path).flags) {
    const given = flags[declared.name];

    if (!declared.values || !Array.isArray(given)) {
      continue;
    }

    for (const value of given) {
      if (!declared.values.includes(value)) {
        throw new ConsoleParseError(
          `--${declared.name} takes ${declared.values.join(" or ")}, not '${value}'`,
        );
      }
    }
  }
}

function unknownCommand(word: string): string {
  const nearest = nearestCommand(word);
  return nearest
    ? `unknown console command '${word}'; did you mean ':${nearest}'? try :help`
    : `unknown console command '${word}'; try :help`;
}
