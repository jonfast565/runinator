// a lightweight codemirror language for the runinator wdl workflow dsl. this is a
// stream tokenizer for syntax highlighting plus keyword/snippet completion only; the
// authoritative parse/lint comes from the rust runinator-wdl compiler via analyze_wdl.

import { StreamLanguage, LanguageSupport, type StringStream } from "@codemirror/language";
import {
  completeFromList,
  snippetCompletion,
  type CompletionSource,
} from "@codemirror/autocomplete";

// keywords mirror runinator-wdl/src/wdl.pest. grouped only for readability.
const STRUCTURE = ["workflow", "input", "let", "type", "set", "meta", "name", "version"];
const CONTROL = [
  "if", "else", "for", "in", "while", "match", "when", "parallel", "race", "try", "catch",
  "finally", "map", "join", "branch", "spawn", "call", "subflow", "wait", "emit",
  "approve", "as", "until", "with", "limit", "concurrency", "initial",
];
const OUTCOMES = ["ok", "fail", "timeout", "reject", "done", "winner"];
const VALUES = ["all", "any", "first_success", "true", "false", "null", "string", "json"];
const BUILTINS = ["exists", "contains", "starts_with", "ends_with", "detached", "reuse"];

const KEYWORDS = new Set([...STRUCTURE, ...CONTROL, ...OUTCOMES, ...VALUES, ...BUILTINS]);

interface WdlState {
  inBlockComment: boolean;
}

// consume the rest of a string literal on the current line, respecting escapes. returns
// true when the closing quote was found on this line.
function consumeString(stream: StringStream): boolean {
  let escaped = false;
  while (!stream.eol()) {
    const ch = stream.next();
    if (escaped) {
      escaped = false;
      continue;
    }
    if (ch === "\\") {
      escaped = true;
      continue;
    }
    if (ch === '"') {
      return true;
    }
  }
  return false;
}

const wdlParser = StreamLanguage.define<WdlState>({
  startState: () => ({ inBlockComment: false }),
  token(stream, state) {
    // continue an open block comment across lines.
    if (state.inBlockComment) {
      if (stream.skipTo("*/")) {
        stream.match("*/");
        state.inBlockComment = false;
      } else {
        stream.skipToEnd();
      }
      return "comment";
    }

    if (stream.eatSpace()) return null;

    // comments.
    if (stream.match("//")) {
      stream.skipToEnd();
      return "comment";
    }
    if (stream.match("/*")) {
      if (!stream.skipTo("*/")) {
        stream.skipToEnd();
        state.inBlockComment = true;
      } else {
        stream.match("*/");
      }
      return "comment";
    }

    // strings (interpolation `${...}` is highlighted as part of the string for now).
    if (stream.peek() === '"') {
      stream.next();
      consumeString(stream);
      return "string";
    }

    // numbers and durations like `30s`, `5m`.
    if (stream.match(/^-?\d+(\.\d+)?(s|m|h|d)?\b/)) {
      return "number";
    }

    // identifiers and keywords.
    if (stream.match(/^[A-Za-z_][A-Za-z0-9_]*/)) {
      const word = stream.current();
      if (KEYWORDS.has(word)) return "keyword";
      return "variableName";
    }

    // annotations like `@id(...)`.
    if (stream.match(/^@[A-Za-z_][A-Za-z0-9_]*/)) {
      return "meta";
    }

    // multi-char operators first, then single chars.
    if (stream.match("->") || stream.match("++") || stream.match("??") ||
        stream.match("&&") || stream.match("||") || stream.match("!=") ||
        stream.match("==")) {
      return "operator";
    }
    if (stream.match(/^[=<>!:,.+?*/%-]/)) {
      return "operator";
    }
    if (stream.match(/^[()[\]{}]/)) {
      return "punctuation";
    }

    stream.next();
    return null;
  },
  languageData: {
    commentTokens: { line: "//", block: { open: "/*", close: "*/" } },
  },
});

// keyword + snippet completion. provider/action-aware completion is intentionally out of
// scope here; this is keyword/syntax only.
const keywordCompletions = [...KEYWORDS].map((label) => ({ label, type: "keyword" }));

const snippets = [
  snippetCompletion("if ${condition} -> ok\nelse -> fail", {
    label: "if",
    type: "keyword",
    detail: "if/else with outcomes",
  }),
  snippetCompletion("for ${item} in ${collection} {\n\t${}\n}", {
    label: "for",
    type: "keyword",
    detail: "for loop",
  }),
  snippetCompletion('workflow "${name}" {\n\tinput {\n\t\t${}\n\t}\n\n\t${}\n}', {
    label: "workflow",
    type: "keyword",
    detail: "workflow scaffold",
  }),
  snippetCompletion("${provider}.${action}(${args}) -> ok", {
    label: "action",
    type: "function",
    detail: "provider action call",
  }),
];

const wdlCompletion: CompletionSource = completeFromList([
  ...snippets,
  ...keywordCompletions,
]);

/// codemirror language support for wdl: highlighting + keyword/snippet completion.
export function wdl(): LanguageSupport {
  return new LanguageSupport(wdlParser, [
    wdlParser.data.of({ autocomplete: wdlCompletion }),
  ]);
}
