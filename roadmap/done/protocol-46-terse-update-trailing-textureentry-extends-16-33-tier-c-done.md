---
id: protocol-46
title: Terse-update trailing TextureEntry (extends #16/#33, Tier C). Done
topic: protocol
status: done
origin: ROADMAP.md — Tier E
---

Context: [context/protocol.md](../context/protocol.md).

**46. Terse-update trailing `TextureEntry` (extends #16/#33, Tier C). ✅ Done.**
`terse_update` (`session.rs`) decoded only the motion blob, and the
`ImprovedTerseObjectUpdate` handler ignored the block's separate `TextureEntry`
field — so a texture/colour change the simulator delivers via a terse update
(when it flags the update `Textures`) was silently dropped, leaving a stale
cached `texture_entry`. Fixed: the handler now extracts that field via a new
`terse_texture_entry` helper and `apply_terse_update` writes it onto the cached
[`Object::texture_entry`] (the raw blob, decodable with
[`decode_texture_entry`], consistent with how the full `ObjectUpdate` surfaces
its own texture entry), emitting the usual [`Event::ObjectUpdated`]; a terse
update with no texture change passes `None` and leaves the cached entry
untouched. The key wire detail (cross-checked against OpenSim's
`CreateImprovedTerseBlock` vs. the full-update `CreatePrimUpdateBlock`):
unlike a full update — whose `TextureEntry` field is the bare blob — the
**terse** field is wrapped as a 2-byte inner length, two zero bytes, then the
`TextureEntry` (the outer 2-byte field length the codec already strips), so the
helper skips the four-byte wrapper to recover the blob. No new command/event
variant and no runtime wiring: `Object` already flows through both runtimes
via `Event::ObjectUpdated`. Covered by a new `sl-proto`
`terse_update_applies_trailing_texture_entry` lifecycle test (a full update
establishes an object with an empty texture entry, then a wrapped terse
`TextureEntry` field round-trips the unwrapped blob into both the event and the
cache). *Unit-/lifecycle-tested only: the reference viewer itself ignores the
terse `TextureEntry` (it reads it only on full updates), and triggering a
texture-flagged terse update needs an in-world scripted object changing a face
rapidly — the same OAR-only constraint as #37 — so the decode is exercised by
the deterministic lifecycle test rather than the local grid. Test: SL grid (the
texture-flagged terse path).*
