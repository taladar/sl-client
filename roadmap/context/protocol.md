# Context — ROADMAP.md

Non-task preamble (including the feature matrix) carried over from `ROADMAP.md`.
Tasks split out of that file carry the `protocol` topic; the matrix rows
correspond to `protocol-NN` task files.

This is a gap analysis of what a *full* Second Life / OpenSim client needs
versus what `sl-client` implements today. Today the workspace does login,
circuit setup, region handshake, keepalive, teleport, logout, and read-only
region/parcel/map *survey* data. Everything below is unimplemented at the
handler level.

**Ordering — incremental standalone value.** Items are ordered so that each one
delivers a client capability *usable on its own* given only that feature plus
everything above it. A feature that is only useful in combination with several
others (notably the world-rendering set: object scene graph, terrain, textures,
avatar appearance, PBR) is ranked by when that *combination* becomes viable, not
by its raw importance. So narrow but self-sufficient clients come first — a
text/chat client, a profile checker, an inventory/product-update bot, an IM or
group bot — and the world-rendering cluster, where value only compounds once its
members coexist, comes later.

All 483 LLUDP messages already exist as generated codec types
(`sl-wire/build.rs`), so most estimates cover
**session state + handler logic + wiring through both runtimes**, not wire
encoding. CAPS-delivered features additionally need new HTTP capability calls
(only `EventQueueGet` is used today).

**Story points** (relative effort, single number for core + both runtimes): 1
trivial · 2 small · 3 small-medium · 5 medium · 8 large · 13 very large · 21
epic. **Test** says whether the local `opensim.service` is enough.

| # | Feature | Pts | Standalone client it unlocks | Test |
|---|---------|-----|------------------------------|------|
| 1 ✅ | Local chat **(done)** | 3 | Text-only chat client / chat bot | Local OpenSim |
| 2 ✅ | Instant messaging **(done)** | 5 | IM bot, notifier, offer-handler | Local OpenSim |
| 3 ✅ | Agent movement & control **(done)** | 5 | Walking/flying/follow bot, autopilot | Local OpenSim (real physics engine) |
| 4 ✅ | Avatar profiles **(done)** | 3 | Profile / picks checker | Local OpenSim (profiles enabled) |
| 5 ✅ | Inventory **(done, UDP + CAPS)** | 8 | Inventory manager, product-update bot | Local OpenSim |
| 6 ✅ | Friends & presence **(done)** | 5 | Presence/online monitor | Local OpenSim (2 accounts) |
| 7 ✅ | Group support **(done)** | 8 | Group chat bot, roster tool | Local OpenSim (Groups V2 module) |
| 8 ✅ | Script dialogs & permissions **(done)** | 3 | Vendor/scripted-object interaction bot | Local OpenSim (scripted object) |
| 9 ✅ | Mute list **(done)** | 2 | Moderation helper | Local OpenSim (MuteList module) |
| 10 ✅ | Seamless teleport (child circuits) **(done)** | 8 | Roaming bot that keeps its session | Local OpenSim (multi-region) |
| 11 ✅ | Money / economy **(done)** | 5 | Balance monitor, tip/vendor bot | **Money module or SL grid** |
| 12 ✅ | Full world map **(done)** | 5 | Live map: agents, POIs, land-for-sale | Local OpenSim |
| 13 ✅ | Parcel management **(done)** | 5 | Land-management tool | Local OpenSim |
| 14 ✅ | Estate/region management **(done)** | 5 | Region admin/restart/ban bot | Local OpenSim (owner account) |
| 15 ✅ | Bandwidth throttle (`AgentThrottle`) | 2 | *(enabler for 16–25)* | Local OpenSim |
| 16 ✅ | Object/scene graph | 13 | Scene auditor, proximity bot | Local OpenSim |
| 17 ✅ | Object interaction & editing | 8 | Builder/rezzer, object mover | Local OpenSim |
| 18 ✅ | Terrain heightmaps (`LayerData`) | 8 | Ground geometry for a renderer | Local OpenSim |
| 19 ✅ | Asset & texture pipeline **(fetch)** | 13 | Asset fetch + textured rendering | Local OpenSim |
| 20 ✅ | Avatar appearance & wearables | 13 | Render avatars; outfit control | Local OpenSim |
| 21 ✅ | Animations | 5 | Dance/gesture bot; animate scene | Local OpenSim |
| 22 ✅ | Sound | 3 | Spatial audio playback | Local OpenSim |
| 23 ✅ | Asset/texture/mesh upload | 5 | Content uploader | Local OpenSim |
| 24 ✅ | Media-on-a-prim / parcel audio | 5 | Media surfaces, streaming audio | Local OpenSim (external stream) |
| 25 ✅ | PBR materials / GLTF | 8 | Modern materials in a renderer | **Recent SL grid; OpenSim varies** |
| 26 ✅ | Voice chat **(signalling)** | 13 | Voice-enabled client | **SL Vivox/WebRTC or FreeSWITCH** |
| 27 ✅ | Experiences | 5 | Experience-permission client | **SL grid only** |
| 28 ✅ | Complete the IM surface | 8 | Offer-handler / IM bot (full) | Local OpenSim (2 accts) |
| 29 ✅ | Profile & pick/classified editing | 5 | Profile editor | Local OpenSim (profiles) |
| 30 ✅ | Inventory mutation & AIS3 | 8 | Inventory manager, product bot | Local OpenSim |
| 31 ✅ | Group management edits | 5 | Group admin bot | Local OpenSim (Groups V2) |
| 32 ✅ | Camera & interest control | 3 | Look-aware roaming bot | Local OpenSim |
| 33 ✅ | World-stream decode & LOD fetch | 5 | *(faithfulness for 16/19)* | Local OpenSim |
| 34 ⛔ | Experience key-value store | 3 | *(out of scope — no client cap)* | **n/a** |

