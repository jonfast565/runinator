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
  snippetCompletion,
  type Completion,
  type CompletionContext,
  type CompletionResult,
  type CompletionSource,
} from "@codemirror/autocomplete";

// keyword groups mirror runinator-wdl/src/wdl.pest, split by role so each gets its own color.
// structural declarations that open blocks or bind names.
const DECL_KW = new Set([
  "workflow",
  "params",
  "input",
  "node",
  "let",
  "type",
  "alias",
  "trigger",
  "start",
  "set",
  "secret",
  "config",
  "fn",
  "namespace",
  "import",
]);
// control-flow statements and block headers.
const CONTROL_KW = new Set([
  "if",
  "else",
  "for",
  "while",
  "until",
  "match",
  "when",
  "toggle",
  "split",
  "on",
  "off",
  "parallel",
  "race",
  "try",
  "catch",
  "finally",
  "map",
  "branch",
  "join",
  "wait",
  "emit",
  "output",
  "yield",
  "approve",
  "fail",
  "subflow",
  "compute",
  "return",
  "goto",
  "edges",
  "gate",
  "signal",
  "watch",
  "compensate",
  "assert",
  "transform",
  "audit",
  "checkpoint",
  "mutex",
  "throttle",
  "await",
  "debounce",
  "collect",
  "barrier",
  "circuit_breaker",
  "event_source",
]);
// clause/option words that modify a statement.
const MODIFIER_KW = new Set([
  "with",
  "as",
  "initial",
  "limit",
  "concurrency",
  "detached",
  "reuse",
  "disabled",
  "blackout",
  "to",
  "cron",
  "winner",
  "name",
  "meta",
  "returns",
  "every",
  "timeout",
  "hold",
  "release",
  "key",
  "priority",
  "max_depth",
  "rate",
  "per",
  "delay",
  "count",
  "threshold",
  "window",
  "cooldown",
  "mode",
  "action",
  "actor",
  "target",
  "reason",
  "filter",
]);
// word-form comparison/membership operators.
const OP_KW = new Set(["exists", "contains", "in", "starts_with", "ends_with"]);
// outcome labels, only highlighted as such when they precede a `->` transition.
const OUTCOMES = new Set(["ok", "next", "fail", "timeout", "reject"]);
// constant-like policy/target atoms.
const ATOMS = new Set([
  "all",
  "any",
  "first_success",
  "done",
  "none",
  "manual",
  "condition",
  "external",
]);
// coercion and compile-time intrinsics, highlighted as functions only when called.
const BUILTINS = new Set(["string", "json", "file", "dir", "inline"]);
// reference roots that are never keywords.
const PURE_REFS = new Set(["run", "loop", "state", "item"]);
// roots that double as keywords; treated as a reference only before a `.`. `std` is the builtin
// standard-library namespace root (`std.strings.upper(...)`).
const ROOT_KEYWORDS = new Set(["params", "config", "secret", "workflow", "std"]);
// primitive type names. surfaced for completion, and colored inside type-position contexts
// (after `type`/`:` in a type field, `node x:`/`let x:` annotations) where they are unambiguous.
const TYPES = [
  "any",
  "boolean",
  "bool",
  "duration",
  "float",
  "int",
  "integer",
  "json",
  "map",
  "null",
  "number",
  "string",
];

const STD_MODULES = [
  "math",
  "strings",
  "collections",
  "objects",
  "encoding",
  "logic",
  "dates",
  "regex",
  "exec",
] as const;

const STD_INTRINSICS: { label: string; module: (typeof STD_MODULES)[number] }[] = [
  ...[
    "add",
    "sub",
    "mul",
    "div",
    "mod",
    "floor",
    "ceil",
    "round",
    "min",
    "max",
    "parse_int",
    "parse_float",
  ].map((label) => ({ label, module: "math" as const })),
  ...[
    "lower",
    "upper",
    "trim",
    "split",
    "join",
    "replace",
    "substring",
    "starts_with",
    "ends_with",
  ].map((label) => ({ label, module: "strings" as const })),
  ...[
    "len",
    "keys",
    "values",
    "contains",
    "at",
    "has",
    "sum",
    "sort",
    "reverse",
    "unique",
    "flatten",
    "slice",
    "first",
    "last",
    "append",
    "range",
    "map",
    "filter",
    "find",
    "any",
    "all",
    "reduce",
    "sort_by",
    "flat_map",
  ].map((label) => ({ label, module: "collections" as const })),
  ...["merge", "pick", "omit", "entries", "from_entries"].map((label) => ({
    label,
    module: "objects" as const,
  })),
  ...["parse_json", "base64_encode", "base64_decode"].map((label) => ({
    label,
    module: "encoding" as const,
  })),
  ...["eq", "ne", "gt", "lt", "gte", "lte", "not", "and", "or", "default"].map((label) => ({
    label,
    module: "logic" as const,
  })),
  ...["format_date", "parse_date", "add_duration", "date_diff"].map((label) => ({
    label,
    module: "dates" as const,
  })),
  ...["regex_match", "regex_replace", "regex_extract"].map((label) => ({
    label,
    module: "regex" as const,
  })),
  ...["http_get", "http_post", "now", "uuid", "env"].map((label) => ({
    label,
    module: "exec" as const,
  })),
];

