       >>SOURCE FORMAT FREE
*> archive-report: metrics report over runinator archive dumps.
*> reads decompressed ndjson (one json envelope per line) and prints a
*> report-section report. consumes the jsonl dumps only, never the database.
identification division.
program-id. archive-report.

environment division.
configuration section.
input-output section.
file-control.
    *> input is a plain .jsonl stream/file; gzip is handled by run.sh.
    select in-file assign to ws-input-path
        organization is line sequential
        file status is ws-in-status.
    *> report writer to the display device (stdout); redirect with the shell to
    *> capture to a file. assigning to a device avoids the lock-on-close warning
    *> gnucobol emits when closing a piped /dev/stdout.
    select report-file assign to display
        organization is line sequential
        file status is ws-rep-status.

data division.
file section.
fd  in-file.
*> generous record so long snapshot/parameters/state blobs are not truncated.
01  in-record            pic x(2000000).

fd  report-file report is metrics-report.

working-storage section.

*> -- file plumbing --
01  ws-input-path        pic x(1024) value "/dev/stdin".
01  ws-in-status         pic xx value "00".
01  ws-rep-status        pic xx value "00".
01  ws-eof-flag          pic x value "n".
    88  end-of-input     value "y".

*> -- counters --
01  ws-lines-read        pic 9(9) value 0.
01  ws-run-total         pic 9(9) value 0.

*> -- one input line plus its scanned remainder --
01  ws-line              pic x(2000000).
01  ws-line-tail         pic x(2000000).
01  ws-junk              pic x(256).

*> -- extracted scalar fields --
01  ws-source-table      pic x(40).
01  ws-status            pic x(40).
01  ws-trigger           pic x(40).
01  ws-num-text          pic x(20).
01  ws-num-value         pic s9(13) value 0.
01  ws-nt-idx            pic 9(2).
01  ws-nt-pos            pic 9(2).
01  ws-created-at        pic s9(13) value 0.
01  ws-started-at        pic s9(13) value 0.
01  ws-finished-at       pic s9(13) value 0.
01  ws-has-started       pic x value "n".
01  ws-has-finished      pic x value "n".
01  ws-duration          pic s9(13) value 0.

*> -- status buckets (12 known + other) --
01  ws-status-table.
    05  ws-status-row occurs 13 times indexed by si.
        10  ws-status-name   pic x(20).
        10  ws-status-count  pic 9(9).
01  ws-status-pct        pic zz9.9.

*> -- trigger-source buckets (9 known + null/unknown) --
01  ws-trigger-table.
    05  ws-trigger-row occurs 10 times indexed by ti.
        10  ws-trigger-name  pic x(20).
        10  ws-trigger-count pic 9(9).

*> -- per-source_table summary --
01  ws-tbl-table.
    05  ws-tbl-row occurs 32 times indexed by bi.
        10  ws-tbl-name      pic x(40).
        10  ws-tbl-count     pic 9(9).
        10  ws-tbl-min-ca    pic s9(13).
        10  ws-tbl-max-ca    pic s9(13).
01  ws-tbl-used          pic 9(4) value 0.

*> -- duration aggregates --
01  ws-dur-count         pic 9(9) value 0.
01  ws-dur-sum           pic 9(15) value 0.
01  ws-dur-min           pic 9(13) value 0.
01  ws-dur-max           pic 9(13) value 0.
01  ws-dur-avg           pic 9(13) value 0.

*> -- derived headline figures --
01  ws-succeeded-count   pic 9(9) value 0.
01  ws-success-rate      pic zz9.9.

*> -- report scratch --
01  ws-found             pic x value "n".
01  ws-label             pic x(40).
01  ws-kv-label          pic x(28).
01  ws-kv-num            pic s9(13).

*> -- timestamp for the page heading --
01  ws-now.
    05  ws-now-date      pic 9(8).
    05  ws-now-time      pic 9(8).
01  ws-now-disp          pic x(19).

report section.
rd  metrics-report
    page limit 66 lines
    heading 1
    first detail 6
    last detail 62
    footing 64.