**Every client protocol feature in this roadmap is now implemented (#1–#33).**
The deferred follow-ups #28–#33 — the bits items #1–#27 knowingly left for later
(the "Deferred:" / "follow-up" / "waits on #…" / "unit-tested only" notes in
those entries), promoted to first-class roadmap items so the gap analysis stays
complete — are done. Each was grouped under and extends an earlier item; their
full prose is in **"Planned — deferred follow-ups"** below the done items.

**Item #34 turned out not to be a client protocol feature and is reclassified
out of scope (⛔).** The experience key-value store has **no viewer/client
capability**: the reference viewer (Firestorm) requests all 13 experience caps
(`GetExperienceInfo`, `GetExperiences`, `RegionExperiences`, …) — every one of
which #27 already implements — but no KV cap; the SL wiki's authoritative
"Current Sim Capabilities" list contains no `ExperienceKeyValue` (or any
`KeyValue`) capability; and `llReadKeyValue`/`llCreateKeyValue`/… appear in the
viewer source only as *script-editor keywords*, never as cap calls. The KV store
is reached **only by in-world LSL scripts** running server-side; the simulator
talks to an internal Linden datastore over a service-to-service path never
exposed to clients. There is therefore nothing for a client to wire — it joins
the other out-of-scope items (J2C/glTF/mesh decode, rendering, the voice audio
transport). See the closing "Out of scope" note.

**Decode-fidelity audit (2026-06-18) — #35–#51, Tier E.** A pass over the
inbound decode path (the struct→`Event` translation in `sl-proto`'s
`session.rs`, the hand-written binary-blob decoders, and the CAPS/LLSD decoders)
found *information loss*: fields that are present on the wire and fully decoded,
but then dropped before reaching the library user — usually because the
user-facing type has no field to hold them. These are **not** missing features;
each is a faithfulness gap in an already-shipped item (#1–#33). They are
collected as a new **Tier E** below, ordered by severity, and do not change the
"client protocol surface is complete" claim above — they make the *data* that
surface already carries reach the caller. (Wire encoding is unaffected; the
generated codec already decodes every field — the loss is purely in what
`session.rs` forwards.)

## Tier A — self-sufficient interactive clients (text & bot viewers)

Each works as a complete, useful client on top of today's connection layer.

## Tier intros & trailing notes

### Tier B — extensions of the existing survey/map strengths

These build directly on the read-only data the client already collects, each a
usable standalone tool.

### Tier C — world-rendering cluster (value compounds across the group)

Individually these do little; together they let the bevy crate render and
interact with the actual world. Do them as a set, in this order. **#15 first** —
it is the bandwidth prerequisite for the bulk UDP streams that the rest depend
on.

## Planned — deferred follow-ups of #1–#27

Everything above is done. The items below are the protocol surface that #1–#27
explicitly **deferred** — recorded in their entries as "Deferred:", "follow-up",
"remain roadmap #…", "waits on", or "unit-tested only". They are collected here
rather than interleaved among the done tiers because each is forward-looking
(unbuilt) and extends a *specific* earlier item; the "value compounds as you go
down" ordering of #1–#27 does not apply to them. Out-of-scope large items
(J2C/glTF/mesh *decode*, rendering, the voice audio transport, experience
asset-byte contents) are deliberately **excluded** — see the closing note.

## Tier E — decode-fidelity & information-loss fixes (#35–#51)

Gaps found by the 2026-06-18 decode audit (see the note above the tiers). Each
recovers data the wire already carries and the codec already decodes, but which
`session.rs` drops before the caller sees it. Ordered by severity. Story points
are mostly small because the wire decode exists — the work is adding fields to
the user-facing value/`Event` types and forwarding them (and, for the two
blob items, writing a structured decoder). "Test" notes whether the local
`opensim.service` exercises the path.

| # | Fix | Pts | Recovers | Test |
|---|-----|-----|----------|------|
| 35 ✅ | `ParcelProperties` full field surface | 3 | Parcel name/desc, group-ownership, sale/pass pricing, prim accounting, landing point, access/env flags, `RequestResult` | Local OpenSim |
| 36 ✅ | `ObjectProperties` full field surface | 2 | `ItemID`/`FolderID`/`FromTaskID`, `InventorySerial`, aggregate perms, texture-id list | Local OpenSim |
| 37 ✅ | `TextureAnim` & particle-system decoders | 3 | Structured prim texture-animation and particle (`llParticleSystem`) params | Local OpenSim |
| 38 ✅ | `AvatarSitResponse` complete `SitTransform` | 1 | Sit rotation, sit-camera eye/at offsets, force-mouselook | Local OpenSim |
| 39 ✅ | `RegionInfo`/`RegionHandshake` extended fields | 3 | Region owner, estate-manager flag, water height, terrain limits, 64-bit flags, chat/combat blocks | Local OpenSim |
| 40 ✅ | `AvatarAnimation` physical-event list | 1 | `PhysicalAvatarEventList` (physics/ragdoll) block | Local OpenSim |
| 41 ✅ | Asset-transfer success event + size | 2 | A success event for `TransferInfo` carrying declared `Size` | Local OpenSim |
| 42 ✅ | Group-reply pagination totals | 1 | `RoleCount` / `TotalPairs` so a client knows a set is complete | Local OpenSim (Groups V2) |
| 43 ✅ | `MoneyBalanceReply` transaction id | 1 | `TransactionID` to correlate a balance reply to its pay/buy | Money module or SL |
| 44 ✅ | Inventory push fidelity | 2 | All `UpdateCreateInventoryItem` entries; per-item bulk `CallbackID` | Local OpenSim |
| 45 ✅ | `ChatterBoxInvitation` session type & bucket | 2 | `type` + `binary_bucket` (group/session name, session kind) | SL grid |
| 46 ✅ | Terse-update trailing `TextureEntry` | 2 | Texture/colour change delivered via a terse update | SL grid |
| 47 ✅ | `ParcelAccessListReply` per-entry flags | 1 | The per-entry access-vs-ban `Flags` | Local OpenSim |
| 48 ✅ | Login-response extra fields | 2 | `home`, `look_at`, `agent_access[_max]`, `max-agent-groups`, Library inventory roots | Local OpenSim |
| 49 ✅ | `TeleportFinish` (CAPS) maturity & flags | 1 | Destination `SimAccess` (maturity), `TeleportFlags` (cause) | SL grid |
| 50 ✅ | Minor dropped-field batch | 3 | `TimeDilation`, `AlertInfo`, `MapBlockReply` water height, joint fields, collision plane, `Options.Flags`, NameValue/bump-shiny accessors | Local OpenSim |
| 51 ✅ | Attachment-point `state` un-swizzle helper | 1 | Correct attachment point from the swizzled `state` byte | Local OpenSim |

### Critical — large structural losses

### High — fields the user clearly wants

### Medium

### Low

### Interpretation trap (not loss — but a correctness footgun)

## Tier F — server / simulator role (#52–#65)

| # | Feature | Pts | Inverse of | Test |
|---|---------|-----|-----------|------|
| 52 ✅ | Generic LLSD-XML serializer (`Llsd` → XML) | 2 | `parse_llsd_xml` | Unit round-trip |
| 53 ✅ | Login request parse / response build (`LoginServer`) | 3 | `build_login_request` / `parse_login_response` | Unit round-trip |
| 54 ✅ | `TextureEntry` encoder | 3 | `decode_texture_entry` | Unit round-trip |
| 55 | `ExtraParams` encoder (all subtypes) | 3 | `decode_extra_params` | Unit round-trip |
| 56 | `ParticleSystem` + `TextureAnim` encoders | 3 | `decode_particle_system` / `decode_texture_anim` | Unit round-trip |
| 57 | Object-motion encoders (full / terse / compressed) | 5 | `full_object_motion` / `terse_update` / `compressed_object` | Unit round-trip |
| 58 ✅ | Terrain `LayerData` compressor | 8 | `decode_layer` | Unit (heightmap round-trip) |
| 59 ✅ | CAPS event serializers + `EventQueueGet` response | 5 | the `*_from_llsd` parsers / `build_event_queue_request` | Unit round-trip |
| 60 ✅ | `SimSession` skeleton (sans-I/O simulator session) | 8 | client `Session` | Loopback vs. `Session` |
| 61 ✅ | AIS3 inventory service pairing | 5 | the AIS3 URL/body builders | Unit round-trip |
| 62 ✅ | Experiences service pairing | 3 | `parse_experience_*` | Unit round-trip |
| 63 ✅ | Voice service pairing | 3 | `*::from_llsd` / voice request builders | Unit round-trip |
| 64 ✅ | Materials service pairing | 3 | `parse_render_materials_response` / `parse_gltf_material_override` | Unit round-trip |
| 65 ✅ | Map service pairing (`MapBlockReply` / `MapItemReply`) | 2 | the map request encoders | Loopback vs. `Session` |

Ordered foundation-first; each is one commit with reverse-direction round-trip
tests. "Inverse of" names the existing function/path the new code mirrors
field-for-field — same field order, fixed-point scales, and length-prefix
conventions.

### Foundation

### Login server role

### Simulator role — binary sub-codec encoders

### Simulator role — CAPS event queue & session

### Grid / CAPS service roles

## Out of scope (not LLUDP/CAPS protocol)

Rendering/physics engines, J2C/mesh *display* (vs. decode), the in-viewer UI,
and the Marketplace (web, not protocol) are deliberately excluded — this roadmap
covers protocol features only. **Server roles are in scope as of Tier F
(#52–#65) — the bidirectional codec and the per-role sans-I/O skeletons — but
a *running* grid is not: world authority, persistence, multi-client broadcast,
and the socket/event-loop I/O remain the consumer's job, not these crates'.**
The same
scope as #19/#23/#25/#26/#27 holds for the follow-ups above: J2C/JPEG-2000 pixel
*encode/decode*, the glTF 2.0 document decode, the mesh model-import
(LOD/physics/cost) pipeline, the voice audio media transport (SIP/RTP/WebRTC),
and an experience's event/asset *byte contents* remain out — those are large
items an existing crate would own, not protocol wiring. The **experience
key-value store (#34)** is out for a different reason: it has no client
capability at all (the viewer never accesses it — see #34's entry), so there is
no client wire protocol to own.