// completion vocabulary spans every group so suggestions stay broad.
const KEYWORDS = new Set([
  ...DECL_KW,
  ...CONTROL_KW,
  ...MODIFIER_KW,
  ...OP_KW,
  ...OUTCOMES,
  ...ATOMS,
  ...BUILTINS,
  ...PURE_REFS,
  ...ROOT_KEYWORDS,
  ...TYPES,
  "true",
  "false",
  "null",
]);

interface WdlState {
  inBlockComment: boolean;
  // previous significant token was a `.` (member access).
  afterDot: boolean;
  // previous token was a provider name awaiting its `.action`.
  afterProvider: boolean;
  // next identifier is the action name of a `provider.action(` call.
  expectAction: boolean;
  // next identifier is the name being declared by a `type` keyword.
  expectTypeName: boolean;
  // next identifier is the name being bound by a `let` keyword.
  expectBindingName: boolean;
  // a `:` after the just-seen binding name opens a type annotation.
  pendingBindingType: boolean;
  // the just-seen token was a declared type name or `params`; a following `{` opens a type body.
  afterTypeName: boolean;
  // currently scanning a type expression; identifiers here are type references.
  inType: boolean;
  // kind of each open `{`: "type" bodies make `:` introduce a type, "value" bodies are object literals.
  braceStack: ("type" | "value")[];
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
  if (PURE_REFS.has(word)) {
    return "refRoot";
  }

  if (ROOT_KEYWORDS.has(word) && stream.match(/^\s*\./, false)) {
    return "refRoot";
  }

  // outcome label immediately before a transition arrow.
  if (OUTCOMES.has(word) && stream.match(/^\s*->/, false)) {
    return "outcome";
  }

