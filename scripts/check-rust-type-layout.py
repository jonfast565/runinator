#!/usr/bin/env python3
"""Check that each Rust source file has at most one file-scope struct or trait."""

from __future__ import annotations

import re
import sys
from pathlib import Path


DECLARATION = re.compile(r"\b(struct|trait)\s+([A-Za-z_][A-Za-z0-9_]*)")
SKIP_PARTS = {".git", "build", "target"}


def mask_source(source: str) -> str:
    """Hide comments and literals while preserving newlines and positions."""
    chars = list(source)
    index = 0
    length = len(source)
    state = "code"
    raw_hashes = 0

    while index < length:
        if state == "code":
            if source.startswith("//", index):
                chars[index] = chars[index + 1] = " "
                index += 2
                state = "line_comment"
                continue
            if source.startswith("/*", index):
                chars[index] = chars[index + 1] = " "
                index += 2
                state = "block_comment"
                continue

            raw_start = re.match(r"(?:b)?r(#+)?\"", source[index:])
            if raw_start:
                raw_hashes = len(raw_start.group(1) or "")
                end = index + len(raw_start.group(0))
                for position in range(index, end):
                    if source[position] != "\n":
                        chars[position] = " "
                index = end
                state = "raw_string"
                continue

            if source[index] == '"':
                chars[index] = " "
                index += 1
                state = "string"
                continue
            if source[index] == "'" and index + 2 < length and source[index + 2] == "'":
                for position in range(index, index + 3):
                    chars[position] = " "
                index += 3
                continue
            index += 1
            continue

        if state == "line_comment":
            if source[index] == "\n":
                state = "code"
            else:
                chars[index] = " "
            index += 1
            continue

        if state == "block_comment":
            if source.startswith("*/", index):
                chars[index] = chars[index + 1] = " "
                index += 2
                state = "code"
            else:
                if source[index] != "\n":
                    chars[index] = " "
                index += 1
            continue

        if state == "string":
            if source[index] == "\\":
                chars[index] = " "
                index += 1
                if index < length:
                    chars[index] = " "
                    index += 1
            elif source[index] == '"':
                chars[index] = " "
                index += 1
                state = "code"
            else:
                if source[index] != "\n":
                    chars[index] = " "
                index += 1
            continue

        if source.startswith('"' + ("#" * raw_hashes), index):
            end = index + raw_hashes + 1
            for position in range(index, end):
                chars[position] = " "
            index = end
            state = "code"
        else:
            if source[index] != "\n":
                chars[index] = " "
            index += 1

    return "".join(chars)


def declarations(path: Path) -> list[tuple[str, str, int]]:
    source = path.read_text(encoding="utf-8")
    masked = mask_source(source)
    result: list[tuple[str, str, int]] = []
    for match in DECLARATION.finditer(masked):
        depth = 0
        for character in masked[: match.start()]:
            if character == "{":
                depth += 1
            elif character == "}":
                depth -= 1
        if depth == 0:
            line = masked.count("\n", 0, match.start()) + 1
            result.append((match.group(1), match.group(2), line))
    return result


def rust_sources(root: Path) -> list[Path]:
    return sorted(
        path
        for path in root.rglob("*.rs")
        if not any(part in SKIP_PARTS for part in path.parts)
    )


def main() -> int:
    root = Path(__file__).resolve().parent.parent
    violations = []
    for path in rust_sources(root):
        items = declarations(path)
        if len(items) > 1:
            violations.append((path.relative_to(root), items))

    if not violations:
        print("rust type layout: ok")
        return 0

    for path, items in violations:
        formatted = ", ".join(f"{kind} {name}:{line}" for kind, name, line in items)
        print(f"{path}: {formatted}", file=sys.stderr)
    print(
        f"rust type layout: {len(violations)} file(s) contain more than one file-scope struct or trait",
        file=sys.stderr,
    )
    return 1


if __name__ == "__main__":
    raise SystemExit(main())
