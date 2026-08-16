// `:settings …` — the unified settings store: secrets and config.

import {
  deleteCredential,
  fetchCredential,
  fetchCredentials,
  saveCredential,
} from "../../api/commandCenterApi";
import type { SettingKind } from "../../domain/models";
import { cell, done, json, table, text } from "../format";
import { flag, parseJson, requiredArg } from "../options";
import type { ConsoleCommand, ConsoleFlags } from "../types";
import { UnavailableCommandError } from "../types";

export const settingsCommands: ConsoleCommand[] = [
  {
    path: ["settings", "list"],
    usage: "settings list [--kind secret|config]",
    summary: "list stored settings without their values",
    run: async ({ flags, json: raw, print }) => {
      const kind = flag(flags, "kind");
      let entries = await fetchCredentials();

      if (kind) {
        entries = entries.filter((entry) => (entry.kind ?? "secret") === kind);
      }

      if (raw) {
        print(json(entries));
        return;
      }

      print(
        table(
          ["kind", "scope", "name"],
          entries.map((entry) => [cell(entry.kind ?? "secret"), entry.scope, entry.name]),
        ),
      );
    },
  },
  {
    path: ["settings", "get"],
    usage: "settings get <scope> <name> [--kind secret|config]",
    summary: "read a setting; config returns json, a secret returns its stored string",
    run: async ({ args, flags, print }) => {
      const detail = await fetchCredential(
        requiredArg(args, 0, "scope"),
        requiredArg(args, 1, "name"),
        settingKind(flags),
      );
      const value = detail.value ?? detail.secret ?? null;
      print(typeof value === "string" ? text(value) : json(value));
    },
  },
  {
    path: ["settings", "set"],
    usage: "settings set <scope> <name> <value> [--kind secret|config] [--schema JSON]",
    summary: "store a setting; a config value is json and a secret is stored verbatim",
    run: async ({ args, flags, print }) => {
      const scope = requiredArg(args, 0, "scope");
      const name = requiredArg(args, 1, "name");
      const raw = requiredArg(args, 2, "value");
      const kind = settingKind(flags);
      // a config value is json and validated against its slot's schema; a secret is a string, and
      // parsing it would turn a password that happens to look like a number into one.
      const value = kind === "config" ? parseJson(raw, "value") : raw;
      const schema = flag(flags, "schema");
      await saveCredential(
        scope,
        name,
        value,
        kind,
        schema === undefined ? undefined : parseJson(schema, "--schema"),
      );
      print(done(`stored ${kind} ${scope}/${name}`));
    },
  },
  {
    path: ["settings", "delete"],
    usage: "settings delete <scope> <name> [--kind secret|config]",
    summary: "delete a setting",
    run: async ({ args, flags, print }) => {
      const scope = requiredArg(args, 0, "scope");
      const name = requiredArg(args, 1, "name");
      const kind = settingKind(flags);
      await deleteCredential(scope, name, kind);
      print(done(`deleted ${kind} ${scope}/${name}`));
    },
  },
  {
    path: ["settings", "import"],
    usage: "settings import <file.wdls>",
    summary: "import a .wdls secrets file (runinatorctl only)",
    run: () => {
      throw new UnavailableCommandError(
        "settings import",
        "it reads a .wdls file from disk; run it with runinatorctl",
      );
    },
  },
];

function settingKind(flags: ConsoleFlags): SettingKind {
  const kind = flag(flags, "kind") ?? "secret";

  if (kind !== "secret" && kind !== "config") {
    throw new Error("--kind takes secret or config");
  }

  return kind;
}
