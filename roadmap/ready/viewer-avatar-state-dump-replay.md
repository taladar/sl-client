---
id: viewer-avatar-state-dump-replay
title: Capture/replay a live avatar's full render state for offline reproduction
topic: viewer
status: ready
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

Constraints: dump files carry a real avatar's appearance, so write them to the
scratchpad / a gitignored path, never committed; only a **scrubbed** fixture
(no avatar name or agent id — keyed by shape/mesh data only, per the repo's
no-avatar-names rule) becomes a test asset.
