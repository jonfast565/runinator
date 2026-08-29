// covers how flags are pulled off a command line and read back.

import { describe, expect, it } from "vitest";
import { ConsoleParseError, flag, flagList, keyValueFlags, numberFlag, requiredArg } from "../options";

describe("reading flags", () => {
  it("parses KEY=VALUE pairs, json where it parses", () => {
    const flags = { param: ["count=3", "name=daily"] };
    expect(keyValueFlags(flags, "param")).toEqual({ count: 3, name: "daily" });
    expect(flag(flags, "param")).toBe("name=daily");
    expect(flagList(flags, "param")).toEqual(["count=3", "name=daily"]);
  });

  it("rejects a KEY=VALUE without a key", () => {
    const flags = { param: ["=3"] };
    expect(() => keyValueFlags(flags, "param")).toThrow(ConsoleParseError);
  });

  it("rejects a numeric flag that is not a number", () => {
    const flags = { limit: ["soon"] };
    expect(() => numberFlag(flags, "limit")).toThrow(ConsoleParseError);
  });

  it("names the missing positional", () => {
    expect(() => requiredArg([], 0, "workflow")).toThrow(/workflow is required/);
  });
});
