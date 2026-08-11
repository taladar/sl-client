---
id: viewer-grey-avatars-bakes-not-showing
title: Most other avatars render grey (baked skin/textures not showing) on aditi
topic: viewer
status: done
origin: noticed live on aditi while extending F3 / async fetch (2026-08-11)
---

Context: [context/viewer.md](../context/viewer.md).

**RESOLVED (2026-08-11):** the primary cause was **`AvatarAppearance` never
dispatched on child-agent (neighbour-region) circuits** — `dispatch_child`
(`sl-proto` `methods.rs`) handled the object stream, animations, coarse
locations and circuit management but
**fell `AvatarAppearance` through to the "unhandled" arm**, so a
neighbour-region avatar's body spawned (from the object stream) yet no bake was
ever ingested → grey. Fixed by adding the `AvatarAppearance` arm to
`dispatch_child` (mirroring the root handler), plus the four **sound** messages
(`SoundTrigger` / `AttachedSound` / `AttachedSoundGainChange` / `PreloadSound`)
that were dropped on child circuits for the same "children carry limited
traffic" reason (neighbour spatial audio). `AvatarAnimation` / `ObjectAnimation`
were already wired; `KillObject` is covered by the object path. Regression
guard: `child_circuit_avatar_appearance_is_dispatched` in
`sl-proto/tests/lifecycle.rs`. Live-verified on aditi: neighbour-region avatars
now texture. Not a perf-branch regression — a never-wired child-circuit gap (R24
wired coarse dots / objects / animations for neighbours, but not appearance).

Residual minor cases (external / not this bug): a genuinely-unbaked same-region
bot recorded 0 visible bakes (`is_bake_visible` correctly skips its default
slots), and a transient aditi bake-CDN 503 on some *uncached* bakes.

---

Original investigation notes follow.

Live on aditi: nearly every **other** avatar renders **grey** (flat skin, no
baked textures) while the **own** avatar textures correctly. The user recalls
avatars were "much more complete before the performance branch merge", so a
perf-branch regression is suspected — and it is likely **more than one bug**.

## What is confirmed (not the cause)

- `agent_appearance_service` **does** parse (aditi returns
  `http://bake-texture.glb.aditi.lindenlab.com/`) — logged by the new sl-wire
  probe. `ingest_avatar_bakes` correctly takes the **server-bake** branch
  (`avatars.rs`), not the by-UUID CDN fallback.
- The bake service **works**: fetching the own avatar's head bake URL by hand
  returns `HTTP 200` (1.5 MB J2C). Own-avatar bakes load (and disk-cache).

## Distinct sub-causes seen

0. **PRIMARY (most grey avatars): `AvatarAppearance` never processed.** A live
   `avatars=debug` run with 5 other avatars present:
   **4 of them spawned a body** (`spawned avatar for <id>`) and had their name
   resolved, yet **never produced an `appearance for <id>` line** — i.e.
   `ingest_avatar_bakes` never ran for them, so no bakes were recorded, fetched,
   or applied → grey. Only 2 avatars (own + one other) ever got an appearance
   line. So the dominant grey cause is
   **the appearance never reaching `ingest_avatar_bakes`**, not a fetch/apply
   failure. Prime hypothesis: they are **neighbour-region avatars** whose
   `AvatarAppearance` (child-agent circuit) is not dispatched into an
   `Event::AvatarAppearance` — the same class as the R24 neighbour coarse-dot
   fix, which handled coarse dots but not appearance/bakes. Confirm with
   `RUST_LOG=sl_proto=debug,sl_client_bevy=debug`: is the `AvatarAppearance` UDP
   message even received for these ids, and are they on a child circuit? Check
   the multi-region handover commits (`3b53feec`, `6f470d26`) for a regression
   vs. a never-wired gap. **Not fixable by Tex Refresh** (no bakes recorded).
1. **Some grey avatars record ZERO visible bakes** (the one *other* avatar that
   did get an appearance). Its `AvatarAppearance` re-processed ~44× each time
   `requested 0 baked texture(s)` — so `visible_body_bakes` returned empty. Why
   0? (genuinely unbaked/cloud/bot, a texture-entry parse gap, or
   `is_bake_visible` wrongly rejecting.) The **repeated re-processing** is
   suspicious — the COF-version gate should suppress it, so the appearance
   likely carries no `cof_version`. (Own avatar parsed 10 bakes through the
   *same* code, so the parser works — this is per-avatar data.)
2. **Bake fetched OK but never applied (hypothesis).** User: "maybe an
   optimization prevents the *application* of the bake and it fetches okay."
   Application is `assign_avatar_bake_materials` / `apply_avatar_bake_textures`;
   an equality-guard / budget / debounce there could drop the drape.
3. **CDN 503 for other avatars' bakes.** 30 fetches failed
   `HTTP 503 Service Unavailable - DNS failure` (Akamai origin-resolution) with
   retries exhausted — an aditi/CDN-side origin failure for *those* assets,
   while own-avatar bakes succeed. Partly external, but our retry gives up
   permanently.

## Regression suspects (perf branch, 2a6484f5 area)

- `2a6484f5 "budget + debounce avatar appearance application"` — debounces the
  shape/morph re-apply; check whether it also starves / drops the bake-material
  application path (sub-cause 2).
- Also the T-pose symptom (other avatars stuck in T-pose, idle anim not driving
  the skeleton) — suspect `41609694 "pose gate — skip settled avatar/animesh
  skeleton evaluation"`. Filed here as a related but separate bug to split out.

## Levers already landed

- Manual **Tex Refresh** pie action (self + other) re-issues + evicts
  (`TextureManager::forget`) an avatar's bakes for another try — helps sub-cause
  3 when the CDN recovers, but is a no-op for sub-cause 1 (0 recorded bakes).
- Diagnostics: sl-wire `agent_appearance_service` log; per-slot server-bake vs
  by-UUID debug in `ingest_avatar_bakes`.

## Next

Reproduce with `RUST_LOG=sl_client_bevy_viewer::avatars=debug` on a crowd,
correlate a specific grey avatar to sub-cause 1/2/3, and bisect the perf branch
(`2a6484f5`) for sub-cause 2.

(The by-UUID fallback is **correct** on OpenSim after all — other avatars' own
viewers client-bake *and upload* the result, which we then fetch by UUID; only
SL uses the appearance service. So no fallback change is needed.)
