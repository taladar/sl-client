---
id: idiomatic-p1-04
title: Physics flags → typed flag newtype (extra_params.rs:364-366 → types/ob
topic: idiomatic
status: done
origin: IDIOMATIC_ROADMAP.md — Phase 1 — Permission & flag bitflags (low invasiveness, high ROI)
---

Context: [context/idiomatic.md](../context/idiomatic.md).

Physics flags → typed flag newtype (`extra_params.rs:364-366` →
`types/object.rs:483-488`), replacing the three reconstructed bools. Added a
public `ReflectionProbeFlags(u8)` bitflags newtype in
`sl-wire/src/reflection_probe_flags.rs` (named `BOX_VOLUME`/`DYNAMIC`/`MIRROR`
consts matching the viewer's `LLReflectionProbeParams::EFlags`, with
`from_bits`/`bits`/`contains`/`is_empty`/`union`/`difference` +
`BitOr`/`BitOrAssign`/`BitAnd`/`Not`, following the
`permissions`/`parcel_flags` pattern). The three
`is_box`/`is_dynamic`/`is_mirror` bools on `ReflectionProbe` collapse to one
`flags: ReflectionProbeFlags` field. Unlike the old 3-bool form, the byte
newtype is byte-identical on round trip even for bits the viewer does not yet
name (the decode/encode now copy the raw `u8` through). Re-exported via
`sl-proto`/`sl-client-tokio`/`sl-client-bevy`; example + lifecycle test query
with `.contains()`. +3 unit tests (named-bit values, raw round-trip incl.
un-named bits, contains/combinators). **Phase 1 COMPLETE.**
