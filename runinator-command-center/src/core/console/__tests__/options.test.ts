// covers how flags are pulled off a command line and read back.

import { describe, expect, it } from "vitest";
import { ConsoleParseError } from "../tokenize";
import { flag, flagList, keyValueFlags, numberFlag, parseArguments, requiredArg } from "../options";

describe("parseArguments", () => {
  it("separates positionals from flags", () => {
    const { args, flags } = parseArguments(["show", "daily", "--limit", "5"]);
    expect(args).toEqual(["show", "daily"]);
    expect(flag(flags, "limit")).toBe("5");
  });

  it("accepts --name=value", () => {
    const { flags } = parseArguments(["--status=running"]);
    expect(flag(flags, "status")).toBe("running");
  });

  it("does not let a declared boolean swallow the next word", () => {
    const { args, flags } = parseArguments(["--open", "list"], ["open"]);
    expect(args).toEqual(["list"]);
    expect(flags.open).toBe(true);
  });

  it("keeps every value of a repeated flag", () => {
    const { flags } = parseArguments(["--param", "a=1", "--param", "b=2"]);
    expect(flagList(flags, "param")).toEqual(["a=1", "b=2"]);
  });

  it("treats a trailing flag as a boolean", () => {
    const { flags } = parseArguments(["--dry-run"]);
    expect(flags["dry-run"]).toBe(true);
  });
});

describe("reading flags", () => {
  it("parses KEY=VALUE pairs, json where it parses", () => {
    const { flags } = parseArguments(["--param", "count=3", "--param", "name=daily"]);
    expect(keyValueFlags(flags, "param")).toEqual({ count: 3, name: "daily" });
  });

  it("rejects a KEY=VALUE without a key", () => {
    const { flags } = parseArguments(["--param", "=3"]);
    expect(() => keyValueFlags(flags, "param")).toThrow(ConsoleParseError);
  });

  it("rejects a numeric flag that is not a number", () => {
    const { flags } = parseArguments(["--limit", "soon"]);
    expect(() => numberFlag(flags, "limit")).toThrow(ConsoleParseError);
  });

  it("names the missing positional", () => {
    expect(() => requiredArg([], 0, "workflow")).toThrow(/workflow is required/);
  });
});
