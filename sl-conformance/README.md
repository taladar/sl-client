# sl-conformance

A manual, live-grid conformance test harness for the `sl-client` workspace.

Unlike `cargo test`, these tests log in to a real grid (the local OpenSim or
Second Life Beta "aditi") and exercise one library feature at a time. Each run
records a git-stamped result — the commit at which the feature was last verified
on that grid, plus per-test metrics — into the committed `records/` tree.

Two binaries:

- `sl-conformance` — the runner. Runs exactly one test per invocation against
  one grid. There is deliberately no "run all" command.
- `sl-conformance-report` — a read-only summary that renders the recorded
  results in a test-suite style with per-metric performance trends.

See the book chapter "Conformance testing" for the full workflow, record format,
and the behaviour-aware git-describe dirty rule.
