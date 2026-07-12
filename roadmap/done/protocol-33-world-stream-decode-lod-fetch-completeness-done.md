---
id: protocol-33
title: World-stream decode & LOD-fetch completeness (done) —
topic: protocol
status: done
origin: ROADMAP.md — Tier C
---

Context: [context/protocol.md](../context/protocol.md).

**33. World-stream decode & LOD-fetch completeness (done) ✅ —
`ObjectUpdateCompressed` trailing fields, HTTP `Range` LOD · 5 pts. (extends #16
& #19, Tier C.)** Two faithfulness gaps the rendering tier left raw, now closed.
**Full `ObjectUpdateCompressed` decode:** Item #16 decoded only the compressed
update's reliable fixed prefix (identity/motion/flags/text/media-url) and left
the trailing length-prefix-less fields raw, noting that walking past the legacy
particle block was "not possible from the stream alone." Cross-checking the
reference viewer's `LLViewerObject`/`LLVOVolume::processUpdateMessage` against
OpenSim's `CreateCompressedUpdateBlock` established the exact packing order and
the two fixed block sizes that make the walk possible: the legacy particle block
is a fixed `PS_LEGACY_DATA_BLOCK_SIZE` (86 bytes) and the path+profile shape is
a fixed 23 bytes, both prefix-less. `compressed_object` now decodes,
best-effort, the full tail in order — the generic `Data` field (tree genome /
linkset prim count, captured from the tree/scratchpad slot), the legacy particle
system (raw bytes), `ExtraParams` (measured by a new
`extra_params::extra_params_len` walker, then decoded via #25's
`decode_extra_params` and stored raw, exactly as a full update), attached sound
(id/gain/flags/radius), name-values, the path/profile shape (decoded into a new
`PrimShapeParams` value), the packed texture entry (its little-endian `u32`
length then the raw `TextureEntry` bytes, ready for #20's
`decode_texture_entry`), the texture-animation block (raw bytes), and the
trailing "new" particle system (raw bytes).
**The full-update decoder was completed to match:** `Object` gained
`shape: PrimShapeParams` (decoded path/profile params), `texture_anim`,
`particle_system` and `data` raw blobs, and `object_from_full_update` now
populates them from the full `ObjectUpdate` block's individual shape fields and
`TextureAnim`/`PSBlock`/`Data` fields it previously dropped — so a compressed
update yields the *same fully-populated* `Object` as a full one (shape, particle
system, texture-animation, generic data, sound, name-value, texture entry,
decoded `extra`/raw `extra_params`, text colour). The only full-block fields
still dropped are the deprecated Linden physical-joint fields (`JointType`,
`JointPivot`, `JointAxisOrAnchor`) — the reference viewer's
`processUpdateMessage` reads none of them and OpenSim's encoder `AddZeros` them,
so they carry no data. The prefix still decodes even when a malformed tail runs
short (the trailing decode short-circuits, leaving the already-decoded fields in
place). Added a non-consuming `Reader::peek_rest` to `sl-wire` to measure the
embedded `ExtraParams` container before consuming it. **HTTP range/LOD fetch:**
Item #19 fetched a texture's whole J2C codestream over the `GetTexture` cap then
truncated it client-side for a discard level; this replaces that with real HTTP
`Range` requests so only the LOD prefix crosses the wire. For a non-zero discard
the runtimes issue a small `Range: bytes=0-599` probe (`j2c::FIRST_PACKET_SIZE`,
now public) to read the J2C `SIZ`/`COD` header, compute the prefix byte length
via the existing `j2c::discard_data_size`, then fetch exactly that prefix with a
second `Range` request when the probe did not already cover it; a server that
ignores `Range` (replying `200` with the whole image) still yields the correct
prefix, just without the saving. `FetchMesh` and `FetchAsset` gained an optional
inclusive `byte_range: Option<(u32, u32)>` that issues a
`Range: bytes=start-end` request against `GetMesh2`/`GetMesh`/`GetAsset` (e.g. a
single mesh LOD whose offsets the caller read from the mesh header). All wired
through both runtimes (the tokio async and bevy blocking fetch paths). Covered
by two new `sl-proto` tests (the full compressed-tail decode of text, media,
particle, extra-params, sound, name-values, shape, texture-entry and texanim
into an `Object`; and the `extra_params_len` walker incl. its truncation clamp),
plus a shape assertion added to the existing full-`ObjectUpdate` test, on top of

## 19's existing j2c header/discard-size tests. *Live-verified against the local

OpenSim via the `asset_fetch` tokio example: the standard plywood texture
(`8955…`, a 512×512 J2C) fetched as the full 79 234 bytes at discard 0 and as a
1 536-byte prefix at discard 3 (= 64×64×3/8, the codestream truncated three LOD
levels via `Range`), on one login with a clean lifecycle. The compressed decode
is unit-tested only — stock OpenSim sends full, not compressed, `ObjectUpdate`s
(as #16 noted) — and is the SL-grid path; the mesh/asset `byte_range` rounds out
the `Range` surface for those caps. Test: local OpenSim.*
