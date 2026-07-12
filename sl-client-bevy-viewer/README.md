# sl-client-bevy-viewer

A minimum-viable **Bevy visual viewer** on top of the `sl-client` stack. It
logs in via the shared `credentials.toml` mechanism (`sl-repl::auth`) and
renders a region — terrain, prims (full Linden path/profile tessellation via
`sl-prim`), meshes (`sl-mesh`), and sculpt-texture prims (`sl-sculpt`) — with
diffuse textures (no advanced materials), sphere placeholders for avatars, an
on-screen chat overlay, a debug fly-camera, and a single quit key.

The viewer is a thin rendering application over `sl-client-bevy`'s
`SlClientPlugin`: it consumes only `SlEvent` / `SlCommand` (never touching
`Session` accessors directly) and builds its own ECS scene mirror from the
event stream. Geometry arrives in Second Life's right-handed **Z-up** space; a
single `sl_to_bevy` conversion is applied at the entity `Transform` / camera
boundary to Bevy's **Y-up**.

See the `viewer` topic in the workspace `roadmap/` tree for the staged plan
(`roadmap/context/viewer.md` plus the `viewer-*` task files). This is a Phase 0
scaffold: the windowed app lands in later phases.

## Non-goals

Advanced materials (PBR / normal / specular / bump / glow), avatar meshes /
rigging / baked textures / animation (spheres only), flexi / particles /
lights, water, sky / atmosphere, distance-based LOD switching, object
selection, chat input, and sound.
