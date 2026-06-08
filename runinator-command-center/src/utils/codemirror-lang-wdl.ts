// a lightweight codemirror language for the runinator wdl workflow dsl. this is a
// stream tokenizer for syntax highlighting plus keyword/snippet completion only; the
// authoritative parse/lint comes from the rust runinator-wdl compiler via analyze_wdl.
// the token classes mirror runinator-wdl/src/wdl.pest and the vscode tmlanguage grammar
// so both surfaces highlight the same vocabulary.

import {
  StreamLanguage,
  LanguageSupport,
  HighlightStyle,
  syntaxHighlighting,
  type StringStream,
} from "@codemirror/language";
import { tags as t } from "@lezer/highlight";
import {
  completeFromList,
  snippetCompletion,
  type CompletionSource,
} from "@codemirror/autocomplete";

// keyword groups mirror runinator-wdl/src/wdl.pest, split by role so each gets its own color.
// structural declarations that open blocks or bind names.
const DECL_KW = new Set([
  "workflow", "input", "let", "type", "alias", "trigger", "start", "set", "secret", "config",
]);
// control-flow statements and block headers.
const CONTROL_KW = new Set([
  "if", "else", "for", "while", "until", "match", "when", "parallel", "race", "try", "catch",
  "finally", "map", "branch", "join", "wait", "emit", "approve", "fail", "subflow", "spawn",
  "call", "compute", "return", "goto",
]);
// clause/option words that modify a statement.
const MODIFIER_KW = new Set([
  "with", "as", "initial", "limit", "concurrency", "detached", "reuse", "disabled", "blackout",
  "to", "cron", "winner", "name", "meta",
]);
// word-form comparison/membership operators.
const OP_KW = new Set(["exists", "contains", "in", "starts_with", "ends_with"]);
// outcome labels, only highlighted as such when they precede a `->` transition.
const OUTCOMES = new Set(["ok", "next", "fail", "timeout", "reject"]);
// constant-like policy/target atoms.
const ATOMS = new Set(["all", "any", "first_success", "done", "none"]);
// coercion builtins, highlighted as functions only when called.
const BUILTINS = new Set(["string", "json"]);
// reference roots that are never keywords.
const PURE_REFS = new Set(["run", "loop", "state", "item"]);
// roots that double as keywords; treated as a reference only before a `.`.
const ROOT_KEYWORDS = new Set(["input", "config", "secret", "workflow"]);
// primitive type names, surfaced for completion only (too ambiguous to color reliably).
const TYPES = ["any", "boolean", "integer", "map", "number", "string"];

// completion vocabulary spans every group so suggestions stay broad.
const KEYWORDS = new Set([
  ...DECL_KW, ...CONTROL_KW, ...MODIFIER_KW, ...OP_KW, ...OUTCOMES, ...ATOMS,
  ...BUILTINS, ...PURE_REFS, ...ROOT_KEYWORDS, ...TYPES, "true", "false", "null",
]);

