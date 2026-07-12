---
id: protocol-37
title: TextureAnim & particle-system structured decoders (extends #16/#33, Tier
topic: protocol
status: done
origin: ROADMAP.md — Tier E
---

Context: [context/protocol.md](../context/protocol.md).

**37. `TextureAnim` & particle-system structured decoders (extends #16/#33, Tier
C). ✅ Done.** `Object::texture_anim` and `Object::particle_system` were
retained only as raw `Vec<u8>` with no decoder in the crate. Added a new
`sl-proto/src/particles.rs` with two faithful ports of the viewer's parsers, and
two new value types on `Object` alongside the (kept) raw blobs:
**`Object::texture_animation: Option<TextureAnimation>`** — the 16-byte
`TextureAnim` / `LLTextureAnim::unpackTAMessage` block (`mode` bit field, `face`
as `i8` with `-1` = all faces, the `size_x`/`size_y` frame grid, and
`start`/`length`/`rate` `f32`s), with a `texture_anim_mode` constants module
(`ON`/`LOOP`/`REVERSE`/`PING_PONG`/`SMOOTH`/`ROTATE`/`SCALE`) and the viewer's
non-`SMOOTH` "floor the grid at 1" behaviour. **`Object::particles:
Option<ParticleSystem>`** — the `PSBlock` / `LLPartSysData::unpackBlock`,
handling **both** wire forms: the legacy fixed 86-byte block (`unpackLegacy`)
and the modern size-prefixed form (`unpack` → `LLPartData::unpack`) with the
optional
trailing glow / blend-func fields gated by the `LL_PART_DATA_GLOW` /
`LL_PART_DATA_BLEND` particle flags. Recovered the full source surface (CRC,
flags, `pattern` — with a `particle_pattern` constants module —
inner/outer angle, burst rate/radius/speed-min/max/part-count, source max/start
age, angular velocity, particle acceleration, particle-texture id, target id)
**and** the per-particle template (flags, max age, start/end colour, start/end
scale, start/end glow, source/dest blend funcs). The viewer's `unpackFixed`
fixed-point reads are ported as small unsigned-`u8`/`u16` and signed-`u16`
helpers. Both decoders run at every site that fills the raw blobs — the full
`ObjectUpdate` and both the legacy and "new" particle paths of the compressed
update. The two value types, the two `decode_*` functions, and the two constants
modules are re-exported through both runtimes. Covered by five `sl-proto` unit
tests (texture-anim decode + wrong-size/grid-floor; particle legacy form,
modern-with-glow/blend form, and empty/bad-size rejection) and a `lifecycle.rs`
end-to-end test (a full `ObjectUpdate` carrying both blobs → the decoded
`Object::texture_animation` and `Object::particles`). *Unit- and
lifecycle-tested only: a live exercise needs an in-world scripted object running
`llSetTextureAnim`/`llParticleSystem` (no headless rez path — it must arrive via
an OAR or a viewer), the same constraint that left #16's compressed/terse
decoders unit-tested. The decoders are deterministic ports of
`lltextureanim.cpp`/`llpartdata.cpp`. Test: local OpenSim (rez a scripted object
running `llSetTextureAnim`/`llParticleSystem`).*
