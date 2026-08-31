import type { StreamParser, StringStream } from "@codemirror/language";

const KEYWORDS = new Set(
  [
    "abort",
    "abs",
    "abstract",
    "accept",
    "access",
    "aliased",
    "all",
    "and",
    "array",
    "at",
    "begin",
    "body",
    "case",
    "constant",
    "declare",
    "delay",
    "delta",
    "digits",
    "do",
    "else",
    "elsif",
    "end",
    "entry",
    "exception",
    "exit",
    "for",
    "function",
    "generic",
    "goto",
    "if",
    "in",
    "interface",
    "is",
    "limited",
    "loop",
    "mod",
    "new",
    "not",
    "null",
    "of",
    "or",
    "others",
    "out",
    "overriding",
    "package",
    "parallel",
    "pragma",
    "private",
    "procedure",
    "protected",
    "raise",
    "range",
    "record",
    "rem",
    "renames",
    "requeue",
    "return",
    "reverse",
    "select",
    "separate",
    "some",
    "subtype",
    "synchronized",
    "tagged",
    "task",
    "terminate",
    "then",
    "type",
    "until",
    "use",
    "when",
    "while",
    "with",
    "xor",
  ].map((word) => word.toLowerCase()),
);

const TYPES = new Set([
  "boolean",
  "character",
  "duration",
  "float",
  "integer",
  "long_float",
  "long_integer",
  "natural",
  "positive",
  "string",
  "wide_character",
  "wide_string",
]);

function consumeString(stream: StringStream): void {
  while (!stream.eol()) {
    if (stream.next() !== '"') {
      continue;
    }

    if (stream.peek() === '"') {
      stream.next();
      continue;
    }

    return;
  }
}

export const ada: StreamParser<Record<string, never>> = {
  startState: () => ({}),
  token(stream) {
    if (stream.eatSpace()) {
      return null;
    }

    if (stream.match("--")) {
      stream.skipToEnd();
      return "comment";
    }

    if (stream.peek() === '"') {
      stream.next();
      consumeString(stream);
      return "string";
    }

    if (stream.match(/^'(?:[^']|'')'/)) {
      return "character";
    }

    if (stream.match(/^(?:\d(?:_?\d)*)#(?:[\da-f](?:_?[\da-f])*)#(?:e[+-]?\d+)?/i)) {
      return "number";
    }

    if (stream.match(/^\d(?:_?\d)*(?:\.\d(?:_?\d)*)?(?:e[+-]?\d+)?/i)) {
      return "number";
    }

    if (stream.match(/^[a-z][a-z\d_]*/i)) {
      const word = stream.current().toLowerCase();

      if (KEYWORDS.has(word)) {
        return "keyword";
      }

      if (TYPES.has(word)) {
        return "typeName";
      }

      return "variableName";
    }

    if (stream.match(/^(?:=>|:=|\*\*|\/=|<=|>=|<<|>>|[+\-*/=<>.&])/)) {
      return "operator";
    }

    stream.next();
    return null;
  },
};
