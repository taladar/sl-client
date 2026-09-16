# sl-sculpt

Pure **sculpt-texture** tessellation for Second Life / OpenSim clients: a
decoded RGB sculpt map is read as a displacement grid and stitched into
geometry. It is the sculpt counterpart of `sl-prim` (parametric prims) and
`sl-mesh` (LLMesh), and reuses `sl-prim`'s `PrimMesh` / `PrimFace` output type.

Like its siblings the crate is **Bevy-free and I/O-free**, producing geometry
in Second Life's right-handed **Z-up** space; the `to_bevy_prim_mesh`
conversion lives in `sl-client-bevy`.

A sculpt map's pixel `(r, g, b) / 255 - 0.5` becomes a grid vertex — the texel
below the vertex's fraction of the map, unfiltered — and the grid is laid over
the prim's **own path and profile**, which `sl-prim` generates at the sizes the
map asks for. The shape decides the faces and the texture coordinates; the map
decides only where the vertices are. The sculpt type pinches the sphere's poles
and wraps the cylinder's, sphere's and torus's seams, honouring the mirror /
invert flags. A map without positions gives an empty surface and one whose area
is implausible a sphere placeholder, both as the reference does.

The grid is sized by `mesh_resolution` from the map's dimensions and the
requested `sl_prim::PrimLod`, matching the reference's
`sculpt_calc_mesh_resolution`: the level of detail caps the vertex budget, the
map caps it again (a vertex per four pixels), and what is left is split between
the axes in the map's own aspect ratio. So a distant sculpt is not tessellated
at full rez, and a small map is not resampled past what it carries.

The tessellation follows Firestorm's `LLVolume::sculpt` /
`sculptGenerateMapVertices`, reimplemented idiomatically rather than copied.

## Usage

`tessellate(map, sculpt_type, shape, lod)` (or
`tessellate_with(map, params, shape, lod)` when the `sculpt_type` byte is
already parsed) takes an `sl_texture::DecodedImage` and the prim's dequantized
`sl_prim::PrimShape` and returns an `sl_prim::PrimMesh` with one face per face
the shape names — one for the usual circle-on-circle sculpt shape. A seam is
two vertices at one position carrying texture coordinates `0` and `1`, as in
the reference. The caller sources the decoded map from the shared `sl-texture`
`TextureStore` — this crate never fetches or decodes.
