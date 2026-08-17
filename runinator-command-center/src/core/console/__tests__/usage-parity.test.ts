// the usage line is what a command line is checked against, so a flag the code reads has to appear
// in it. this reads the command sources back and fails when one does not — the same kind of ratchet
// the web service's route-parity lint is, and for the same reason: the alternative is a flag that
// silently stops working the day validation notices it was never declared.

import { describe, expect, it } from "vitest";

import { COMMANDS } from "../registry";
import { acceptedFlags, usageShape } from "../usage";

// the sources themselves, read through vite rather than through node's fs so this stays a plain
// browser-target module like everything else under `core/`.
const sources: Record<string, string> = import.meta.glob("../commands/*.ts", {
  query: "?raw",
  import: "default",
  eager: true,
});

// flags read inside a command's own `run`. a command built by a factory reads its flags in the
// factory body instead, which is outside every command object and so is not covered here.
const READS = /(?:flag|flagList|numberFlag|flagSet|requiredFlag|keyValueFlags)\(flags,\s*"([^"]+)"/g;

interface Declaration {
  file: string;
  usage: string;
  reads: string[];
}

function declarations(): Declaration[] {
  const found: Declaration[] = [];

  for (const [file, source] of Object.entries(sources)) {
    // one chunk per command object, cut at the end of the array so trailing module helpers are not
    // attributed to the last command.
    for (const chunk of source.split(/(?=\n {2}\{\n {4}path: \[)/).slice(1)) {
      const body = chunk.split(/\n\];/)[0];
      const usage = /usage:\s*\n?\s*"([^"]+)"/.exec(body)?.[1];

      if (usage) {
        found.push({ file, usage, reads: [...body.matchAll(READS)].map((match) => match[1]) });
      }
    }
  }

  return found;
}

describe("usage lines", () => {
  it("finds the command sources", () => {
    expect(declarations().length).toBeGreaterThan(20);
  });

  it("name every flag their command reads", () => {
    const undeclared = declarations().flatMap(({ file, usage, reads }) => {
      const accepted = acceptedFlags(usage);
      return reads
        .filter((flag) => !accepted.includes(flag))
        .map((flag) => `${file}: '${usage}' reads --${flag}`);
    });

    expect(undeclared).toEqual([]);
  });

  it("are parsed into the flags and positionals they spell out", () => {
    const shape = usageShape(
      "replicas list [--kind KIND] [--status live|stale|offline] [--live]",
      ["replicas", "list"],
    );

    expect(shape.positionals).toEqual([]);
    expect(shape.flags).toEqual([
      { name: "kind", placeholder: "KIND", values: undefined },
      { name: "status", placeholder: "live|stale|offline", values: ["live", "stale", "offline"] },
      { name: "live", placeholder: undefined, values: undefined },
    ]);
  });

  it("read a positional as required only inside angle brackets", () => {
    const shape = usageShape("functions alias <package> <alias> [--version N]", [
      "functions",
      "alias",
    ]);

    expect(shape.positionals).toEqual([
      { name: "package", required: true },
      { name: "alias", required: true },
    ]);
  });

  it("cover every registered command", () => {
    for (const command of COMMANDS) {
      expect(command.usage.startsWith(command.path.join(" "))).toBe(true);
    }
  });
});