01  rh-page type page heading.
    05  line 1.
        10  column 1  pic x(29)
            value "runinator archive dump report".
    05  line 2.
        10  column 1  pic x(11) value "generated: ".
        10  column 12 pic x(19) source ws-now-disp.
    05  line 3.
        10  column 1  pic x(14) value "lines parsed: ".
        10  column 15 pic zzz,zz9 source ws-lines-read.
    05  line 4.
        10  column 1  pic x(60)
            value "============================================================".

01  d-section type detail.
    05  line plus 1.
        10  column 1  pic x(40) source ws-label.

01  d-status type detail.
    05  line plus 1.
        10  column 3  pic x(20) source ws-status-name (si).
        10  column 25 pic zzz,zz9 source ws-status-count (si).
        10  column 35 pic zz9.9 source ws-status-pct.
        10  column 41 pic xx value " %".

01  d-trigger type detail.
    05  line plus 1.
        10  column 3  pic x(20) source ws-trigger-name (ti).
        10  column 25 pic zzz,zz9 source ws-trigger-count (ti).

01  d-table type detail.
    05  line plus 1.
        10  column 3  pic x(40) source ws-tbl-name (bi).
        10  column 44 pic zzz,zz9 source ws-tbl-count (bi).
        10  column 53 pic x(8) value "min_ca: ".
        10  column 61 pic 9(10) source ws-tbl-min-ca (bi).
        10  column 73 pic x(8) value "max_ca: ".
        10  column 81 pic 9(10) source ws-tbl-max-ca (bi).

01  d-kv type detail.
    05  line plus 1.
        10  column 3  pic x(28) source ws-kv-label.
        10  column 31 pic zzz,zzz,zz9 source ws-kv-num.

01  d-rate type detail.
    05  line plus 1.
        10  column 3  pic x(28) source ws-kv-label.
        10  column 33 pic zz9.9 source ws-success-rate.
        10  column 39 pic xx value " %".

procedure division.
main-procedure.
    perform init-tables
    perform get-args
    perform get-now
    open input in-file
    if ws-in-status not = "00"
        display "cannot open input: " function trim (ws-input-path)
            upon syserr
        stop run returning 1
    end-if
    open output report-file
    initiate metrics-report
    perform read-loop until end-of-input
    close in-file
    perform finalize-figures
    perform emit-report
    terminate metrics-report
    close report-file
    stop run.

*> seed the fixed status and trigger buckets in stable display order.
init-tables.
    move "queued"            to ws-status-name (1)
    move "running"           to ws-status-name (2)
    move "paused"            to ws-status-name (3)
    move "debug_paused"      to ws-status-name (4)
    move "waiting"           to ws-status-name (5)
    move "approval_required" to ws-status-name (6)
    move "input_required"    to ws-status-name (7)
    move "blocked"           to ws-status-name (8)
    move "succeeded"         to ws-status-name (9)
    move "failed"            to ws-status-name (10)
    move "timed_out"         to ws-status-name (11)
    move "canceled"          to ws-status-name (12)
    move "other"             to ws-status-name (13)
    perform varying si from 1 by 1 until si > 13
        move 0 to ws-status-count (si)
    end-perform

    move "manual"         to ws-trigger-name (1)
    move "api"            to ws-trigger-name (2)
    move "cron"           to ws-trigger-name (3)
    move "system"         to ws-trigger-name (4)
    move "worker_control" to ws-trigger-name (5)
    move "replay"         to ws-trigger-name (6)
    move "debug"          to ws-trigger-name (7)
    move "subflow"        to ws-trigger-name (8)
    move "map"            to ws-trigger-name (9)
    move "null/unknown"   to ws-trigger-name (10)
    perform varying ti from 1 by 1 until ti > 10
        move 0 to ws-trigger-count (ti)
    end-perform.

get-args.
    accept ws-input-path from argument-value
    *> empty argument keeps the /dev/stdin default.
    if ws-input-path = spaces
        move "/dev/stdin" to ws-input-path
    end-if.

get-now.
    move function current-date to ws-now
    string ws-now-date (1:4) "-" ws-now-date (5:2) "-" ws-now-date (7:2)
        " " ws-now-time (1:2) ":" ws-now-time (3:2) ":" ws-now-time (5:2)
        delimited by size into ws-now-disp
    end-string.

