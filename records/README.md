# Conformance records

This directory holds the committed results of the `sl-conformance` live-grid
test harness — one file per `(test, grid)` pair at
`records/<grid>/<test>.toml`.

Each file is written by `sl-conformance run` and read by
`sl-conformance-report`. It records, for the most recent runs, the git commit at
which the feature was last exercised on that grid (a behaviour-aware describe,
with a `-dirty` suffix when the source tree had uncommitted behaviour changes),
the pass/fail outcome, whether the run was complete, and any metrics the test
wrote.

These files are meant to be committed: they are the durable record of what has
been verified, where, and when. See the book chapter "Conformance Testing" for
the format and the dirty rule.

Changes under this directory are deliberately *not* treated as behavioural by
the dirty rule, so committing updated records here does not make a later run
report itself as dirty.
