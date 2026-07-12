---
id: viewer-p17-3
title: Bake-on-mesh
topic: viewer
status: done
origin: VIEWER_ROADMAP.md — Phase 17 — Rigged mesh & bake-on-mesh
---

Context: [context/viewer.md](../context/viewer.md).

**P17.3. Bake-on-mesh.** A worn rigged (BoM) body face whose
`TextureEntry` slot is an `IMG_USE_BAKED_*` sentinel is textured from the
wearer's own baked avatar texture rather than fetched. **Shape:** a
`BomFace` marker (agent + baked slot) tags such faces in
`build_rigged_submeshes` (spawned with the opaque body-skin placeholder,
never the sentinel — the P17.2 invisible-shell finding);
`apply_bom_face_materials` then mirrors each face onto its wearer's
matching base-region material every frame, so it follows whichever bake
resolved that region (server bake on SL, client composite on OpenSim) and
its alpha, updating in place as the bake decodes. The `IMG_USE_BAKED_*`
constants already existed from P16's region-hide.
**Three cross-cutting fixes were needed to render a real SL mesh body:**
(1) **P17.2 binding bug** — a mesh body is worn as a multi-prim *linkset*
whose rigged parts parent to the linkset **root prim**, not the avatar, so
the old `body_root(tracked.parent)` never resolved (146k "skeleton not
ready" retries → invisible body); `apply_rigged_attachments` now chases
the parent chain to the wearer (`AvatarState::wearer_of` →
`avatar_root_of`). (2) **Server-bake fetch** — a SL server ("Sunshine")
bake is *not* fetchable by UUID from the `GetTexture`/`ViewerAsset` CDN
(it 503s); it lives on a separate **appearance service** whose base URL
arrives in the `agent_appearance_service` login field. Added: parse it in
`sl-wire` `LoginSuccess` → expose on `Session` → deliver as
`SlIdentity::agent_appearance_service`; a typed `sl-texture`
`TextureFetchType` (full, mirrors the reference `FTType`) narrowed to a
remote-only `RemoteTextureSource` via `TryFrom` (local-generated kinds —
media-on-a-prim, local files — error at that boundary before the store)
threaded through `TextureStore::get`/`request` and both runtime fetchers,
which pick the CDN (by UUID) or the bake's URL
(`<svc>texture/<avatar>/<slot>/<uuid>`); the bake is stored/decoded in the
normal store keyed by its UUID. (3) **5-component J2C** — a server bake is
a 5-component codestream (`R, G, B, bump, clothing`), which `jpeg2k`'s
`get_pixels` rejects; `decode_j2c` reads the diffuse RGB from the first
three components (opaque alpha, dropping bump/clothing), matching the
reference `decodeChannels(.., 0, 4)`. Also fixed the **mesh UV V-flip**
(SL mesh UVs are OpenGL bottom-up, Bevy samples top-down) so clothing and
the BoM body map correctly instead of near-uniform, and set a
**0.02 m camera near plane**. Verified live on aditi: a BoM mesh body
binds, deforms, and shows the wearer's server-baked skin +
correctly-mapped clothing. Remaining avatar-fidelity bugs this surfaced
(skinning distortion, rigid eyes/teeth, prim params) are collected under
**Known rendering issues** below.
