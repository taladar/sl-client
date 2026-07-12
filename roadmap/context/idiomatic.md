# Context — IDIOMATIC_ROADMAP.md

Non-task preamble carried over from `IDIOMATIC_ROADMAP.md`. Tasks split out of
that file carry the `idiomatic` topic.

The protocol surface is broad and complete (client + server, both directions).
This road map closes a different kind of gap: places where the **type system
could prevent misuse** but currently does not, because raw integers, `Uuid`s,
`bool`s, and magic constants carry semantics the compiler can't see. The intent
is to make illegal states unrepresentable — the classic hardening pass for code
conceptually ported from a less type-safe origin (C++/C#).

**Out of scope (already enforced, verified clean):** memory/panic safety. Every
crate enforces ~175 restriction clippy lints (`unsafe_code = "forbid"`,
`unwrap_used`/`expect_used`/`panic` denied, `indexing_slicing` denied,
`as_conversions` denied, `arithmetic_side_effects` denied, `must_use_candidate`
denied, `unused_must_use` forbidden). There is zero `unsafe`, zero
`unwrap`/`expect`/`panic`, bounds-checked wire parsing, and capped allocations.
Do **not** spend effort re-deriving `#[must_use]` or hunting panics — the lints
already own that.

Work the phases top-to-bottom (high-ROI / low-risk first); tick a box only when
the step builds, is clippy-clean under the restriction lints, and
`cargo test --workspace` passes. Add sub-tasks as you discover them.

## Scope reminders

- Commit on the current branch only (never auto-create a feature branch).
- Keep `sl-client-tokio` and `sl-client-bevy` at feature parity (land mirrored
  changes together).
- `sl-types` is normally **consume-only** — new client wrappers live in
  `sl-proto`/`sl-wire`. The only sanctioned `sl-types` *additions* are general
  SL concepts (not client-only): `LindenBalance` (Phase 6) and any new union-key
  enums modelled on `OwnerKey` (Phase 5). List each such addition explicitly.
- SL (Linden Lab) is the primary target; OpenSim is only the safe test grid.

## The per-step refactor pattern

These are refactors, not new capabilities, so the 9-step per-capability pattern
collapses to a uniform sweep. For each type change:

1. **Change the type** in `sl-proto`/`sl-wire` (or consume the `sl-types` type).
2. **Fix every codec site** — encode/decode/conversion in
   `sl-proto/src/session/{conversions,methods,circuit}.rs`, `sim_session.rs`,
   and the wire layer. Wrap/unwrap only at the codec boundary so the wire bytes
   are byte-identical to before.
3. **Fix downstream** — `sl-repl/src/registry.rs` arg parsing and
   `sl-repl/src/format.rs` rendering; `sl-client-tokio/src/lib.rs` and
   `sl-client-bevy/src/lib.rs` (parity).
4. **Tests** — keep the lifecycle + `sim_session` round-trip suites green; add a
   focused unit test that the new type round-trips bit-identically to the old
   raw value.
5. **Book** — update any `book/src/content/*.md` that documents the changed
   field.

Expect to fight the usual restriction-lint gotchas already recorded in the
project memory (`indexing_slicing`, `arithmetic_side_effects`,
`must_use_candidate`, `float_cmp`).

## Phases