interface WdlState {
  inBlockComment: boolean;
  // previous significant token was a `.` (member access).
  afterDot: boolean;
  // previous token was a provider name awaiting its `.action`.
  afterProvider: boolean;
  // next identifier is the action name of a `provider.action(` call.
  expectAction: boolean;
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

// resolve a bare word to its token class; member-access words are handled by the caller.
function classifyWord(word: string, stream: StringStream): string {
  // reference roots: pure refs always, keyword-roots only before a dot.
  if (PURE_REFS.has(word)) return "refRoot";
  if (ROOT_KEYWORDS.has(word) && stream.match(/^\s*\./, false)) return "refRoot";
  // outcome label immediately before a transition arrow.
  if (OUTCOMES.has(word) && stream.match(/^\s*->/, false)) return "outcome";
  // coercion builtin in call position.
  if (BUILTINS.has(word) && stream.match(/^\s*\(/, false)) return "builtin";
  if (ATOMS.has(word)) return "atom";
  if (word === "true" || word === "false") return "bool";
  if (word === "null") return "null";
  if (DECL_KW.has(word)) return "declKw";
  if (CONTROL_KW.has(word)) return "controlKw";
  if (MODIFIER_KW.has(word)) return "modifierKw";
  if (OP_KW.has(word)) return "opKw";
  return "variableName";
}

const wdlParser = StreamLanguage.define<WdlState>({
  startState: () => ({ inBlockComment: false, afterDot: false, afterProvider: false, expectAction: false }),
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

    // dot context applies to the next token only; preserve it across whitespace.
    const afterDot = state.afterDot;
    state.afterDot = false;

    if (stream.eatSpace()) {
      state.afterDot = afterDot;
      return null;
    }

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

    // action name immediately following a `provider .`.
    if (state.expectAction) {
      state.expectAction = false;
      if (stream.match(/^[A-Za-z_][A-Za-z0-9_-]*/)) return "action";
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

    // annotations like `@id(...)`, `@skip`.
    if (stream.match(/^@[A-Za-z_][A-Za-z0-9_]*/)) {
      return "annotation";
    }

    // provider name in a `provider.action(...)` call (hyphens allowed in provider/action idents).
    if (stream.match(/^[A-Za-z_][A-Za-z0-9_-]*(?=\s*\.\s*[A-Za-z_][A-Za-z0-9_-]*\s*\()/)) {
      state.afterProvider = true;
      return "provider";
    }

    // identifiers, keywords, references.
    if (stream.match(/^[A-Za-z_][A-Za-z0-9_]*/)) {
      const word = stream.current();
      // member access: a method call when followed by `(`, otherwise a property.
      if (afterDot) {
        return stream.match(/^\s*\(/, false) ? "method" : "property";
      }
      return classifyWord(word, stream);
    }

    // transition arrow.
    if (stream.match("->")) return "arrow";
    // argument/object spread.
    if (stream.match("...")) return "operator";
    // multi-char operators.
    if (stream.match("++") || stream.match("??") || stream.match("&&") || stream.match("||") ||
        stream.match("!=") || stream.match("==") || stream.match(">=") || stream.match("<=")) {
      return "operator";
    }
    // member dot: routes the next token to action (after a provider) or property access.
    if (stream.match(".")) {
      if (state.afterProvider) {
        state.afterProvider = false;
        state.expectAction = true;
      } else {
        state.afterDot = true;
      }
      return "operator";
    }
    // remaining single-char operators.
    if (stream.match(/^[=<>!:,+?*/%|&-]/)) {
      return "operator";
    }
    // brackets and punctuation.
    if (stream.match(/^[()[\]{}]/)) {
      return "bracket";
    }

    stream.next();
    return null;
  },
  // custom token names to highlight tags. reused tag instances are shared with the style below.
  tokenTable: {
    declKw: t.definitionKeyword,
    controlKw: t.controlKeyword,
    modifierKw: t.modifier,
    opKw: t.operatorKeyword,
    outcome: t.special(t.controlKeyword),
    atom: t.atom,
    bool: t.bool,
    null: t.null,
    provider: t.namespace,
    action: t.function(t.variableName),
    method: t.function(t.propertyName),
    builtin: t.standard(t.function(t.variableName)),
    refRoot: t.special(t.variableName),
    property: t.propertyName,
    annotation: t.meta,
    number: t.number,
    string: t.string,
    operator: t.operator,
    arrow: t.controlOperator,
    bracket: t.bracket,
    comment: t.comment,
    variableName: t.variableName,
  },
  languageData: {
    commentTokens: { line: "//", block: { open: "/*", close: "*/" } },
  },
});

// wdl color scheme (one-light inspired) layered over codemirror's default highlight style.
// basicSetup registers the default style as a fallback, so these non-fallback rules win.
const wdlHighlightStyle = HighlightStyle.define([
  { tag: t.comment, color: "#a0a1a7", fontStyle: "italic" },
  // declaration, control, and modifier keywords share the keyword purple.
  { tag: [t.definitionKeyword, t.controlKeyword, t.modifier], color: "#a626a4" },
  // outcome labels (`ok ->`, `fail ->`) read as amber control flow.
  { tag: t.special(t.controlKeyword), color: "#c18401", fontWeight: "bold" },
  { tag: t.operatorKeyword, color: "#0184bc" },
  // provider namespace vs the action/method/builtin function names.
  { tag: t.namespace, color: "#c18401" },
  { tag: [t.function(t.variableName), t.function(t.propertyName), t.standard(t.function(t.variableName))], color: "#4078f2" },
  // reference roots (`input.*`, `run.*`) and their member path.
  { tag: t.special(t.variableName), color: "#e45649" },
  { tag: t.propertyName, color: "#383a42" },
  // annotations (`@id`, `@skip`) in dark blue.
  { tag: t.meta, color: "#00008b" },
  { tag: t.atom, color: "#986801" },
  { tag: [t.bool, t.null], color: "#0184bc" },
  { tag: t.number, color: "#986801" },
  { tag: t.string, color: "#50a14f" },
  // transition arrows pop in the keyword purple.
  { tag: t.controlOperator, color: "#a626a4" },
  { tag: [t.operator, t.bracket], color: "#383a42" },
  { tag: t.variableName, color: "#383a42" },
]);

// keyword + snippet completion. provider/action-aware completion is supplied by the
// command-center editor as an async source backed by runinator-wdl.
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
export function wdl(providerCompletion?: CompletionSource): LanguageSupport {
  return new LanguageSupport(wdlParser, [
    syntaxHighlighting(wdlHighlightStyle),
    wdlParser.data.of({ autocomplete: providerCompletion ? [providerCompletion, wdlCompletion] : wdlCompletion }),
  ]);
}
