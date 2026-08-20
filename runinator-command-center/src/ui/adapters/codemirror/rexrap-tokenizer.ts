// stream tokenizer and highlight style for the rexrap editor.

import { HighlightStyle, StreamLanguage, type StringStream } from "@codemirror/language";
import { tags as t } from "@lezer/highlight";
import {
  ATOMS,
  BUILTINS,
  CONTROL_KW,
  DECL_KW,
  MODIFIER_KW,
  OP_KW,
  OUTCOMES,
  PURE_REFS,
  ROOT_KEYWORDS,
} from "./rexrap-vocabulary";

interface RexRapState {
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

export const rexrapParser = StreamLanguage.define<RexRapState>({
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

      // the name being bound by `node` (workflow scope) or `let` (do-local); a following `:`
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

// rexrap color scheme (one-light inspired) layered over codemirror's default highlight style.
// basicSetup registers the default style as a fallback, so these non-fallback rules win.
export const rexrapHighlightStyle = HighlightStyle.define([
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
