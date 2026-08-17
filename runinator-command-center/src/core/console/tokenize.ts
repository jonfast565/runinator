// splitting a console command line the way a shell would.
//
// this mirrors `runinator-ctl`'s `repl::tokenize` on purpose: the two consoles accept the same
// commands, so `:settings set aws key '{"a": 1}'` has to survive as the same five arguments in
// both. json arguments are the whole reason quoting exists here.

export class ConsoleParseError extends Error {}

export function tokenize(line: string): string[] {
  const tokens: string[] = [];
  let current = "";
  let hasToken = false;
  let quote: string | null = null;
  let escaped = false;

  for (const character of line) {
    if (escaped) {
      current += character;
      escaped = false;
      continue;
    }

    // a backslash escapes inside double quotes and outside quotes, but not inside single quotes,
    // which is what lets a windows path or a json string be pasted verbatim in '...'.
    if (character === "\\" && quote !== "'") {
      escaped = true;
      hasToken = true;
      continue;
    }

    if (quote) {
      if (character === quote) {
        quote = null;
      } else {
        current += character;
      }

      continue;
    }

    if (character === "'" || character === '"') {
      quote = character;
      hasToken = true;
      continue;
    }

    if (/\s/.test(character)) {
      if (hasToken) {
        tokens.push(current);
        current = "";
        hasToken = false;
      }

      continue;
    }

    current += character;
    hasToken = true;
  }

  if (quote) {
    throw new ConsoleParseError("unterminated quote");
  }

  if (escaped) {
    throw new ConsoleParseError("line ends with a dangling backslash");
  }

  if (hasToken) {
    tokens.push(current);
  }

  return tokens;
}
