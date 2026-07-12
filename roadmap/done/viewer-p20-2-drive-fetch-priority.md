---
id: viewer-p20-2
title: Drive fetch priority
topic: viewer
status: done
origin: VIEWER_ROADMAP.md — Phase 20 — On-screen render priority
---

Context: [context/viewer.md](../context/viewer.md).

**P20.2. Drive fetch priority.** Map pixel area (plus a boost for
the own avatar / attachments / UI, mirroring `LLGLTexture::BOOST_*`) to a
`sl_asset_sched::Priority`; feed it through `TextureStore::request` /
`MeshStore::request` and re-prioritize each (throttled) frame via
`.set_priority()` as the camera moves. The existing `popularity_boost`
already lifts textures shared across many on-screen faces. Reference:
`LLViewerTexture::addTextureStats`, the mesh `LODRequest` priority.
**Done:** a new `Priority::from_pixel_area` in `sl-asset-sched` maps a P20.1
pixel area to a scheduling priority exactly as the reference viewer's texture
decode priority *is* its `mMaxVirtualSize` (clamped/rounded into the `u32`
range, saturating at the full-resolution `2048 * 2048` area — the
reference's `BOOST_HIGH` full-res force — exposed as
`FULL_RESOLUTION_PIXEL_AREA`). The two viewer managers (`TextureManager` /
`MeshManager`) now fetch through `store.request(…, priority).resolved()`
instead of the ungated `store.get`, so every fetch is admitted through the
store's 16-slot priority gate in on-screen order; each keeps its
re-prioritizable request handle and gains a `set_priority`. A new
`render_priority` module's `drive_render_priority` system recomputes, a few
times a second (throttled 0.25 s), the pixel area every visible prim /
sculpt / mesh face covers — keeping the *max* per texture (the reference's
per-texture `mMaxVirtualSize`) — and the pixel area of each mesh object's
still-fetching geometry, then feeds those back through `set_priority`, so
what the camera looks at rises in the queue and what it turns away sinks
(the driver's per-frame value is clamped — in *both* the texture and mesh
managers — to never *demote* a request below its request-time base, so a boost
is never undone by the face pass). Assets the face pass cannot rank from a
scene object's pixel area are instead requested at a fixed boost: terrain
detail textures (`BOOST_TERRAIN`), avatar textures / server bakes /
client-bake layers, and — crucially — a **worn attachment's** face textures
*and* mesh geometry (`BOOST_AVATAR`). An attachment is a skinned / joint-
parented entity whose transform does not reflect its on-screen size, so the
pixel-area pass ranks it too low; the boost (threaded through the geometry
build from `worn_base_priority`, and unconditional for a rigged mesh) is what
loads it with the avatar. Every boost sits in a band *strictly above* the
pixel-area range (which saturates at the full-resolution `2048 * 2048`), so a
boosted asset always outranks even the closest, largest prim rather than
merely tying with it on a dense region. Verified live: OpenSim (terrain,
prims, sculpt, textured avatar all load through the gated path) and aditi (a
~25k-entity region drove the gate genuinely under load — 270+ textures and
440+ meshes queued, hundreds waiting — draining in on-screen order, with the
center avatar's server bakes *and its worn mesh attachments* — jeans, top,
hair — resolving ahead of the surrounding build).
