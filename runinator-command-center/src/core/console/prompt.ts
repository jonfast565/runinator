// what the prompt does with a keystroke: when Enter submits, and what Tab offers.
//
// both answers are pure functions of the buffer, which is what lets them be tested without a dom —
// and what keeps the two consoles agreeing, since `runinatorctl`'s reedline validator decides the
// same way.

import { completions } from "./registry";
import { tokenize } from "./tokenize";

/// true when Enter should submit rather than open a new line.
///
/// a `:` command is always one line. WDL is not: an open brace, bracket, paren, or quote means the
/// author is mid-construct, and submitting there would send a fragment that cannot compile.
export function isSubmittable(source: string): boolean {
  const trimmed = source.trim();

  if (!trimmed) {
    return false;
  }

  if (trimmed.startsWith(":")) {
    return true;
  }

  return isBalanced(source) && !source.trimEnd().endsWith("\\");
}

// delimiters closed and quotes finished, ignoring anything escaped or inside a quote.
export function isBalanced(source: string): boolean {
  const stack: string[] = [];
  let quote: string | null = null;
  let escaped = false;

  for (const character of source) {
    if (escaped) {
      escaped = false;
      continue;
    }

    if (character === "\\") {
      escaped = true;
      continue;
    }

    if (quote) {
      if (character === quote) {
        quote = null;
      }

      continue;
    }

    if (character === "'" || character === '"') {
      quote = character;
      continue;
    }

    if (character === "{" || character === "[" || character === "(") {
      stack.push(character);
      continue;
    }

    const opener = { "}": "{", "]": "[", ")": "(" }[character];

    if (opener && stack.at(-1) === opener) {
      stack.pop();
    }
  }

  return !quote && stack.length === 0;
}

export interface Completion {
  /// where in the buffer the replaced word starts.
  start: number;
  /// the candidates, already narrowed to what has been typed.
  options: string[];
}

/// what Tab offers at the end of a buffer.
///
/// only `:` lines complete. a bare line is WDL, and offering `settings` to someone typing an
/// expression would be worse than offering nothing.
export function complete(buffer: string): Completion {
  const trimmed = buffer.trimStart();

  if (!trimmed.startsWith(":")) {
    return { start: buffer.length, options: [] };
  }

  const bodyStart = buffer.length - trimmed.length + 1;
  const body = buffer.slice(bodyStart);
  const lastSpace = body.search(/\s\S*$/);
  const [prefix, start] =
    lastSpace >= 0 ? [body.slice(lastSpace + 1), bodyStart + lastSpace + 1] : [body, bodyStart];
  const typed = tokenizeQuietly(body.slice(0, start - bodyStart));

  return { start, options: completions(typed, prefix) };
}

// an unterminated quote mid-line is normal while typing, so a completion never fails on it.
function tokenizeQuietly(text: string): string[] {
  try {
    return tokenize(text);
  } catch {
    return [];
  }
}
