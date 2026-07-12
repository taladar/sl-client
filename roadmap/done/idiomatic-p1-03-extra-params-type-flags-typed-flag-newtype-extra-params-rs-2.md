---
id: idiomatic-p1-03
title: Extra-params type flags → typed flag newtype (extra_params.rs:22-40)
topic: idiomatic
status: done
origin: IDIOMATIC_ROADMAP.md — Phase 1 — Permission & flag bitflags (low invasiveness, high ROI)
---

Context: [context/idiomatic.md](../context/idiomatic.md).

Extra-params type flags → typed flag newtype (`extra_params.rs:22-40`).
    Replaced the eight module-private `PARAMS_*` `u16` type-code consts with a
    private `ExtraParamType(u16)` newtype carrying the same named constants
    (`FLEXIBLE` through `REFLECTION_PROBE`) plus a transparent
    `from_code`/`code`. Unlike `CompressedFlags` these codes are mutually
    exclusive (one tag per container entry, not OR-able), so the newtype has
    no `contains`/union: the decoder matches by name
    (`ExtraParamType::SCULPT | ExtraParamType::MESH`) and the encoder writes
    codes by name, dropping the scattered `0x10`/`0x20`/... literals. Kept
    private to the codec (the codes
    only appear inside the raw `ExtraParams` blob); wire bytes are
    byte-identical. A unit test asserts every named code wraps/unwraps to its
    exact wire value.
