---
id: viewer-avatar-state-dump-replay
title: Capture/replay a live avatar's full render state for offline reproduction
topic: viewer
status: done
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

**In progress — full render replay** (the target). Implemented, pending a live
capture+replay verification run.

Design: **capture the raw session events, replay the raw session events.** The
wire `Object` / `AvatarAppearance` / `PlayingAnimation` types are all
`serde`-round-tripping, so the capture retains and serialises them verbatim and
replay re-emits them as synthetic `SlEvent`s — the normal render systems then
*derive* bakes, invisible regions, attachments and pose exactly as a live login
would (so a rendering fix is tested against the same inputs).

- **Bundle v2** (`replay_bundle.rs`): `<agent>.json` = the avatar object + its
  whole attachment tree (verbatim `Object`s, so transforms / `TextureEntry` /
  `ExtraParams`+light / mesh id / reflection-probe blocks are all carried), the
  decoded `AvatarAppearance` (visual params + baked-texture entry), and the
  playing-animation set. Plus a shared `cache/<kind>/<first-char>/<uuid>.<ext>`
  drop-in cache: meshes / animations / PBR material assets are copied verbatim
  out of the live caches; **textures are fetched at full resolution** (see
  below).
- **Capture** (`avatar_dump.rs`): a `ReplayCaptureStore` folds the object /
  appearance / animation events every frame (only when `SL_VIEWER_DUMP_DIR` is
  set — zero cost otherwise); **Ctrl+Alt+D** writes one manifest per nearby
  avatar + bundles its referenced assets. No display names are stored.
- **Texture full-fetch** (the crux of getting textures on replay): the local
  cache usually holds only the low-LOD prefix the viewer happened to load, so a
  copy would be an incomplete codestream the offline store then fails to grow.
  Instead the capture fetches each referenced texture at full resolution from
  the live caps (regular textures from `GetTexture`, baked body textures from
  the appearance service), on a worker thread the dump **joins** before
  returning (a detached fetch dies when the operator closes the viewer). The
  viewer pauses a few seconds; wait for the `capture complete` log.
- **Offline session** (`sl-client-bevy`): `SlClientPlugin.offline` registers the
  full event/resource substrate but skips login — the session is fed only by the
  injector.
- **Replay** (`avatar_replay.rs` + `--replay <dir>`): points the asset stores at
  the bundle `cache/` (a `paths` cache-root override), injects a synthetic
  `SlCapabilities` (opens the four cap-gated managers so they serve from the
  bundle), sets the world origin from the primary avatar's region, and re-emits
  the captured events once. Every injected avatar renders through the faithful
  "other avatar" path. Composes with the existing debug-camera / screenshot
  harness; frames the primary avatar by default.
- **Test rig** (per request): `--replay-orbit-light` (a local light orbiting the
  avatar, sweeps specular highlights) and `--replay-reflection-probe` (a local
  reflection probe on the avatar, feeds IBL). The global reflection probe is
  active regardless.
- The headless geometry-slice analyzer (`example avatar_replay`) was ported to
  the v2 bundle (typed wire-type deserialisation + the drop-in caches), so it
  still reports the face-bone diagnostic.

Materials are bundled too: **PBR** (`AT_MATERIAL`) render-material assets are
copied out of `materialcache` and decoded to also bundle their base-colour /
metallic-roughness / normal / emissive maps; **legacy** `LLMaterial`s (which
have no disk cache — they arrive over the `RenderMaterials` cap) are captured
resolved from the live session into the manifest and re-emitted as a synthetic
`RenderMaterials` event on replay, with their normal / specular maps bundled.

**Verified end-to-end on aditi** (2026-08-06): captured 5 avatars, replayed
offline — bodies + mesh attachments + bakes + materials + animation render
faithfully, as they appeared live.

Two general `sl-texture` store bugs surfaced and were fixed along the way (they
also help the live viewer): a cached texture whose grow-fetch fails now decodes
at its available resolution instead of being **dropped**, and a complete
(J2C-EOC-terminated) codestream is recognised as done so it is not re-fetched.

Minor follow-ups (not blocking):

- A small texture **collection gap** — a handful of referenced textures (≈8/150
  on the test capture) are not enumerated by the fetch plan and so render
  missing; find the reference path that misses them.
- **Reflection-probe rig is best-effort** — it spawns a local
  `ObjectReflectionProbe`; if the capture pool needs more to bind it, the global
  probe still provides IBL.
- A **separate**, pre-existing texture bug (not part of this work): a texture
  can stay stuck at a low-res prefix and never upgrade to a finer level that
  exists — see the `viewer-texture-stuck-low-lod` bug item.
