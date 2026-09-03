# sl-conformance

A conformance test harness for the `sl-client` workspace: one case per library
feature, each driven against a grid.

Most of these tests log in to a real grid (the local OpenSim or Second Life Beta
"aditi") and are run by hand, one at a time. Each such run records a git-stamped
result — the commit at which the feature was last verified on that grid, plus
per-test metrics — into the committed `records/` tree.

The exception is `--grid fake`: an `sl-fake-grid` started inside the process on
ephemeral ports, with its own accounts and no network. The cases listed in
`fake::OFFLINE_CASES` run against it as ordinary `cargo test` tests
(`tests/offline.rs`), so they are exercised on every commit and write no record.

Two binaries:

- `sl-conformance` — the runner. Runs exactly one test per invocation against
  one grid. There is deliberately no "run all" command.
- `sl-conformance-report` — a read-only summary that renders the recorded
  results in a test-suite style with per-metric performance trends.

See the book chapter "Conformance testing" for the full workflow, record format,
and the behaviour-aware git-describe dirty rule.
