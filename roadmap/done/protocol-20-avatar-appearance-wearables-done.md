---
id: protocol-20
title: Avatar appearance & wearables (done)
topic: protocol
status: done
origin: ROADMAP.md — Tier C
---

Context: [context/protocol.md](../context/protocol.md).

**20. Avatar appearance & wearables (done) ✅ — `AvatarAppearance` (receive),
`AgentSetAppearance`, `AgentWearablesUpdate`/`Request`, `AgentIsNowWearing`,
`AgentCachedTexture`/`Response`, plus the modern server-side-bake CAPS
(`UpdateAvatarAppearance`) · 13 pts.** Decode other avatars' baked-texture IDs +
visual params to render them, and manage the agent's own outfit. **Receive:**
incoming `AvatarAppearance` is surfaced as `Event::AvatarAppearance` (a decoded
`AvatarAppearance` value: avatar id, the per-face **`TextureEntry`** — see
below, the visual-param bytes, the optional appearance-version / COF-version /
flags, hover height, and attachments). The key piece is a faithful port of the
viewer's packed-`TextureEntry` decoder in new `sl-proto/src/appearance.rs`
(`decode_texture_entry`): the run-length
`(default value, then (face-bitmask, value) overrides terminated by a zero
bitmask)`
form for all eleven per-face fields (texture id, tint colour un-inverted from
the wire's `255−x`, scale, offset, rotation, bump/shiny/fullbright, media, glow,
material id), matching `LLPrimitive::parseTEMessage`/`unpack_TEField`. New value
types `TextureEntry`/ `TextureFace`, a `WearableType` enum and `Wearable`, an
`AvatarAppearance`/ `AvatarAttachment`, and an `avatar_texture` module of slot
constants (`HEAD_BAKED`=8 … the 11 baked slots, `COUNT`=45). The agent's own
outfit is surfaced as `Event::AgentWearables` (from `AgentWearablesUpdate`,
pushed at login and on change). **Send:** `Session::request_wearables`
(`AgentWearablesRequest`), `set_wearing` (`AgentIsNowWearing`), `set_appearance`
(`AgentSetAppearance` — the legacy client-side bake), and
`request_cached_textures` (`AgentCachedTexture`, reply
`AgentCachedTextureResponse` → `Event::CachedTextureResponse`).
**Modern Second Life server-side baking ("Sunshine"):** on a baking-capable
region the viewer no longer computes or uploads bakes — it manages the COF in
inventory and POSTs `{cof_version}` to the new `UpdateAvatarAppearance`
capability (`CAP_UPDATE_AVATAR_APPEARANCE`, added to the seed); the grid
composites and broadcasts the result over the same UDP `AvatarAppearance`. The
cap POST is wired through both runtimes (`RequestServerAppearanceUpdate`), with
its `{success, error?, expected?}` reply decoded by `handle_caps_event` into
`Event::ServerAppearanceUpdate`. All wired as `Command`/`SlCommand` variants
(`RequestWearables`, `SetWearing`, `SetAppearance`, `RequestCachedTextures`,
`RequestServerAppearanceUpdate`) through both runtimes. Built on #19 (fetch the
baked textures by id) and #5 (the COF). Covered by `sl-proto` unit tests (the TE
decoder: default fill, face override, empty blob, full round-trip) and four
`lifecycle.rs` tests (`AvatarAppearance` baked-texture + visual-param decode,
`AgentWearablesUpdate` worn list, and the `UpdateAvatarAppearance` reply →
`ServerAppearanceUpdate`). *Live-verified against the local OpenSim via the
`tokio_login_hold_logout` example: one login decoded the avatar's
`AvatarAppearance` (218 visual params, **all 11 baked slots** carrying real
texture ids) and a `RequestWearables` round-trip returned the 6 worn wearables
(Shape/Skin/Hair/ Eyes/Shirt/Pants). The server-side-bake cap is SL-only
(OpenSim's central-bake version is 0, so it uses the legacy path), so
`UpdateAvatarAppearance` is unit-tested only. Test: local OpenSim.*
