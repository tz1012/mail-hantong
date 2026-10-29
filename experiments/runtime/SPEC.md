# Runtime comparison experiment v1

This is an authorized, disposable ADR-001 experiment, not product code or a
product API/IPC/storage contract. No accounts, credentials, network operations,
message sending, approval issuance, or GUI are implemented. OPS-01 targets are
context only; these measurements cannot satisfy product acceptance criteria.

## Comparable behavior

Both executables use only their language/platform standard libraries. Build
Rust in release mode and .NET 10 as Native AOT, optimized for speed. Use the same
file bytes and commands. Keep ordinary owned strings in both implementations;
do not introduce a custom index or zero-copy storage for only one implementation.

The cache is UTF-8 without BOM, LF-separated TSV, with no header:

`id\taccount_id\tutc_seconds\tunread\tsubject\tsender\tsnippet`

IDs are positive integers, account IDs are 1..5, UTC seconds are signed 64-bit,
unread is 0 or 1. Text fields contain no tabs/newlines. Preserve Unicode. Parse
once into an array/vector of records, then sort descending by (utc_seconds, id).
Synthetic input is deliberately unsorted. Each record owns its text fields;
release the input buffer after parsing. Input errors yield nonzero exit code and
a generic error without file contents. No real mail data may be used.

## Experiment-only command line and line protocol

- `--ping`: write `PONG` plus LF and exit successfully, without opening files.
- `--serve CACHE BODY`: load and sort CACHE, then write `READY\tN` and flush.
- Read commands from UTF-8 standard input, respond with one flushed UTF-8 line.
- `LIST\tACCOUNT`: first 100 records of that account in sorted order; response
  `LIST\tCOUNT\tID1,ID2,...`. No accounts outside the specified one.
- `SEARCH\tACCOUNT\tQUERY`: case-sensitive literal substring in subject OR
  snippet, scoped to account. Return all matches aggregated as
  `SEARCH\tCOUNT\tSUM_OF_IDS` (64-bit sum); do not limit scanning to 100.
- `BODY`: read the fixed BODY path as bytes on every request; return
  `BODY\tBYTE_COUNT\tSUM_OF_BYTES` (64-bit sum). This is cached body file I/O,
  not MIME parsing, rendering, encoding detection, or sanitization.
- `PING`: `PONG`.
- `QUIT`: write `BYE`, flush, and exit successfully.
- Invalid/empty account, malformed/unknown command, or empty search term:
  `ERR`, keep running. Never interpret an empty/invalid account as all accounts.
- EOF exits successfully. Do not print diagnostics or timing to stdout beyond
  this protocol. Errors on stderr must be generic.

## Verification before timing

Python standard-library tests supply an independent oracle for row ordering,
account isolation, English/Korean substring queries, missing results, body byte
counts/checksums, and invalid input. The runner checks measured responses too.
No performance result is accepted if outputs differ or a process fails.

## Measurement boundaries

Warm OS file cache; do not claim disk-cold starts. Process launch + ping/exit,
process launch + 100k-row load/sort + READY, and resident command round-trips
are separate metrics. Pipes include Python/OS overhead and are not the future
product IPC implementation. Rotate candidate order between rounds, use warmup,
retain every sample, and report median/p95 (nearest-rank), CPU and live/peak
working set/private bytes where available, executable size and exact builds.
No timing runs concurrent with builds/tests. Do not force antivirus changes,
purge OS caches, change global power settings, or access actual accounts.

Default fixture: 100,000 records, 5 accounts, mixed Korean/English text, and a
128 KiB UTF-8 body. Record fixture SHA-256, environment and tool versions.
Small fixture tests are independent of the default performance fixture.

Interpretation: a useful initial signal about these two implementations on this
machine. Actual DB/MIME/provider libraries, authenticated IPC, policy/approval,
GUI readiness, cold-disk behavior, concurrency, sustained sync and macOS remain
unmeasured. Language adoption requires this limited evidence plus those checks.
