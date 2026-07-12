---
id: viewer-p18-1
title: Scaffold sl-anim + .anim decode
topic: viewer
status: done
origin: VIEWER_ROADMAP.md — Phase 18 — Animations (full pipeline)
---

Context: [context/viewer.md](../context/viewer.md).

**P18.1. Scaffold `sl-anim` + `.anim` decode.** New pure crate (scaffold
like P12.1). Decode the Linden keyframe-motion binary → `Motion`
with per-joint rotation/position keyframe tracks, priority, ease-in/out, loop
points, and constraints. Fixture-based tests. `cargo test -p sl-anim`.
**Done:** the decoder lives in `decode.rs` (named for its role and to avoid
the `module_name_repetitions` lint on `motion::Motion`, mirroring
`sl-mesh`/`sl-texture`'s `decode` module) and is re-exported at the crate
root. `Motion::from_bytes(&[u8])` decodes the whole file: the header
(`base_priority`, `duration`, `emote_name`, loop points, ease-in/out,
`hand_pose`), the per-joint tracks, and the collision-volume `Constraint`s,
applying the reference viewer's range/finiteness validations (bad priority,
over-long/`NaN` duration, too many joints, negative key counts, out-of-range
key time, unknown constraint type/over-long chain → a typed `AnimDecodeError`;
a corrupt constraint *count* is skipped, not fatal, matching the reference).
Quantised values are widened exactly like the C++ (`U16_to_F32` with its
near-zero snap; rotations completed to a unit quaternion via
`unpackFromVector3`). **Both** wire versions decode: the modern `1.0`
(`u16`-quantised) form and the legacy `0.1` form (`f32` times, `f32` Euler
angles built with a `mayaQ`/`ZYX` port, `f32` positions clamped to `[-5, 5]`)
— the latter still backs many decades-old SL animation assets that visual
updates never replace. Priorities/hand poses are newtypes (`JointPriority` /
`HandPose`) with named constants; constraint kind/target are enums. A
forward-only `Cursor` reads little-endian primitives via `f32::from_bits` /
byte-fold shifts / `u32::cast_signed` (the crate lints forbid `from_le_bytes`,
`as`, indexing, `unwrap`/`expect`/`panic`). Two committed binary fixtures
(`tests/fixtures/minimal.anim` v1.0, `minimal_old.anim` v0.1) drive eight
round-trip + error-path tests.
