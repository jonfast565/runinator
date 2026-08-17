// `:wdl …` — the language tools, pointed at cells instead of files.
//
// `runinatorctl wdl <verb> <file>` reads a path. a browser tab has no working tree, so the console
// takes the same verbs against the notebook: the cell you just wrote, or `--cell <n>`.

import { analyzeWdl, compileWdl, decompileToWdl, formatWdl } from "../../api/commandCenterApi";
import { done, json, table, text, truncate } from "../format";
import { resolveWorkflow } from "../lookup";
import { numberFlag, requiredArg } from "../options";
import type { ConsoleCommandContext, ConsoleCommand } from "../types";

export const wdlCommands: ConsoleCommand[] = [
  {
    path: ["wdl", "check"],
    usage: "wdl check [--cell N]",
    summary: "parse, lower, and validate a cell, printing any diagnostics",
    run: async (context) => {
      const diagnostics = await analyzeWdl(source(context));

      if (context.json) {
        context.print(json(diagnostics));
        return;
      }

      if (diagnostics.length === 0) {
        context.print(done("no diagnostics"));
        return;
      }

      context.print(
        table(
          ["severity", "line", "message"],
          diagnostics.map((diagnostic) => [
            diagnostic.severity,
            String(diagnostic.line),
            truncate(diagnostic.message, 72),
          ]),
        ),
      );
    },
  },
  {
    path: ["wdl", "format"],
    usage: "wdl format [--cell N]",
    summary: "print a cell in canonical form",
    run: async (context) => {
      context.print(text(await formatWdl(source(context))));
    },
  },
  {
    path: ["wdl", "compile"],
    usage: "wdl compile [--cell N]",
    summary: "compile a cell into a workflow definition, without saving it",
    run: async (context) => {
      const workflow = await compileWdl(source(context), false);
      context.print(json(workflow));
    },
  },
  {
    path: ["wdl", "decompile"],
    usage: "wdl decompile <workflow>",
    summary: "print a saved workflow back as wdl source",
    run: async ({ args, print }) => {
      const workflow = await resolveWorkflow(requiredArg(args, 0, "workflow"));
      print(text(await decompileToWdl(workflow)));
    },
  },
];

// the cell a language command reads: `--cell <position>`, else the last cell in the session.
function source({ flags, session }: ConsoleCommandContext): string {
  const cells = session.cells();
  const position = numberFlag(flags, "cell");
  const chosen =
    position === undefined ? cells.at(-1) : cells.find((cell) => cell.position === position);

  if (!chosen) {
    throw new Error(
      position === undefined
        ? "this session has no cells yet; write one first, or pass --cell <n>"
        : `no cell at position ${String(position)}`,
    );
  }

  return chosen.source;
}
