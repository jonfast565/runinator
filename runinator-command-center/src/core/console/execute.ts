// running one `:` line.

import { COMMANDS } from "./registry";
import type { ConsoleOutput, ConsoleSessionPort, ConsoleTerminalPort } from "./types";
import { ctlFlags, ctlParse } from "./wasm-engine";

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
  const parsed = ctlParse(line);

  if (parsed.kind === "empty") {
    return;
  }

  const name = parsed.path.join(" ");
  const command = COMMANDS.find((candidate) => candidate.path.join(" ") === name);

  if (!command) {
    throw new Error(`:${name} is not available in Command Center`);
  }

  const flags = ctlFlags(parsed);

  await command.run({
    args: parsed.args,
    flags,
    json: parsed.json,
    signal: execution.signal,
    session: execution.session,
    terminal: execution.terminal,
    print: (output) => {
      execution.print(output);
    },
  });
}