  // coercion builtin in call position.
  if (BUILTINS.has(word) && stream.match(/^\s*\(/, false)) {
    return "builtin";
  }

  if (ATOMS.has(word)) {
    return "atom";
  }

  if (word === "true" || word === "false") {
    return "bool";
  }

  if (word === "null") {
    return "null";
  }

  if (DECL_KW.has(word)) {
    return "declKw";
  }

  if (CONTROL_KW.has(word)) {
    return "controlKw";
  }

  if (MODIFIER_KW.has(word)) {
    return "modifierKw";
  }

  if (OP_KW.has(word)) {
    return "opKw";
  }

  return "variableName";
}

const wdlParser = StreamLanguage.define<WdlState>({
  startState: () => ({
    inBlockComment: false,
    afterDot: false,
    afterProvider: false,
    expectAction: false,
    expectTypeName: false,
    expectBindingName: false,
    pendingBindingType: false,
    afterTypeName: false,
    inType: false,
    braceStack: [],
  }),
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

    // type expressions are single-line; reset type context at the start of each line so a field
    // name beginning a new line is not mistaken for the previous field's type.
    if (stream.sol()) {
      state.inType = false;
      state.afterTypeName = false;
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

      if (stream.match(/^[A-Za-z_][A-Za-z0-9_-]*/)) {
        return "action";
      }
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

      // the name being declared by `type` (a `{` or `=` body follows).
      if (state.expectTypeName) {
        state.expectTypeName = false;
        state.afterTypeName = true;
        return "typeName";
      }

      // the name being bound by `node` (workflow scope) or `let` (compute-local); a following `:`
      // would open a type annotation.
      if (state.expectBindingName) {
        state.expectBindingName = false;
        state.pendingBindingType = true;
        return "variableName";
      }

      // any identifier inside a type expression is a type reference (named or builtin primitive).
      if (state.inType) {
        return "typeName";
      }

      const cls = classifyWord(word, stream);

      if (cls === "declKw" && word === "type") {
        state.expectTypeName = true;
      }

      if (cls === "declKw" && (word === "node" || word === "let")) {
        state.expectBindingName = true;
      }

      // the `params` block keyword opens a type body of input fields.
      if (word === "params" && stream.match(/^\s*\{/, false)) {
        state.afterTypeName = true;
        return "declKw";
      }

      return cls;
    }

    // transition arrow.
    if (stream.match("->")) {
      return "arrow";
    }

    // argument/object spread.
    if (stream.match("...")) {
      return "operator";
    }

    // multi-char operators.
    if (
      stream.match("++") ||
      stream.match("??") ||
      stream.match("&&") ||
      stream.match("||") ||
      stream.match("!=") ||
      stream.match("==") ||
      stream.match(">=") ||
      stream.match("<=")
    ) {
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

    // braces maintain a context stack so a `:` can tell a type field from an object-literal entry.
    if (stream.match("{")) {
      const kind = state.afterTypeName || state.inType ? "type" : "value";
      state.afterTypeName = false;
      state.inType = false;
      state.braceStack.push(kind);
      return "bracket";
    }

    if (stream.match("}")) {
      state.braceStack.pop();
      state.inType = false;
      return "bracket";
    }

    // `=` assignment (`==` is handled above): opens a `type X =` alias body, otherwise ends type
    // context. the lambda arrow `=>` keeps its operator role without touching type context.
    if (stream.match("=")) {
      if (stream.peek() === ">") {
        return "operator";
      }

      if (state.afterTypeName) {
        state.afterTypeName = false;
        state.inType = true;
      } else {
        state.inType = false;
        state.pendingBindingType = false;
      }

      return "operator";
    }

    // `:` opens a type when inside a type body or a `let`/field annotation.
    if (stream.match(":")) {
      const top = state.braceStack[state.braceStack.length - 1];

      if (top === "type" || state.pendingBindingType) {
        state.inType = true;
        state.pendingBindingType = false;
      }

      return "operator";
    }

    // `,` separates type fields; the next field name leaves type context.
    if (stream.match(",")) {
      state.inType = false;
      return "operator";
    }

    // remaining single-char operators.
    if (stream.match(/^[<>!+?*/%|&-]/)) {
      return "operator";
    }

    // brackets and punctuation.
    if (stream.match(/^[()[\]]/)) {
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
    typeName: t.typeName,
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
  // type names (declared `type X`, primitive builtins, and type-position references) in cyan.
  { tag: t.typeName, color: "#0997b3" },
  {
    tag: [
      t.function(t.variableName),
      t.function(t.propertyName),
      t.standard(t.function(t.variableName)),
    ],
    color: "#4078f2",
  },
  // reference roots (`params.*`, `run.*`) and their member path.
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
  snippetCompletion('workflow "${name}" {\n\tparams {\n\t\t${}\n\t}\n\n\t${}\n}', {
    label: "workflow",
    type: "keyword",
    detail: "workflow scaffold",
  }),
  snippetCompletion("node ${name} <- ${provider}.${action}(${args}) -> ok", {
    label: "action",
    type: "function",
    detail: "provider action node",
  }),
  snippetCompletion("fn ${name}(${arg}: ${type}) -> ${return_type} = ${value}", {
    label: "fn",
    type: "function",
    detail: "function definition",
  }),
  snippetCompletion("import std.${module} as ${alias}", {
    label: "import std",
    type: "keyword",
    detail: "standard-library import",
  }),
  snippetCompletion('trigger cron "${cron}" with { ${} }', {
    label: "trigger cron",
    type: "keyword",
    detail: "cron trigger",
  }),
  snippetCompletion("watch ${condition} -> ${target}", {
    label: "watch",
    type: "keyword",
    detail: "workflow guard",
  }),
  snippetCompletion("gate condition when ${condition} every ${interval} timeout ${deadline}", {
    label: "gate condition",
    type: "keyword",
    detail: "condition gate",
  }),
  snippetCompletion('signal "${name}" key ${correlation}', {
    label: "signal",
    type: "keyword",
    detail: "external signal wait",
  }),
  snippetCompletion("compensate ${provider}.${action}(${args})", {
    label: "compensate",
    type: "keyword",
    detail: "compensating action",
  }),
  snippetCompletion('assert {\n\t"${name}": ${condition}\n}', {
    label: "assert",
    type: "keyword",
    detail: "invariant assertions",
  }),
  snippetCompletion("transform {\n\t${name} = ${expr}\n}", {
    label: "transform",
    type: "keyword",
    detail: "data reshape bindings",
  }),
  snippetCompletion('audit action "${action}" actor ${actor}', {
    label: "audit",
    type: "keyword",
    detail: "compliance audit record",
  }),
  snippetCompletion('checkpoint "${name}"', {
    label: "checkpoint",
    type: "keyword",
    detail: "named state snapshot",
  }),
  snippetCompletion('mutex "${name}" {\n\t${body}\n}', {
    label: "mutex",
    type: "keyword",
    detail: "cross-run exclusive lock (critical section)",
  }),
  snippetCompletion('throttle "${name}" rate ${n} per ${window}', {
    label: "throttle",
    type: "keyword",
    detail: "cross-run rate limiter",
  }),
  snippetCompletion('await ${run_ids} mode "all"', {
    label: "await",
    type: "keyword",
    detail: "wait for other run(s)",
  }),
  snippetCompletion('debounce "${name}" delay ${delay}', {
    label: "debounce",
    type: "keyword",
    detail: "trailing-delay debounce",
  }),
  snippetCompletion('collect "${name}" max ${count} timeout ${deadline}', {
    label: "collect",
    type: "keyword",
    detail: "timed accumulator",
  }),
  snippetCompletion('barrier "${name}" count ${n} timeout ${deadline}', {
    label: "barrier",
    type: "keyword",
    detail: "multi-run rendezvous",
  }),
  snippetCompletion(
    'circuit_breaker "${name}" threshold ${n} window ${window} cooldown ${cooldown}',
    {
      label: "circuit_breaker",
      type: "keyword",
      detail: "cross-run failure guard",
    },
  ),
  snippetCompletion('event_source type "${event_type}" max ${count} timeout ${deadline}', {
    label: "event_source",
    type: "keyword",
    detail: "stream-driven iteration",
  }),
  snippetCompletion("type ${Name} {\n\t${field}: ${type}\n}", {
    label: "type struct",
    type: "type",
    detail: "named struct type",
  }),
  snippetCompletion("enum[${value}]", {
    label: "enum",
    type: "type",
    detail: "enum type",
  }),
  snippetCompletion("${integer} range ${0}..${10}", {
    label: "range",
    type: "type",
    detail: "bounded type",
  }),
  snippetCompletion("${item} => ${expr}", {
    label: "lambda",
    type: "function",
    detail: "lambda expression",
  }),
];

const moduleCompletions: Completion[] = STD_MODULES.map((label) => ({
  label,
  type: "module",
  detail: "std module",
}));

const intrinsicCompletions: Completion[] = STD_INTRINSICS.map(({ label, module }) => ({
  label,
  type: "function",
  detail: `std.${module}.${label}`,
}));

function intrinsicCompletionsFor(module: string): Completion[] {
  return intrinsicCompletions.filter(
    (completion) => completion.detail === `std.${module}.${completion.label}`,
  );
}

export const wdlStaticCompletionLabels = [
  ...new Set([
    ...snippets.map((completion) => completion.label),
    ...keywordCompletions.map((completion) => completion.label),
    ...moduleCompletions.map((completion) => completion.label),
    ...intrinsicCompletions.map((completion) => completion.label),
  ]),
].sort();

export const wdlCompletion: CompletionSource = (
  context: CompletionContext,
): CompletionResult | null => {
  const word = context.matchBefore(/[A-Za-z_][A-Za-z0-9_-]*/);
  const tokenStart = word?.from ?? context.pos;
  const beforeToken = context.state.sliceDoc(0, tokenStart);
  const stdModule = /\bstd\.([A-Za-z_][A-Za-z0-9_]*)\.$/.exec(beforeToken);
  const afterDot = beforeToken.endsWith(".");

  if (!context.explicit && !word && !afterDot) {
    return null;
  }

  if (stdModule) {
    return {
      from: tokenStart,
      options: intrinsicCompletionsFor(stdModule[1]),
      validFor: /^[A-Za-z_][A-Za-z0-9_]*$/,
    };
  }

  if (/\bstd\.$/.test(beforeToken)) {
    return {
      from: tokenStart,
      options: moduleCompletions,
      validFor: /^[A-Za-z_][A-Za-z0-9_]*$/,
    };
  }

  if (afterDot) {
    return {
      from: tokenStart,
      options: intrinsicCompletions,
      validFor: /^[A-Za-z_][A-Za-z0-9_]*$/,
    };
  }

  return {
    from: word?.from ?? context.pos,
    options: [...snippets, ...keywordCompletions],
    validFor: /^[A-Za-z_][A-Za-z0-9_-]*$/,
  };
};

// codemirror language support for wdl: highlighting + keyword/snippet completion.
export function wdl(providerCompletion?: CompletionSource): LanguageSupport {
  const autocomplete = wdlParser.data.of({ autocomplete: providerCompletion ?? wdlCompletion });
  return new LanguageSupport(wdlParser, [syntaxHighlighting(wdlHighlightStyle), autocomplete]);
}
