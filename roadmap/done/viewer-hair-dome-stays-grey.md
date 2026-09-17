---
id: viewer-hair-dome-stays-grey
title: The default hair dome intermittently stays grey for a whole session
topic: viewer
status: done
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

## Investigation (2026-09-16/17)

Restarting until it recurs was not an option: the user reports the avatar has
rendered correctly in the last few dozen runs. Two aditi runs with the logging
above were healthy and are recorded here only for what they ruled out.

- **The grace-period warning is not a symptom.** It fired in a healthy run too:
  an `AgentWearables` lands while the worn assets are still being fetched, the
  change is deferred, and the refetch times out. Drop it from the evidence.
- **The own avatar's hair bake** (`d4a376e5…` in these runs) is a flat colour
  (220, 183, 146) with alpha 0 at every discard level, classifies
  `Transparent`, and hides the region. A bake drawn unmasked would look tan,
  not grey, so candidate 1 in its "wrong alpha" form does not fit. The user
  describes the failure as "the system hair default look": the region was
  never hidden, so the hair slot had no bake in `baked_textures` (or it never
  decoded).
- **The two `jpeg2k` errors are the store's normal path.** OpenJPEG runs in
  strict mode (the `jpeg2k` crate's `strict-mode` feature is off, so
  OpenJPEG's own default of strict applies). A per-level byte estimate that
  cuts a tile part fails with exactly that message, and `upgrade` then grows
  to the full codestream and decodes again.
- **The primary avatar has no bake-on-mesh faces** (no
  `SL_VIEWER_LOG_AVATAR_FACES` tally in a whole session), so the grey BoM
  placeholder is not it either.

What the reference does and this viewer did not, both in
`LLVOAvatar::processAvatarAppearance` / `applyParsedAppearanceMessage`:

1. **An appearance with at most one visual param is discarded whole**, bakes
   included ("no reliable basis for knowing appearance"). Firestorm adds a
   self-only branch, "Empty appearance for self. Forcing a refresh", which
   exists because Second Life does send one for the agent's own avatar. Ours
   applied it: an empty texture entry replaced the avatar's bakes with none.
   A mesh-body wearer's head, upper and lower regions stay hidden by the worn
   meshes' `IMG_USE_BAKED_*` sentinels regardless, so the one region left on
   the untextured default body is the hair (a mesh-hair wearer wears no hair
   sentinel), and it stays so until the next appearance, which on a settled
   outfit may never come. That is the symptom exactly: intermittent,
   own-avatar, only the dome, for the whole session.
2. **A slot the new appearance leaves undefined keeps the last defined bake**
   (`isTextureDefined`: not null, not `IMG_DEFAULT_AVATAR`, not `IMG_DEFAULT`),
   for every region but the skirt and the universal slots. Ours replaced the
   whole set, so an appearance sent while a region is still being baked also
   dropped that region's good bake.

## Fix

Both, in `ingest_avatar_bakes` / `apply_avatar_appearance`
(`AvatarAppearance::has_visual_params`, `avatar_texture::is_bake_defined`,
`bake_slot_ids`), with a `warn` when our own avatar gets a param-less
appearance, so the next occurrence names itself in the log.

Also Firestorm's forced refresh for the self case
(`LLAppearanceMgr::syncCofVersionAndRefresh`): the `IncrementCOFVersion`
capability end to end (command, event, both runtimes, the fake grid's handler
bumping its Current Outfit Folder), and `drive_server_bake` running the
sequence when our own avatar gets a param-less appearance after a real one —
the `AvatarRezSelfBakeForceUpdateNotification` tip, the increment retried after
`2^n - 1` s up to five times, then a bake request at the answered version (or,
after giving up, at the known one). Ignoring the message keeps our own view
right; the refresh is what repairs what the grid shows everyone else.

**Reproduced end to end on the fake grid**
(`full_stack_test::an_appearance_that_says_nothing_keeps_the_bakes`): the real
viewer logs in, the catalogue NPC arrives wearing its blue bakes, and the grid
sends it first an `AvatarAppearance` with no visual params and an empty
texture entry, then one with params and every baked slot at
`IMG_DEFAULT_AVATAR`. With the fix disabled the NPC's chest goes from blue to
0 % blue after the first message; with only the slot carry-over disabled, the
same after the second. With the fix both keep it blue. And
`full_stack_test::an_empty_own_appearance_asks_the_grid_to_rebake` sends our
own avatar a param-less appearance: the viewer's increment reply names a
version and the grid then receives a bake request at exactly that version
(with the refresh disabled the test times out waiting for the increment).

What stays unproven is only that *Second Life* sent such a message in the
2026-09-08 session (nobody can make it send one on demand). The reference
code and Firestorm's own self-avatar fix say it does, and the `warn` names it
the next time it happens.
