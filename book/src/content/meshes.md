# Meshes & the LLMesh Pipeline

Second Life mesh geometry is a **custom binary format** — *not* glTF. (glTF in
SL is the per-face PBR [material](materials.md) layer, which shades a mesh's
faces; it is a separate concern from the geometry described here.) A mesh asset
is fetched over the `GetMesh2` (or legacy `GetMesh`)
[capability](../comms/caps.md); at the lowest level a client issues
`Command::FetchMesh` and receives the raw asset bytes as `Event::AssetReceived`.

The **`sl-mesh`** crate adds a higher-level pipeline on top of that — the mesh
counterpart of [`sl-texture`](textures.md). It fetches, **decodes** the LLMesh
format, and **caches** meshes in memory and on disk, so a mesh is never fetched
or decoded twice while it is still referenced. It decodes geometry, skin
(rigging), and physics; mesh **upload** (the viewer build path) is out of scope.

## The LLMesh format

A mesh asset is a **binary-LLSD header** map followed by concatenated
**zlib-compressed blocks**:

- A small header, fetched with an initial HTTP `Range` probe of the first
  `4096` bytes. It is a binary LLSD map (parsed with the same
  [LLSD](../comms/llsd.md) codec as CAPS bodies), optionally preceded by a
  legacy `"<? LLSD/Binary ?>"` prefix. Its length — `header_size` — is exactly
  the number of bytes the LLSD parse consumes.
- Each block is named in the header by a `{ offset, size }` sub-map, where
  `offset` is measured **from the end of the header**. So a block's absolute
  byte range in the asset is `[header_size + offset, header_size + offset +
  size)`, fetched with a second `Range` request and zlib-inflated to a further
  binary-LLSD value.

The blocks are:

- **Four geometry levels of detail** — `lowest_lod`, `low_lod`, `medium_lod`,
  `high_lod` — each an array of per-face **submeshes**. A submesh carries
  `u16`-quantized positions, normals, and UV0 coordinates plus a `u16` triangle
  index list, dequantized to `f32`: a position is
  `min + (u16 / 65535) * (max - min)` over the submesh's position domain, a
  normal is `(u16 / 65535) * 2 - 1`, and a UV over its texture domain. (The
  per-axis `NormalizedScale` is kept as metadata, not baked into positions,
  matching the viewer's core unpack.)
- A **`skin`** block — the rig: joint names, per-joint inverse-bind matrices,
  the bind-shape matrix, and (in each geometry submesh's `Weights`) up to four
  `(joint, weight)` influences per vertex.
- **`physics_convex`** (a convex-hull decomposition) and **`physics_mesh`** (a
  triangle collision mesh, decoded like a geometry block).

Multi-byte quantized values in the block payloads are little-endian. The decode
mirrors Firestorm's `LLVolume::unpackVolumeFacesInternal` and
`LLModel`/`LLModel::Decomposition::fromLLSD`.

## Level of detail

Unlike a texture's progressive [discard level](textures.md#level-of-detail)
(where a *prefix* of one codestream is a coarser image), a mesh's four levels
are **discrete, independently stored blocks**. The `MeshLod` newtype (in
`sl-proto`, beside `DiscardLevel`) names them `Lowest`/`Low`/`Medium`/`High`
and orders so a *higher* level is *finer* — the opposite sense to `DiscardLevel`
— so "at least as fine as" is `>=`. There is no progressive reuse and no
in-place downsample between levels: switching level fetches and decodes a
*different* block.

## The store

`MeshStore::get(id, target).await` returns an `Arc<MeshEntry>` decoded to
**at least** the requested level, fetching and decoding only what is missing. As
with the texture store, entries are held only by **weak** reference, so a mesh
becomes collectible as soon as the last external `Arc` drops (a periodic
`sweep()` prunes dead entries), and requests for the same mesh share work: a
per-entry lock makes fetch and decode **single-flight**, and concurrent requests
at different levels **coalesce to the finest** one wanted, so a single decode
satisfies every requester. Decoding runs on a `rayon` pool off the caller's
thread.

The level flow differs from textures. `get` first ensures the header is fetched
and parsed (from the disk cache when present, else the network), then fetches
the target level's block byte range, zlib-inflates it, and decodes it — serving
the nearest available level when the exact one is absent. `set_lod` switches
level by fetching and decoding that level's block (there is no downsample). Skin
and physics are fetched once via their own blocks (`get_skin` / `get_physics`)
and are level-independent. Progress is observable as `MeshProgress` (`Queued`,
`ReadingDisk` vs `Downloading`, `Decoding`, `Ready(level)`, `Failed`,
`Cancelled`), and `request(id, target, priority)` returns a cancellable,
re-prioritizable `MeshRequest` handle.

## The shared scheduler

The store's priority gate, popularity boost, CPU-pool bridge, and network
`AssetFetcher<K>` trait are **not** mesh-specific: they live in the small
**`sl-asset-sched`** crate and are shared with the texture store. Everything
domain-specific — the LLMesh decode, the disk cache, the LOD flow, the entry and
its decoded representation — stays concrete in `sl-mesh`. A texture-wanted-by-
many boost and a mesh-wanted-by-many boost use the same
`floor(log2(count))`-scaled term.

## The on-disk cache

The disk cache is **Firestorm-compatible**, in its own dedicated directory: one
`<hex>/<uuid>.mesh` file per mesh, holding the viewer's 12-byte preamble
(`version`, `header_size`, and a `flags` bitmask of which blocks the file
contains) followed by the contiguous asset region — the header and each fetched
block written at its absolute header offset, with any gaps zero-padded. The
store slices blocks out of that region with the same offset arithmetic it uses
for the network, so a mesh present on disk is never re-fetched. Files are purged
least-recently-written down to a fraction of a byte or entry-count budget.

## Using it from the tokio client

`sl-client-tokio` provides `ReqwestMeshFetcher`, a network backend over async
`reqwest` that fetches `GetMesh2` / `GetMesh` byte ranges and whose capability
URL the run loop refreshes on each region change (preferring `GetMesh2`). Build
a `MeshStore` over it (with an optional on-disk cache directory) and drive it
with `get` / `request`. The store surface is re-exported from `sl-client-tokio`
(`MeshStore`, `MeshRequest`, `MeshProgress`, `MeshLod`, `MeshError`,
`DecodedMesh`, `Submesh`, `MeshSkin`, `MeshPhysics`, ...).

The `mesh-fetch-http` [conformance case](../conformance/overview.md) exercises
this end to end against a live grid: resolve a mesh id (from a fixture, or by
scanning the region's object stream for a mesh-shaped prim), fetch and decode it
through the store, confirm a second request is served from cache, and switch its
level of detail. On the real beta grid it decodes a multi-thousand-triangle
mesh; the local OpenSim grid, which streams no mesh in its default region,
records the case `partial`.

## Bevy

`sl-client-bevy` (on Bevy 0.19, via the `bevy_mesh` feature) bridges the store
to Bevy's renderer: `to_bevy_mesh` turns one decoded `Submesh` into a
`bevy::mesh::Mesh` (a `TriangleList` with position — and, when present, normal
and UV0 — attributes plus `u32` indices), and `to_bevy_meshes` yields one Bevy
mesh per face (skipping empty faces), preserving face order so the app can pair
each with its per-face material. `BevyMeshFetcher` is a blocking-HTTP fetcher
for a Bevy app with no async runtime.

Rigged-mesh skinning (feeding a `SkinnedMesh`) and pairing each face to its
material are left to the app: the decoded types expose the skin and per-vertex
weights, and the *object* side already carries the `face → material_id` /
`texture_id` pointers (`sl_proto::SculptData` names the mesh id, and the
render-material / texture-face types name each face's material and texture).

---

The decode, cache, store, and LOD flow live in the `sl-mesh` crate; the shared
scheduler in `sl-asset-sched`; the `MeshLod` newtype in `sl-proto`; the tokio
and Bevy network backends and re-exports in `sl-client-tokio` /
`sl-client-bevy`.

See also [Textures & the Asset Pipeline](textures.md) (the texture counterpart,
which shares the scheduler) and [Materials](materials.md) (the glTF material
layer that shades a mesh's faces).
