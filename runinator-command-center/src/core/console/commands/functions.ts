// `:functions …` and `:invoke …` — published packaged functions.

import {
  deleteFunctionAlias,
  deleteFunctionPackage,
  fetchFunctionCatalog,
  fetchFunctionPackage,
  fetchFunctionPackages,
  invokeFunction,
  setFunctionAlias,
} from "../../api/commandCenterApi";
import type { JsonRecord } from "../../domain/json";
import { cell, done, json, table, text, time, truncate } from "../format";
import { flag, numberFlag, parseJson, requiredArg } from "../options";
import type { ConsoleCommand } from "../types";
import { UnavailableCommandError } from "../types";

export const functionCommands: ConsoleCommand[] = [
  {
    path: ["functions", "list"],
    usage: "functions list",
    summary: "list published packages",
    run: async ({ json: raw, print }) => {
      const packages = await fetchFunctionPackages();

      if (raw) {
        print(json(packages));
        return;
      }

      print(
        table(
          ["name", "namespace", "latest", "updated"],
          packages.map((entry) => [
            truncate(entry.name, 32),
            cell(entry.namespace),
            cell(entry.latest_version),
            time(entry.updated_at),
          ]),
        ),
      );
    },
  },
  {
    path: ["functions", "show"],
    usage: "functions show <package>",
    summary: "show one package with its versions, aliases, and exports",
    run: async ({ args, json: raw, print }) => {
      const detail = await fetchFunctionPackage(requiredArg(args, 0, "package"));

      if (raw) {
        print(json(detail));
        return;
      }

      print(text(`${detail.name} (latest version ${cell(detail.latest_version)})`));
      print(
        table(
          ["alias", "version"],
          (detail.aliases ?? []).map((alias) => [alias.name, String(alias.version)]),
        ),
      );
      print(
        table(
          ["export", "handler", "description"],
          (detail.exports ?? []).map((entry) => [
            truncate(entry.name, 28),
            truncate(entry.handler, 32),
            truncate(entry.description, 48),
          ]),
        ),
      );
    },
  },
  {
    path: ["functions", "versions"],
    usage: "functions versions <package>",
    summary: "list a package's versions",
    run: async ({ args, json: raw, print }) => {
      const detail = await fetchFunctionPackage(requiredArg(args, 0, "package"));
      const versions = detail.versions ?? [];

      if (raw) {
        print(json(versions));
        return;
      }

      print(
        table(
          ["version", "digest", "runtime", "published"],
          versions.map((version) => [
            String(version.version),
            truncate(version.artifact_digest, 24),
            cell(version.runtime.runtime),
            time(version.created_at),
          ]),
        ),
      );
    },
  },
  {
    path: ["functions", "catalog"],
    usage: "functions catalog",
    summary: "list every published export as a catalog entry",
    run: async ({ json: raw, print }) => {
      const catalog = await fetchFunctionCatalog();

      if (raw) {
        print(json(catalog));
        return;
      }

      print(
        table(
          ["package", "export", "version", "aliases"],
          catalog.map((entry) => [
            truncate(entry.package_name, 28),
            truncate(entry.export_name, 28),
            String(entry.version),
            cell(entry.aliases?.join(",")),
          ]),
        ),
      );
    },
  },
  {
    path: ["functions", "alias"],
    usage: "functions alias <package> <alias> [--version N] [--from ALIAS]",
    summary: "point an alias at a version",
    run: async ({ args, flags, print }) => {
      const packageName = requiredArg(args, 0, "package");
      const alias = requiredArg(args, 1, "alias");
      await setFunctionAlias(packageName, alias, numberFlag(flags, "version"), flag(flags, "from"));
      print(done(`moved ${packageName}@${alias}`));
    },
  },
  {
    path: ["functions", "unalias"],
    usage: "functions unalias <package> <alias>",
    summary: "delete an alias; the version it named is untouched",
    run: async ({ args, print }) => {
      const packageName = requiredArg(args, 0, "package");
      const alias = requiredArg(args, 1, "alias");
      await deleteFunctionAlias(packageName, alias);
      print(done(`deleted alias ${packageName}@${alias}`));
    },
  },
  {
    path: ["functions", "delete"],
    usage: "functions delete <package>",
    summary: "archive a package, retaining versions pinned by workflows",
    run: async ({ args, print }) => {
      const packageName = requiredArg(args, 0, "package");
      await deleteFunctionPackage(packageName);
      print(done(`archived ${packageName}`));
    },
  },
  {
    path: ["invoke"],
    usage: "invoke <package.export> [--alias NAME | --version N] [--input JSON]",
    summary: "call a packaged function and print what it returned",
    run: async ({ args, flags, print }) => {
      const target = requiredArg(args, 0, "package.export");
      const separator = target.lastIndexOf(".");

      if (separator <= 0) {
        throw new Error("the function target must be package.export");
      }

      const input = flag(flags, "input");
      const result = await invokeFunction(
        target.slice(0, separator),
        target.slice(separator + 1),
        (input === undefined ? {} : parseJson(input, "--input")) as JsonRecord,
        { alias: flag(flags, "alias"), version: numberFlag(flags, "version") },
      );
      print(json(result));
    },
  },
  ...localOnly([
    ["validate", "functions validate <path>", "check a package directory offline"],
    ["publish", "functions publish <path> [--alias NAME]", "publish one version of a package"],
    ["restore", "functions restore <package>", "restore an archived package"],
  ]),
];

// publishing reads a working tree, and restore has no web-service binding here yet; both stay in
// the catalog so `:help` says where to run them.
function localOnly(entries: [string, string, string][]): ConsoleCommand[] {
  return entries.map(([name, usage, summary]) => ({
    path: ["functions", name],
    usage,
    summary: `${summary} (runinatorctl only)`,
    run: () => {
      throw new UnavailableCommandError(`functions ${name}`, "run it with runinatorctl");
    },
  }));
}
