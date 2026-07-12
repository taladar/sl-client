---
id: idiomatic-p1-02
title: Compressed object-update flags → typed flag newtype (object_update/com
topic: idiomatic
status: done
origin: IDIOMATIC_ROADMAP.md — Phase 1 — Permission & flag bitflags (low invasiveness, high ROI)
---

Context: [context/idiomatic.md](../context/idiomatic.md).

Compressed object-update flags → typed flag newtype
    (`object_update/compressed.rs:10-40`), `.contains()` in place of
    `& MASK != 0`. Added a private `CompressedFlags` newtype (named
    `SCRATCHPAD`…`HAS_PARTICLES_NEW` consts,
    `from_bits`/`bits`/`contains`/`BitOrAssign`, following the
    `parcel_flags`/`permissions` pattern) replacing the eleven module-private
    `COMPRESSED_*` masks; every `& MASK != 0` is now `.contains()` and the
    flags word builds via `|=`. Kept private to the codec (never part of the
    public API); wire bytes are byte-identical (`bits()`/`from_bits` are
    transparent). Unit tests cover the constant values, raw round-trip, and
    `contains`/union.
