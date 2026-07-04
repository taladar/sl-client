# sl-avatar

Pure Second Life / OpenSim **avatar** decoding: the standard Linden system
avatar — skeleton, legacy base-body meshes, and the visual-param / morph-target
/ skeletal-scale / driver system — plus the generic matrix-palette skinning math
shared by the base body and rigged mesh.

Like its siblings `sl-mesh` (LLMesh) and `sl-texture` (J2C) the crate is
deliberately **Bevy-free and I/O-free**: it parses from bytes / strings
(`&[u8]` / `&str`) and produces geometry in Second Life's right-handed **Z-up**
space. The standard Linden `character/` assets (`avatar_skeleton.xml`,
`avatar_lad.xml`, base-body `.llm` meshes) are client-side viewer files the
*caller* reads from an installed viewer and hands to this crate as bytes — this
crate never opens a file or fetches from the grid. The Bevy skeleton-instance /
`SkinnedMesh` conversion lives in `sl-client-bevy`, at the entity boundary.

The pieces (added over the course of the viewer road map) are:

- skeleton parse (`avatar_skeleton.xml`) → joint hierarchy, rest transforms,
  collision volumes, and the attachment-point / HUD-point maps
  (`avatar_lad.xml`).
- base-mesh `.llm` decode → per-part positions / normals / UVs / skin weights +
  morph-target deltas, distinct from `sl-mesh`'s `LLMesh`.
- the `avatar_lad.xml` visual-param table (morph / skeleton / driver params) and
  the byte→value dequantization of an `AvatarAppearance.visual_params`.
- matrix-palette skinning math shared by the base body and rigged
  (`sl_mesh::MeshSkin`) mesh.

This crate holds the pure decoding + math only; the viewer's rigged-body
rendering, morphing, and animation driving live in `sl-client-bevy` and the
viewer binary.
