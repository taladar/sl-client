# Records & the Dirty Rule

Each `(test, grid)` pair has one committed record at
`records/<grid>/<test>.toml`. A record keeps a bounded history of recent runs so
the reporter can compare the latest run to the previous one.

## The record format

```toml
test = "inventory-fetch"
grid = "opensim"

[[run]]  # appended each run; oldest trimmed
behavior_describe = "v0.1.0-87-g1a2b3c4"
dirty = false
outcome = "pass"  # "pass" | "fail"
completeness = "complete"  # "complete" | "partial"
recorded_at = "2026-06-28T19:42:11Z"  # RFC 3339, UTC
sl_conformance_version = "0.1.0"

[run.metrics]  # free-form, written by the test
inventory_fetch_secs = 4.21
root_folders = 312

[run.metric_meta]  # per-metric direction / completeness
inventory_fetch_secs = { lower_is_better = true }
```

Each run records:

- `behavior_describe` — the behaviour-aware describe (see below).
- `dirty` — whether the behaviour-relevant tree was dirty at run time.
- `outcome` — `pass` or `fail`. Failures are recorded too: a committed `fail` is
  a visible regression signal.
- `completeness` (+ optional `completeness_note`) — whether the run covered the
  full dataset. A partial run's counts are never compared against a complete
  run's.
- `metrics` and `metric_meta` — the measurements and their direction hints.

The history is capped at ten runs; git history preserves anything older.

## The behaviour-aware dirty rule

A record stamps the commit at which a feature was last verified. A plain
`git describe --dirty` would flag the tree dirty whenever *any* file changed —
including the record files this harness writes and the documentation — neither
of which changes runtime behaviour.

So the harness computes the describe itself and applies a `-dirty` suffix only
when a **behaviour-relevant** path differs. A changed path is *not* behavioural
(and so is ignored) when it is:

- under `records/`,
- under `book/`,
- any `*.md` file, or
- a changelog file.

Everything else is behavioural — notably `*.rs`, `Cargo.toml`/`Cargo.lock`, the
message template, and build scripts. Modified, staged, and untracked files all
count.

## The report

`sl-conformance-report` reads the records and prints one row per test with a
status per grid:

- `ok` / `FAILED` / `· never ran` / `n/a` (not applicable to that grid).
- A commit-freshness annotation derived by comparing the newest run's commit to
  the current checkout:
  - nothing when the run is at the current commit and clean;
  - `(dirty@current)` when the run is at the current commit but was recorded on
    an uncommitted tree;
  - `(stale: N commits behind @ <describe>)` when the run is at an older commit;
  - `(@ <describe>)` when git cannot determine the current commit.
- `(partial: ...)` when the newest run was partial.

Under each row, for every metric present in both the newest and previous runs,
the report prints `old -> new (Δ%)`. The change is coloured improved/worse only
when the metric is complete in **both** runs and has a known direction; partial
or directionless metrics are shown without a verdict. The footer tallies
`ok / FAILED / never ran` per grid, and the process exits non-zero if any run
failed.
