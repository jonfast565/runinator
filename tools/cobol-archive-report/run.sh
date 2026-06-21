#!/bin/sh
# run.sh: decompress one or more .jsonl.gz archive dumps and feed them to the
# archive-report COBOL program. The program itself never touches gzip.
#
# usage:
#   ./run.sh archive/2026-06-21/*.jsonl.gz
#   ./run.sh /var/lib/runinator/archive/**/*.jsonl.gz
#
# plain (already decompressed) .jsonl files are passed through unchanged, so
# you can also mix .jsonl and .jsonl.gz arguments.
set -eu

here="$(cd "$(dirname "$0")" && pwd)"
bin="$here/archive-report"

if [ ! -x "$bin" ]; then
    echo "archive-report binary not found; run 'make' first" >&2
    exit 1
fi

if [ "$#" -eq 0 ]; then
    echo "usage: $0 <dump.jsonl[.gz]> [more...]" >&2
    exit 2
fi

# decompress into a temp file rather than piping: GnuCOBOL warns when it closes
# a pipe fd, and a real path keeps the report program's file handling clean.
# gzip -cdf transparently passes through non-gzip input, so plain .jsonl and
# .jsonl.gz arguments can be mixed freely.
tmp="$(mktemp "${TMPDIR:-/tmp}/archive-report.XXXXXX.jsonl")"
trap 'rm -f "$tmp"' EXIT INT TERM
gzip -cdf "$@" > "$tmp"
"$bin" "$tmp"
