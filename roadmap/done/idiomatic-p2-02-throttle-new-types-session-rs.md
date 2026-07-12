---
id: idiomatic-p2-02
title: Throttle::new (types/session.rs):
topic: idiomatic
status: done
origin: IDIOMATIC_ROADMAP.md — Phase 2 — Constructor invariants (low invasiveness, caller-facing)
---

Context: [context/idiomatic.md](../context/idiomatic.md).

`Throttle::new` (`types/session.rs`): seven positional `f32`s in a
    fixed wire order — easy to transpose. Did the maximal version of every
    option. New public `Kilobits(f32)` newtype (validating `new` →
    `Result<_, ThrottleError>` rejecting NaN/infinite/negative, `const
    new_unchecked` codec-boundary ctor, `ZERO`, `get`). The seven `Throttle`
    fields are now **private** `Kilobits` with `resend()`…`asset()` accessors,
    so a negative/NaN bandwidth can't be set post-construction. The old `new`
    became validating (`Result<_, ThrottleError>`, mirroring the `Camera`
    pattern); a new `const new_unchecked` (used by the presets and the
    `from_bits_per_second` wire-decode) reconstructs verbatim. Added a
    `ThrottleBuilder` (named per-category setters taking already-validated
    `Kilobits`, infallible `const build`) reachable via `Throttle::builder` —
    this is what fixes the transposition hazard. New `ThrottleError`
    (`NotFinite`/`Negative`, `thiserror`). All re-exported through
    `sl-proto`/`sl-client-tokio`/`sl-client-bevy`. REPL `build_throttle`
    (`sl-repl/src/registry.rs`) uses validating `Throttle::new`, mapping a
    `ThrottleError` to `ReplError::InvalidArg`. Wire bytes byte-identical
    (`bits_per_second`/`from_bits_per_second` unchanged in value). +5 unit
    tests (accessor layout, builder == positional `new`, bps round-trip,
    `Kilobits::new` rejects NaN/inf/negative, `new` rejects a bad category).
