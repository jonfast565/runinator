# archive-report (GnuCOBOL)

A small, self-contained **GnuCOBOL** tool that reads `runinator-archiver`
JSONL/NDJSON dumps and prints a metrics report using a COBOL **REPORT SECTION**.
It consumes the JSONL dumps *only* — it never touches the database.

## What it reports

The archiver writes gzip'd NDJSON dumps (one JSON envelope per line) under
`/var/lib/runinator/archive/<YYYY-MM-DD>/<table>-<uuid>.jsonl.gz`. This tool
parses those envelopes and emits:

- **workflow_runs status breakdown** — count and percentage per `status`
  (`succeeded`, `failed`, `timed_out`, `canceled`, `running`, …).
- **trigger source breakdown** — count per `trigger_source_kind`
  (`manual`, `api`, `cron`, `system`, `subflow`, …; missing → `null/unknown`).
- **workflow run summary** — total runs, succeeded count, success rate, and run
  duration min / avg / max in seconds. Duration is `finished_at - started_at`
  (both are integer Unix-epoch seconds in the dump), counted only for runs where
  both timestamps are present.
- **dump summary by source_table** — record count and `created_at` epoch range
  for every table seen across the input (`workflow_runs`, `audit_log`,
  `dead_letters`, …).

## Build

Requires GnuCOBOL (`cobc`) 3.x, which ships the report writer.

```sh
make            # cobc -x -free -Wall archive-report.cob -o archive-report
```

## Run

The program reads a **decompressed** `.jsonl` file (COBOL never handles gzip).

```sh
# directly, on an already-decompressed file:
./archive-report path/to/dump.jsonl

# on real (gzip'd) dumps, via the wrapper — it decompresses first:
./run.sh /var/lib/runinator/archive/2026-06-21/*.jsonl.gz

# plain and gz arguments can be mixed; they are concatenated:
./run.sh dumpA.jsonl dumpB.jsonl.gz

# capture to a file with normal shell redirection:
./archive-report dump.jsonl > report.txt
```

If invoked with no argument the program reads `/dev/stdin`.

## Test

```sh
make test       # builds, then runs against the bundled sample.jsonl
```

`sample.jsonl` contains seven `workflow_runs` (mixed statuses, trigger sources,
and durations — including null timestamps) plus an `audit_log` and a
`dead_letters` line. One run's `workflow_snapshot` embeds an escaped
`\"status\":\"draft\"` blob to confirm the scanner only matches real top-level
keys, not quoted values inside JSON strings.

## How it works

Each NDJSON line is one self-contained JSON object, so the program scans per
line with `UNSTRING` using the JSON key as a delimiter (e.g. `"status":"`).
Quotes inside JSON string values are escaped as `\"`, so a bare `"key":"`
delimiter only ever matches genuine top-level/row keys. This avoids depending on
`JSON PARSE` support and stays robust to the variable `row` schema across tables.

## Notes

- The input record buffer is large (2 MB) because `workflow_snapshot`,
  `parameters`, `state`, and `trigger_metadata` are serialized as JSON strings
  and can make a `workflow_runs` line long.
- Field/schema reference: archive envelope is built in
  `runinator-archiver/src/main.rs`; per-table `row` payloads in
  `runinator-database/src/operations.rs` (`archive_row_json`).
