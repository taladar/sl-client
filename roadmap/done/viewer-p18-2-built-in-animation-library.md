---
id: viewer-p18-2
title: Built-in animation library
topic: viewer
status: done
origin: VIEWER_ROADMAP.md — Phase 18 — Animations (full pipeline)
---

Context: [context/viewer.md](../context/viewer.md).

**P18.2. Built-in animation library.** Resolve an `anim_id` to its asset:
built-in fixed-UUID motions from the `--viewer-assets` path, else fetch an
uploaded `.anim` over `ViewerAsset` (reuse the asset fetch path). Cache
decoded motions. **Done:** a new `sl_anim::registry` module (named for its
role, like `decode`, to dodge `module_name_repetitions`) ports the reference
viewer's 140 `ANIM_AGENT_*` built-in UUIDs
(`llcharacter/llanimationstates.cpp`), each tagged `BuiltinKind::Keyframe` (a
downloadable `.anim` asset) or `Procedural` (the 48 walk/stand/turn/`LLEmote`/
always-on-adjuster motions the reference viewer synthesises in C++ and never
fetches — taken from `llvoavatar.cpp`'s `registerMotion` block), with a
`builtin_animation(uuid)` lookup and six unit tests. The viewer's new
`animations.rs` owns an `AnimationManager` resource driving the same
`ViewerAsset` generic-asset store the P15.2 wearable fetch uses:
`request(id)` skips a nil/cached/in-flight/known-unavailable id, records a
procedural built-in as unavailable *without* a fetch (fetching its UUID would
404), and otherwise resolves the `.anim` bytes — first from a `<uuid>.anim`
file under `--viewer-assets` (a pre-provisioned built-in; stock viewers ship
none, so this is the escape hatch and downloadable built-ins arrive over
`ViewerAsset` like uploads), else over `ViewerAsset` — decoding to a `Motion`
off the render thread on the `IoTaskPool` and caching it by UUID (shared
across every avatar playing it). `ingest_avatar_animations` requests a motion
for every animation each `AvatarAnimation` lists; `poll_animations` folds a
finished decode into the cache and announces `AnimationDecoded`. The
`motion()` accessor + the event carry the P18.3 seam (`#[expect(dead_code)]`
until then). Verified live on OpenSim with the real skeleton loaded (Firestorm
`character/` dir via `SL_VIEWER_ASSETS`): the agent's own `stand` is ingested,
resolved against the registry, and correctly classified procedural / not
fetched. The download+decode branch was not triggered live — an idle OpenSim
avatar only ever signals the procedural `stand` — but it is covered by
`sl-anim`'s decode unit tests and reuses the P15.2 `ViewerAsset` fetch path
already proven on OpenSim. No visible avatar motion yet: posing the skeleton
from the cached motions is P18.3.
