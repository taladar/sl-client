---
id: viewer-avatar-state-dump-replay
title: Capture/replay a live avatar's full render state for offline reproduction
topic: viewer
status: in-progress
origin: proposed while diagnosing viewer-avatar-tongue-protrudes on aditi (2026-08-05)
refs: [viewer-avatar-tongue-protrudes, viewer-mesh-hair-not-rendering,
  viewer-facelight-too-bright]
---

Context: [context/viewer.md](../context/viewer.md).

Live avatar-render bugs (protruding tongue, brow spikes, missing hair, blown-out
facelights) are hard to reproduce because the offending avatar can log out and
the exact combination of shape + attachments + animations is gone. The **heavy
assets already persist** (`meshcache/`, `texturecache/`, `animcache/`), so what
is missing is the small per-avatar record of *how it is assembled and posed*.

Add a debug **dump** of a picked avatar's complete render input and a **replay**
loader that rebuilds it with no grid:

Dump (pick an avatar → key, or a REPL/console command) serialises:

- the `AvatarAppearance` visual-param bytes (→ skeletal / morph deformation),
- the attachment list: each worn object's mesh + texture UUIDs, local
  position / rotation / scale, attachment point, and any joint-position
  overrides (the rig's alternate-bind matrices),
- the resolved `SkeletalDeformations` / `JointOverrides` actually applied,
- the set of currently-playing animation ids (so a face-bone posing bug repro
  reproduces the pose, not just the rest shape).

Replay: a headless entry (fits the existing debug-camera / screenshot harness)
loads a dump file and renders/CPU-skins the
avatar from the on-disk caches alone. This makes any buggy avatar reproducible
after logout and turns each into a committed regression test.

**Constraints — captured bundles are strictly local, ephemeral, and NEVER
committed or shared.** They contain other residents' actual mesh/texture assets,
which carry creator permissions (no-transfer, etc.); redistributing them (e.g.
in git) would violate content permissions and Linden Lab's DRM/ToS, as well as
privacy (real names/appearance). The tool must have no in-repo default output
path and a `.gitignore` guard so an accidental in-repo dump can never be staged.
A committed **regression-test fixture must be synthetic** (hand-authored
geometry / params) — never derived from a real captured avatar.

## Progress

**Done — geometry slice** (a first, working consumer). Capture
(`sl-client-bevy-viewer/src/avatar_dump.rs`, opt-in on `SL_VIEWER_DUMP_DIR`,
**Ctrl+Alt+D**) writes `<agent>.json` (avatar name + key, appearance bytes,
animation ids, worn rigged-mesh ids) and bundles the referenced mesh + animation
bytes. A headless analyzer
(`cargo run -p sl-client-bevy --example avatar_replay`) rebuilds the skeleton +
shape deform + overrides + animation pose and reports each mouth/brow bone's
distance from `mHead` — verified reproducing the tongue tucked at d≈0.098. Keep
this as the geometry-diagnosis path.

**In progress — full render replay** (the target). Extend the capture to the
avatar's **whole attachment tree** (every worn object's transform,
`TextureEntry`, `ExtraParams`/light data, mesh id), its **baked textures** +
invisible regions, and bundle **all** meshes / textures / anims in the
**cache's own on-disk layout** (`<first-char>/<uuid>.ext`) so the bundle is a
drop-in cache. Add a viewer `--replay <dir>` mode that skips login, points the
asset fetchers at the bundle, injects the captured appearance / objects /
animations into `AvatarState` / `ObjectState` / `AnimationPlayback`, and lets
the normal render systems draw the avatar — textures, BoM alpha-hiding,
facelights, and mesh body parts all visible. This is what lets render-only bugs
(facelight brightness, missing hair, brow spike) be reproduced offline after the
avatar logs out.
