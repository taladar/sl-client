# Conformance Testing

The `sl-conformance` crate is a manual, live-grid test harness. Unlike the
`cargo test` suites elsewhere in the workspace, its tests log in to a real grid
and exercise a library feature end to end, then record a git-stamped result into
the committed `records/` tree.

## Why a separate harness

Feature-level behaviour can only be verified against a live grid: the local
OpenSim (`http://127.0.0.1:9000/`) or Second Life Beta, "aditi". These tests:

- need a running grid and real logins, so they cannot run on every commit;
- must not all run at once — many logins in a short window on aditi risk
  rate-limiting or a temporary ban.

So the harness is deliberately *not* wired into `cargo test`. You run one test,
against one grid, when you want to check that feature on that grid.

## The two binaries

- `sl-conformance` — the runner. Logs in, runs exactly one test, and appends the
  result to that test's record. There is no "run all" command, by design.
- `sl-conformance-report` — a read-only summary. It reads `records/` only (no
  network) and prints a `cargo test`-style table: a status per grid, with
  per-metric performance trends and commit-freshness annotations. It exits
  non-zero if any recorded run failed, so it can gate scripts.

## The workflow

1. Configure credentials. Both grids use the `sl-repl` credentials TOML format
   (named avatars; aditi carries an `mfa_command`). The runner defaults to
   `credentials.toml` for OpenSim and `credentials.aditi.toml` for aditi, or use
   `--credentials <path>`.
2. Run one test:

   ```sh
   sl-conformance run --grid opensim inventory-fetch
   ```

3. Inspect the recorded results:

   ```sh
   sl-conformance-report
   ```

See [The Runner](runner.md) for the full command surface and how to add a test,
and [Records & the Dirty Rule](records.md) for the record format and the
behaviour-aware describe.
