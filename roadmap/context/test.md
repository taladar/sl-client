# Context — TEST_ROADMAP.md

Non-task preamble carried over from `TEST_ROADMAP.md`. Tasks split out of that
file carry the `test` topic.

A staged plan for growing the `sl-conformance` live-grid suite from its current
four behaviours (`login-handshake`, `inventory-fetch`, `asset-decode`,
`region-info`) to comprehensive coverage of the protocol surface and the
higher-level flows built on top of it (chat sessions, inventory, teleport,
groups, ...). Every case runs against live grids — local **OpenSim** and the
Second Life **Aditi** beta grid — not as `cargo test` units.

This file is a plan, not test code. Future sessions implement one phase (or one
case) at a time, run it live, commit the generated record, and tick the box
here. For *how* the harness works and how runs are recorded, see the book:
`book/src/conformance/{overview,runner,records}.md`.

## How to add a case (recap)

Three mechanical steps, modelled on the existing cases under
`sl-conformance/src/cases/`:

1. Add `sl-conformance/src/cases/<name>.rs` with a unit struct that implements
   `GridTest` (`sl-conformance/src/registry.rs`): `name()` (kebab-case, also the
   record file stem), `description()`, `grids()`, optional `accounts()`, and the
   async `run()` body.
2. Add `pub mod <name>;` to `sl-conformance/src/cases.rs`.
3. Add `Box::new(crate::cases::<name>::<Struct>)` to `registry()` in
   `sl-conformance/src/registry.rs`.

Inside `run()`, drive the live session(s):

- `ctx.primary()` / `ctx.secondary()` (and, once added, `ctx.tertiary()`) yield
  `Session` handles.
- `session.wait_for_region(timeout).await?` gates on the region handshake.
- `session.send(Command::...).await?` issues a command.
- `session.wait_for(timeout, |event| match event { ... })` awaits a typed
  `Event`. The `Command`/`Event` surface lives in `sl-client-tokio/src/lib.rs`;
  the state machines (teleport phases, sit, chat sessions, inventory) live in
  `sl-proto/src/session.rs`.
- `ctx.metrics().set("k", v)` / `.set_timing("k_secs", secs)` record values.
- Fail an assertion with `Err(TestFailure::Assertion("...".to_owned()))`.
- `ctx.mark_partial("reason")` flags a legitimately incomplete dataset instead
  of failing (e.g. a grid that omits a field).

Run and record:

```sh
sl-conformance run --grid opensim <name>
sl-conformance run --grid aditi  <name> --force   # --force skips cooldown
sl-conformance-report                              # green = Current
```

## Legend & conventions

- Grid gating: `[both]`, `[opensim]` (OpenSim only), `[aditi]` (SL only).
- Account count: `1av`, `2av`, `3av` (see Phase 0 and Phase Z).
- Status: `[ ]` todo, `[x]` done (tick when the live record is committed green).
- Prefer asserting an observable protocol effect (a field value, a state
  transition) over only timing it. Keep a timing metric anyway — the reporter
  tracks regressions.
- Record meaningful metrics: counts, timings (`*_secs`), codec/format names.
- Use `mark_partial` (not failure) when a grid legitimately returns less data.
- Keep timeouts generous for Aditi (network + MFA + load).
- Respect the Aditi 120 s per-avatar cooldown; serialise multi-avatar Aditi
  logins and expect long wall-clock.

## Grid capability differences (for gating)

- **SL only** (`[aditi]`): Experiences, Display Names, Voice provisioning,
  god-bit enforcement, modern CAPS-only flows where OpenSim has no equivalent.
- **OpenSim only** (`[opensim]`): OpenRegionInfo limits bag, Hypergrid
  teleport, per-estate physics/scripting restriction.
- **Auto-selected** (write once, runs on both): inventory fetch picks CAPS
  `FetchInventoryDescendents2` vs UDP `FetchInventoryDescendents` per region.
- Several OpenSim features are **OFF by default** and need a config/module step
  before they can be tested — see the Setup-cost appendix.

---