read-loop.
    read in-file into ws-line
        at end
            set end-of-input to true
        not at end
            add 1 to ws-lines-read
            perform parse-line
    end-read.

*> extract just the metric-bearing fields from one ndjson envelope line.
parse-line.
    move spaces to ws-source-table
    move spaces to ws-status
    move "null/unknown" to ws-trigger
    move 0 to ws-created-at
    move "n" to ws-has-started
    move "n" to ws-has-finished

    *> top-level source_table is the routing key for everything else.
    perform extract-source-table
    if ws-source-table = spaces
        exit paragraph
    end-if

    *> top-level created_at (epoch seconds) feeds the per-table date range.
    perform extract-created-at
    perform tally-table

    *> guard: only workflow_runs carries the deep run metrics.
    if ws-source-table not = "workflow_runs"
        exit paragraph
    end-if

    add 1 to ws-run-total
    perform extract-status
    perform tally-status
    perform extract-trigger
    perform tally-trigger
    perform extract-durations
    perform tally-duration.

*> -- field extractors --
*> a bare "key":" delimiter only matches real top-level/row keys, since
*> quotes inside json string blobs are escaped as \" and never match.

extract-source-table.
    move spaces to ws-line-tail
    unstring ws-line delimited by '"source_table":"'
        into ws-junk ws-line-tail
    end-unstring
    if ws-line-tail not = spaces
        unstring ws-line-tail delimited by '"' into ws-source-table
    end-if.

extract-status.
    move spaces to ws-line-tail
    unstring ws-line delimited by '"status":"'
        into ws-junk ws-line-tail
    end-unstring
    if ws-line-tail not = spaces
        unstring ws-line-tail delimited by '"' into ws-status
    end-if.

extract-trigger.
    move spaces to ws-line-tail
    unstring ws-line delimited by '"trigger_source_kind":"'
        into ws-junk ws-line-tail
    end-unstring
    if ws-line-tail not = spaces
        unstring ws-line-tail delimited by '"' into ws-trigger
    end-if.

extract-created-at.
    move spaces to ws-line-tail
    unstring ws-line delimited by '"created_at":'
        into ws-junk ws-line-tail
    end-unstring
    if ws-line-tail not = spaces
        perform read-number
        move ws-num-value to ws-created-at
    end-if.

extract-durations.
    *> started_at.
    move spaces to ws-line-tail
    unstring ws-line delimited by '"started_at":'
        into ws-junk ws-line-tail
    end-unstring
    if ws-line-tail not = spaces and ws-line-tail (1:4) not = "null"
        perform read-number
        move ws-num-value to ws-started-at
        move "y" to ws-has-started
    end-if
    *> finished_at.
    move spaces to ws-line-tail
    unstring ws-line delimited by '"finished_at":'
        into ws-junk ws-line-tail
    end-unstring
    if ws-line-tail not = spaces and ws-line-tail (1:4) not = "null"
        perform read-number
        move ws-num-value to ws-finished-at
        move "y" to ws-has-finished
    end-if.

*> read a leading run of digits from ws-line-tail into ws-num-value.
read-number.
    move spaces to ws-num-text
    move 1 to ws-nt-idx
    perform varying ws-nt-pos from 1 by 1 until ws-nt-pos > 18
        if ws-line-tail (ws-nt-pos:1) is numeric
            move ws-line-tail (ws-nt-pos:1) to ws-num-text (ws-nt-idx:1)
            add 1 to ws-nt-idx
        else
            exit perform
        end-if
    end-perform
    if ws-num-text = spaces
        move 0 to ws-num-value
    else
        compute ws-num-value = function numval (ws-num-text)
    end-if.

*> -- tallies --

