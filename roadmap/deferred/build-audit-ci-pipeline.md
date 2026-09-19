---
id: build-audit-ci-pipeline
title: There is no CI — every quality gate is a local pre-commit hook
topic: viewer
status: deferred
origin: static code audit (2026-08-26)
points: 8
---

Context: [context/viewer.md](../context/viewer.md).

The repository has **no `.github/workflows/`** and no other CI configuration.
`cargo build`, `cargo test` and `cargo clippy` run only in the local `ggh`
pre-commit hook, on one machine.

For 815k lines across 68 crates with 364 commits in the last 30 days, nothing
verifies that a clean checkout builds — after a dependency bump, a `[patch]`
rev change, a rustc update, or a crate split. The `#[ignore]`d tests
(`sl-client-bevy-viewer/tests/uv_seams.rs`) and the whole live-grid conformance
suite are manual by design, which makes the automated tier the only regression
net there is.

Scope: a workflow that runs on push and PR — `cargo build --workspace`,
`cargo test --workspace`, `cargo clippy --workspace --all-targets`,
`cargo fmt --check`, and `python3 roadmap/index.py --check`. Two known costs to
plan around: the `cef-dll-sys` build script downloads the CEF distribution on
first build (cache `.cef/`), and two viewer-crate `rustc` processes running
concurrently OOM on constrained runners, so the job needs `-j` limited.

Worth adding once green: `cargo deny check advisories`, which the sibling
`sl-map-tools` workspace already gates on.

## Deferred (2026-09-19)

Not while the workspace is developed exclusively on one machine and takes a
dozen commits a day: building 815k lines of Rust on every push to GitHub buys
nothing the `ggh` pre-commit hook has not already checked on the same tree, and
costs minutes per push. The two things CI would genuinely add — *does a fresh
checkout build* and *does it build somewhere other than here* — are release
preparation concerns, so this comes back with the first release, not before.
