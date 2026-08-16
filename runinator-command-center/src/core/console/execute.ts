// running one `:` line.

import { parseArguments } from "./options";
import { matchCommand } from "./registry";
import { tokenize } from "./tokenize";
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
    throw new Error(`unknown console command '${tokens.join(" ")}'; try :help`);
  }

  const { args, flags } = parseArguments(match.rest, match.command.booleans);
  await match.command.run({
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
