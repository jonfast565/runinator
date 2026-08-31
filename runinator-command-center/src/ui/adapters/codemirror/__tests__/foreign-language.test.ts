import { syntaxTree } from "@codemirror/language";
import { EditorState } from "@codemirror/state";
import { describe, expect, it } from "vitest";
import { canonicalForeignLanguage, loadForeignLanguageExtension } from "../foreign-language";

describe("foreign do language highlighting", () => {
  it.each([
    ["py", "python"],
    ["node", "javascript"],
    ["sh", "bash"],
    ["common-lisp", "commonlisp"],
    ["sbcl", "commonlisp"],
    ["gnucobol", "cobol"],
    ["golang", "go"],
    ["pwsh", "powershell"],
    ["c#", "csharp"],
    ["f#", "fsharp"],
    ["vb.net", "vbnet"],
  ])("canonicalizes %s to %s", (input, expected) => {
    expect(canonicalForeignLanguage(input)).toBe(expected);
  });

  it.each([
    "python",
    "javascript",
    "bash",
    "commonlisp",
    "cobol",
    "ruby",
    "perl",
    "php",
    "go",
    "swift",
    "powershell",
    "csharp",
    "fsharp",
    "vbnet",
  ] as const)("installs the %s parser", async (language) => {
    const source = language === "php" ? "<?php function main($context) { return 1; }" : "main";
    const extension = await loadForeignLanguageExtension(language);
    const state = EditorState.create({
      doc: source,
      extensions: [extension],
    });

    const tree = syntaxTree(state);
    expect(tree.length).toBeGreaterThan(0);
    expect(tree.topNode.type.isError).toBe(false);
  });
});
