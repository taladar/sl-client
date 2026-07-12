---
id: idiomatic-p6-03
title: LindenBalance (new signed-money type)
topic: idiomatic
status: done
origin: IDIOMATIC_ROADMAP.md — Phase 6 — Adopt `sl-types` non-key value types (low-medium)
---

Context: [context/idiomatic.md](../context/idiomatic.md).

**`LindenBalance` (new signed-money type)** — for the legitimately signed
    fields: group `balance`/`amount` and transaction deltas.
    **Kept client-local in `sl-proto` (NOT `sl-types`)** per the user (this
    session's standing rule: new types go local first, batch-migrated to
    `sl-types` later to avoid version churn) — same precedent as
    `LandArea`/the union keys, overriding the roadmap's original "add to
    `sl-types`" note. New public `LindenBalance` in
    `sl-proto/src/types/money.rs`: shape
    `{ negative: bool, magnitude: LindenAmount }` with **private** fields and
    a normalising `new` (zero is canonically non-negative → no negative-zero,
    so derived `Eq`/`Hash` stay consistent with the manual sign-aware
    `Ord`/`PartialOrd`). Arithmetic composes balances and amounts by type:
    `Add`/`Sub<LindenAmount>` and `Add`/`Sub<LindenBalance>` (+ the four
    assign variants), `Neg`, `From<LindenAmount>`, and
    `TryFrom<LindenBalance> for LindenAmount` (errors with a new
    `NegativeBalanceError` when negative). Wire codec is pure inherent methods
    (`from_i32`/`to_i32`/`from_i64`/ `to_i64`; decode is total, encode
    fallible on `i32` overflow) so the type migrates to `sl-types` cleanly
    without dragging `sl_wire::WireError`; the thin `WireError`-wrapping
    boundary helper `linden_balance_to_wire` lives in `sl-proto/src/types.rs`
    next to `land_area_to_wire`. Typed the three signed L$ fields —
    `GroupAccountSummary.balance`, `GroupAccountDetailsEntry.amount`,
    `GroupAccountTransaction.amount` — wrapping at the codec boundary only
    (decode `LindenBalance::from_i32`, encode `linden_balance_to_wire`) so the
    wire i32 is byte-identical. LEFT RAW (deliberately, NOT signed L$):
    the `MoneyTransaction` wire-block amount (the typed
    `MoneyTransaction.amount` is already `LindenAmount`; only the raw
    wire-block integer stays raw, like every wire field) and
    `ResourceAmount.amount` (script memory/url count, not money). Re-exported
    `LindenBalance`+`NegativeBalanceError` through
    `sl-proto`/`sl-client-tokio`/`sl-client-bevy` (parity). REPL/survey only
    label these events (no field access) → no downstream change. Book
    `content/economy.md` updated (replaced the "awaiting a `LindenBalance`
    type" note with the realised description). +6 focused unit tests (i32 wire
    round-trip incl. `i32::MIN`/`MAX`, negative-zero normalisation, sign-aware
    ordering, by-type arithmetic, `LindenAmount` interconvert,
    out-of-`i32`-range encode → `None`); lifecycle + `sim_session` round-trip
    suites updated. Build+clippy(`--workspace --all-targets`, 0 warnings)+all
    tests+`cargo doc -D warnings`+mdbook green. NO `sl-types` touched.
    **Follow-up (same session, user-spotted `LindenAmount`-sweep MISS):**
    `EventInfo.amount` was raw `u32` but is documented as the event cover
    charge in L$ (wire `Amount` is `U32`, non-negative). Typed it
    `Option<LindenAmount>` gated on the companion `cover` flag (user picked
    the `Option`-gating shape, mirroring the `sale_price` precedent): `Some`
    iff `cover != 0`, `None` otherwise, `None` ⇒ the `0` no-cover wire
    sentinel. New `pub(crate)` boundary helpers
    `linden_cover_from_wire(cover, amount)` (total — `U32` is always in range)
    / `linden_cover_to_wire(field, amount)` (rejects an amount above the `u32`
    wire range) in `types.rs`; wire bytes byte-identical. Book
    `content/search.md` updated; +1 unit test
    (`linden_cover_gates_on_cover_flag`); lifecycle + `sim_session` suites
    updated. No downstream change (REPL/survey only label `EventInfoReply`).