tally-table.
    move "n" to ws-found
    perform varying bi from 1 by 1 until bi > ws-tbl-used
        if ws-tbl-name (bi) = ws-source-table
            add 1 to ws-tbl-count (bi)
            if ws-created-at < ws-tbl-min-ca (bi)
                move ws-created-at to ws-tbl-min-ca (bi)
            end-if
            if ws-created-at > ws-tbl-max-ca (bi)
                move ws-created-at to ws-tbl-max-ca (bi)
            end-if
            move "y" to ws-found
            exit perform
        end-if
    end-perform
    if ws-found = "n" and ws-tbl-used < 32
        add 1 to ws-tbl-used
        move ws-source-table to ws-tbl-name (ws-tbl-used)
        move 1 to ws-tbl-count (ws-tbl-used)
        move ws-created-at to ws-tbl-min-ca (ws-tbl-used)
        move ws-created-at to ws-tbl-max-ca (ws-tbl-used)
    end-if.

tally-status.
    move "n" to ws-found
    perform varying si from 1 by 1 until si > 12
        if ws-status-name (si) = ws-status
            add 1 to ws-status-count (si)
            move "y" to ws-found
            exit perform
        end-if
    end-perform
    if ws-found = "n"
        add 1 to ws-status-count (13)
    end-if.

tally-trigger.
    perform varying ti from 1 by 1 until ti > 10
        if ws-trigger-name (ti) = ws-trigger
            add 1 to ws-trigger-count (ti)
            exit perform
        end-if
    end-perform.

tally-duration.
    if ws-has-started = "y" and ws-has-finished = "y"
        compute ws-duration = ws-finished-at - ws-started-at
        if ws-duration >= 0
            add 1 to ws-dur-count
            add ws-duration to ws-dur-sum
            if ws-dur-count = 1
                move ws-duration to ws-dur-min
                move ws-duration to ws-dur-max
            else
                if ws-duration < ws-dur-min
                    move ws-duration to ws-dur-min
                end-if
                if ws-duration > ws-dur-max
                    move ws-duration to ws-dur-max
                end-if
            end-if
        end-if
    end-if.

finalize-figures.
    *> succeeded count drives the success-rate headline.
    move ws-status-count (9) to ws-succeeded-count
    if ws-run-total > 0
        compute ws-success-rate rounded =
            ws-succeeded-count * 100 / ws-run-total
    else
        move 0 to ws-success-rate
    end-if
    if ws-dur-count > 0
        compute ws-dur-avg rounded = ws-dur-sum / ws-dur-count
    else
        move 0 to ws-dur-avg
    end-if.

*> -- report emission via the report writer --

emit-report.
    move "workflow_runs status breakdown" to ws-label
    generate d-section
    move all "-" to ws-label
    generate d-section
    perform varying si from 1 by 1 until si > 13
        if ws-status-count (si) > 0
            if ws-run-total > 0
                compute ws-status-pct rounded =
                    ws-status-count (si) * 100 / ws-run-total
            else
                move 0 to ws-status-pct
            end-if
            generate d-status
        end-if
    end-perform

    move spaces to ws-label
    generate d-section
    move "trigger source breakdown" to ws-label
    generate d-section
    move all "-" to ws-label
    generate d-section
    perform varying ti from 1 by 1 until ti > 10
        if ws-trigger-count (ti) > 0
            generate d-trigger
        end-if
    end-perform

    move spaces to ws-label
    generate d-section
    move "workflow run summary" to ws-label
    generate d-section
    move all "-" to ws-label
    generate d-section
    move "total runs" to ws-kv-label
    move ws-run-total to ws-kv-num
    generate d-kv
    move "succeeded" to ws-kv-label
    move ws-succeeded-count to ws-kv-num
    generate d-kv
    move "success rate" to ws-kv-label
    generate d-rate
    move "runs with duration" to ws-kv-label
    move ws-dur-count to ws-kv-num
    generate d-kv
    move "duration min (s)" to ws-kv-label
    move ws-dur-min to ws-kv-num
    generate d-kv
    move "duration avg (s)" to ws-kv-label
    move ws-dur-avg to ws-kv-num
    generate d-kv
    move "duration max (s)" to ws-kv-label
    move ws-dur-max to ws-kv-num
    generate d-kv

    move spaces to ws-label
    generate d-section
    move "dump summary by source_table" to ws-label
    generate d-section
    move all "-" to ws-label
    generate d-section
    perform varying bi from 1 by 1 until bi > ws-tbl-used
        generate d-table
    end-perform.

end program archive-report.
