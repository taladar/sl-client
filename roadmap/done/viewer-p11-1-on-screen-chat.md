---
id: viewer-p11-1
title: On-screen chat
topic: viewer
status: done
origin: VIEWER_ROADMAP.md — Phase 11 — Chat overlay
---

Context: [context/viewer.md](../context/viewer.md).

**P11.1. On-screen chat.** A `bevy_ui` `Text` node pinned to a corner; on
`ChatReceived` append `"{from_name}: {message}"` (shout / whisper as a prefix
label), keep the last N lines bottom-up. Read-only, no input box. Verify with
chat from the second avatar. **Done.** A new `chat.rs` module owns a
`ChatOverlay` resource (a bounded `VecDeque` of the last `CHAT_HISTORY_LINES`
= 12 formatted lines) and one persistent overlay text node, tagged
`ChatOverlayText`, spawned by a `setup_chat_overlay` startup system anchored
at the bottom-left corner (`PositionType::Absolute`, `left`/`bottom`
inset) so the node grows upward and the newest line sits at the bottom.
`update_chat_overlay` folds every `SlSessionEvent::ChatReceived` message
(`ChatFromSimulator`) into the history and rewrites the node's `Text` only
when a displayable line arrives. Each line is
`"{from_name}: {message}"`, with a `[whisper]` / `[shout]` prefix label for
those two volumes and none for a normal say; the simulator already supplies
the speaker's display name, so (unlike the avatar name tags) no
`UUIDNameRequest` resolution is needed. Typing triggers
(`StartTyping` / `StopTyping`, which actually arrive as
`SlSessionEvent::ChatTyping` rather than `ChatReceived`) and empty-text
messages are filtered so blank lines never accumulate. Viewer-only, no
library change: `ChatMessage`, `ChatType`, and the other chat value types
were already re-exported from `sl-client-bevy`.
Verified live on OpenSim with a second avatar (a `sl-repl-tokio` login of
`avatar2` co-located in the Default Region): the viewer rendered all
three volumes correctly — `avatar2: hello from avatar2`,
`[whisper] avatar2: psst over here`, and
`[shout] avatar2: HELLO EVERYONE` — and the lines persist in the corner
(user-confirmed).

The remaining phases replace the placeholder avatar spheres (Phase 10) with real
avatars: the system-avatar body, server- and client-side baked texturing (incl.
alpha), attachments, rigged mesh with bake-on-mesh, animations, and HUD
attachments. They follow the same top-to-bottom, one-point-per-session cadence.

A new CLI flag `--viewer-assets <dir>` is added in P13.2 and reused by every
avatar / animation phase; absent it, avatars keep the Phase-10 sphere. The
standard Linden `character/` assets (`avatar_skeleton.xml`, `avatar_lad.xml`,
base-body `.llm` meshes, visual-param definitions, the built-in animation
library) are client-side viewer files, not fetched from the grid — the viewer
reads them from that path (point at an installed Firestorm / SL viewer), and the
pure crates stay I/O-free (parse from `&[u8]` / `&str`), mirroring `sl-mesh` /
`sl-texture`. Pure-crate phases verify with `cargo test -p <crate>` using small
committed **fixture** XML / `.llm` / `.anim` files (deterministic-fixture style,
as in `sl-mesh` — not the full LL assets, which stay runtime-loaded); viewer
phases verify with a live run: OpenSim first, then aditi (real SL) for the paths
OpenSim can't exercise (server-side bake, BoM, HUDs).

Key net-new library facts (reused across the phases): `sl-proto` already carries
`AvatarAppearance { texture_entry, visual_params, cof_version, attachments, .. }`
and `PlayingAnimation`, the baked-slot constants
`avatar_texture::{HEAD,UPPER,LOWER,EYES,SKIRT,HAIR,LEFT_ARM,LEFT_LEG,AUX*}_BAKED`
(`COUNT = 45`), `decode_texture_entry`, `WearableType::Alpha`, and the
`AttachmentPoint` enum (HUD points 31–38). `sl-mesh` already decodes rigged-mesh
skin data (`MeshSkin` joint names / inverse-bind / bind-shape / alt-bind /
`pelvis_offset` + per-vertex `VertexWeights`), so rigged mesh needs skinning
*math*, not a new decoder. The BoM magic `IMG_USE_BAKED_*` UUID constants live
only in Firestorm today and are added to `sl-proto` in P17.3.
