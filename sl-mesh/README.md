# sl-mesh

A higher-level mesh fetch API and cache for Second Life / OpenSim clients, the
mesh counterpart of `sl-texture`. It fetches SL mesh assets over the
`GetMesh2` / `GetMesh` capability, decodes the custom **LLMesh** format, and
keeps the results in a level-of-detail-aware store.

SL mesh geometry is a **custom binary format, not glTF** (glTF in SL is the
per-face PBR *material* layer, out of scope here). A mesh asset is a binary
LLSD **header** followed by concatenated **zlib-compressed blocks**: four
discrete geometry levels of detail (`sl_proto::MeshLod`:
`Lowest`/`Low`/`Medium`/`High`), an optional `skin` block (joints + bind
matrices + per-vertex weights), and optional `physics_convex` / `physics_mesh`
blocks.

- **LLMesh decode** — parse the header, byte-range-fetch a level's block,
  zlib-inflate it, and dequantize the `u16` positions / normals / UVs and `u16`
  triangle indices into `f32` geometry (plus skin and physics decode). Runs off
  the calling thread on the shared `sl-asset-sched` rayon bridge.
- **Weak-reference store** — the store holds only `Weak<MeshEntry>`, so a mesh
  is collectible as soon as the last external `Arc` drops.
- **LOD as separate blocks** — unlike a texture (progressive discard), each mesh
  level of detail is an *independent* block: switching level fetches and decodes
  that block (there is no in-place downsample). One decode at the finest-wanted
  level satisfies every concurrent requester.
- **Never fetch or decode twice while referenced** — single-flight requests
  share one fetch and one decode; an in-memory + on-disk cache avoids repeat
  network fetches.
- **Priority scheduler** — the shared `sl-asset-sched` priority gate and
  popularity boost (non-blocking, observable, cancellable, re-prioritizable
  requests).
- **Firestorm-compatible on-disk cache** — per-UUID `.mesh` files with the
  viewer's 12-byte preamble (`version`, `header_size`, cached-block `flags`) in
  a dedicated cache directory.

Mesh **upload** (the viewer build path) and rigged-mesh skinning integration
are out of scope; the decode exposes skin/weights and physics for an app to use.
