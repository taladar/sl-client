---
id: viewer-hair-dome-stays-grey
title: The default hair dome intermittently stays grey for a whole session
topic: viewer
status: bugs
origin: aditi run while verifying viewer-quick-prefs-environment-presets (2026-09-08)
refs: [viewer-avatar-mesh-hair-and-hairbase-both-render]
---

Context: [context/viewer.md](../context/viewer.md).

On aditi (`server_bake_grid=true`), the base avatar's **hair mesh** — the dome
on top of the head — rendered untextured **grey** and stayed that way for the
whole session. On a correctly baked avatar it should be textured by the HAIR
bake, and for a mesh-hair wearer that bake is transparent, so the dome should
disappear.

**Intermittent, and that is the most useful thing known about it.** The same
avatar on the same grid bakes correctly in other runs; it failed in this one and
then stayed failed for the whole session. So this is not a wrong constant or a
missing case — it is a **race that does not recover**: something needed arrived
late or not at all, and nothing re-ran once it did.

Observed by the user during an unrelated verification run; not a regression from
the environment work in that run, which touches neither the avatar nor the
texture path.

## What the run's log holds

Not much, because the run was logged at `warn` for the avatar and texture
modules. Two things:

```text
WARN sl_viewer_world_avatar::bake_inputs: assembling own bake inputs after grace
     period (4 asset(s) [Skin, Hair, Eyes, Shirt], 0 texture(s) still pending;
     server_bake_grid=true)
ERROR jpeg2k::codec: "Tile part length size inconsistent with stream length"   x2
```

The grace-period warning is **probably not the cause**: its own reasoning is
that on a server-bake grid those wearable assets feed shape parameters only (the
body is textured by the server bake), and it singles out Shape/Skin as the pair
that skews proportions — Shape was not among the four.

The two JPEG2000 decode failures are unattributed at that log level. They could
be the bake layers or ordinary prim textures.

## Two candidates, and how to tell them apart

Both have to explain *intermittent, and permanent once it happens*.

1. **The HAIR bake texture failed to decode.** One of the two `jpeg2k` errors.
   Intermittent by nature, and permanent if a bake whose decode failed is never
   re-fetched — worth checking whether the bake path has the retry/give-up
   ladder the ordinary texture path has (the same log shows a prim texture
   retrying six times and giving up, so the two paths differ).
2. **The HAIR slot still carried `IMG_DEFAULT_AVATAR` when the appearance was
   processed.** `visible_body_bakes` omits a slot whose id fails
   `is_bake_visible` (nil, `IMG_DEFAULT_AVATAR`, `IMG_INVISIBLE`), while
   `invisible_body_slots` hides a region only for `IMG_INVISIBLE`. A slot
   carrying **`IMG_DEFAULT_AVATAR`** — "not baked yet" — is therefore neither
   textured nor hidden, which renders exactly as an untextured grey region.
   That is a *timing* state, not a permanent one: it becomes permanent only if
   the later `AvatarAppearance` carrying the real bake is not processed, or is
   processed without re-draping. This run's grace-period warning says the run
   was late fetching wearables generally, which is the kind of run a
   bake-not-ready-yet race would surface in.

No new code is needed to tell them apart. `avatars.rs` already logs every slot
it asks for:

```text
debug!("requesting server bake slot {slot} ({slot_name}) = {id}")
```

Re-run against aditi with

```sh
RUST_LOG='warn,sl_viewer_world_avatar=debug,sl_texture=debug' \
  BEVY_ASSET_ROOT="${PWD}/sl-client-bevy-viewer" \
  ./target/release/sl-client-bevy-viewer \
  --credentials credentials.aditi.toml --grid aditi --avatar primary \
  >hair-bake.log 2>&1
```

and read whether the HAIR slot appears at all. Absent ⇒ candidate 2, and the id
it carried says which sentinel. Present ⇒ candidate 1, and the texture log
attributes the decode failure.

Because it is intermittent the run may have to be repeated, so log to a file and
keep the ones that reproduce. Note also whether a **second** `AvatarAppearance`
for the own avatar arrives later in a bad run and what the hair slot holds in
it — that is the difference between "the grid never published the bake" and "we
did not act on it when it did".

## The working case is already written down

[[viewer-avatar-mesh-hair-and-hairbase-both-render]] investigated a *different*
hair defect and, on the way, recorded what a correct hair region looks like on
this code: "the `hair`-region base mesh (`avatar_hair.llm`) is hidden because
the hair bake slot is `IMG_INVISIBLE`". So the dome being visible at all says
the slot was **not** `IMG_INVISIBLE` — which narrows this to the two candidates
above and rules out the hiding path itself being broken.

Live-only: mesh bodies and real bakes need aditi (see the aditi live-testing
memory), and the R22 avatar-render notes are explicit that headless screenshots
are too noisy for avatar-render defects — the idle animation moves limbs
between frames.
