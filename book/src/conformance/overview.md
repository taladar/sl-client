# Conformance Testing

The `sl-conformance` crate is a live-grid test harness. Its tests log in to a
grid and exercise a library feature end to end, then record a git-stamped result
into the committed `records/` tree.

## Why a separate harness

Most feature-level behaviour can only be verified against a live grid: the local
OpenSim (`http://127.0.0.1:9000/`) or Second Life Beta, "aditi". Those tests:

- need a running grid and real logins, so they cannot run on every commit;
- must not all run at once — many logins in a short window on aditi risk
  rate-limiting or a temporary ban.

So the harness is deliberately *not* wired into `cargo test`. You run one test,
against one grid, when you want to check that feature on that grid.

## The exception: the offline grid

Not every case needs a grid somebody stood up. A case that asserts protocol
*shape* — a handshake, a ping, a throttle, a parcel record, the world map, a
region crossing — needs only fixtures the workspace already ships, and
`Grid::Fake` gives it those: an `sl-fake-grid` started inside the test process
on ephemeral ports, serving the same fixture catalogue the viewer's render
harness photographs.

Those cases (`sl_conformance::fake::OFFLINE_CASES`) run as ordinary `cargo test`
tests in `sl-conformance/tests/offline.rs`, so they are exercised on every
commit rather than the next time somebody logs in. They write no record: the
assertion is re-made from scratch each run, so a committed copy of it could only
be staler. Everything asserting grid *semantics* — groups, estates, money,
experiences, display names, offline IM, the marketplace, AIS3 — stays live.

See [The Runner](runner.md#the-offline-grid) for what qualifies and how to add
one.

## The two binaries

- `sl-conformance` — the runner. Logs in, runs exactly one test, and appends the
  result to that test's record. There is no "run all" command, by design.
- `sl-conformance-report` — a read-only summary. It reads `records/` only (no
  network) and prints a `cargo test`-style table: a status per grid, with
  per-metric performance trends and commit-freshness annotations. It exits
  non-zero if any recorded run failed, so it can gate scripts.

## The workflow

1. Configure credentials. Both live grids use the `sl-repl` credentials TOML
   format (named avatars; aditi carries an `mfa_command`). The runner defaults
   to `credentials.toml` for OpenSim and `credentials.aditi.toml` for aditi, or
   use `--credentials <path>`. `--grid fake` needs none: it registers its own
   accounts as it starts.
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
