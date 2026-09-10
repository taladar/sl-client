---
id: viewer-environment-personal-lighting
title: Personal lighting — local environment override
topic: viewer
status: in-progress
origin: Vintage-parity coverage audit (2026-07-22)
blocked_by: [viewer-ui-widget-scaffold]
refs: [viewer-phototools, viewer-environment-fixed-editor]
---

Context: [context/viewer.md](../context/viewer.md).

The "Personal Lighting" floater: override the environment **locally** —
region settings untouched, nothing published — with immediate sliders for
sun/moon position, sun colour, ambient, haze, cloud coverage and the other
high-traffic sky knobs, plus a water section; a reset returns to the region
environment. This is the local-override layer the P22 environment pipeline
needs anyway (a settings source that shadows the region's EEP values), and
[[viewer-phototools]] explicitly builds its environment half on it.

Scope: the override layer in `environment.rs` (region ⊂ parcel ⊂ local
precedence, matching EEP semantics), the floater with live-updating
sliders, apply-a-preset (built-in Linden day frames already ported in
`render_scene.rs`; inventory settings assets arrive with
[[viewer-environment-fixed-editor]]), and reset.

Reference (Firestorm, read-only): `llfloaterenvironmentadjust`,
`floater_adjust_environment.xml`, `llenvironment` (ENV_LOCAL layer).

Builds on: the P22 EEP ingest + sky/water renderers.

**First slice landed (2026-07-23, user request):** the **World ▸
Environment** menu pins a fixed sky — Sunrise / Midday / Sunset / Midnight
over the four ported Linden presets (now shared in `sky_presets.rs`) — and
"Use Shared Environment" restores the grid settings.
`EnvironmentState` grew the local-override layer skeleton this task needs:
it keeps the `shared` (grid) environment beside the rendered `settings`,
re-applies the pin across region changes and late EEP replies, and exposes
`set_fixed_sky` / `fixed_sky` (the menu check marks). The floater's slider
surface and the water section remain this task's scope.

## Parity-audit addendum (2026-08-19)

Parity-audit extensions — override lifecycle knobs: a manual
transition time when applying a personal environment
(`FSEnvironmentManualTransitionTime`), persist the personal environment
across logout/login (`EnvironmentPersistAcrossLogin`), and the
repeated-keybind behaviour where pressing the same environment toggle
again reverts to the shared/region environment
(`FSRepeatedEnvTogglesShared`).

## Done

A new crate, `sl-viewer-environment`, is the home for the environment editors;
`personal_lighting` is its first window. Four columns — the colour swatches and
the two image pickers with **Reset**, the atmosphere sliders, where the sun and
moon sit, and the water — over the sky and water captured when the window opens.
Every control writes `EnvironmentState::set_local_instant`, so the sky, water,
terrain and fog drivers pick the edit up on the next frame with no
editor-specific path through the renderer.

**The knobs are three tables, not forty fields.** `SkyKnob` / `WaterKnob` /
`ColorKnob` / `TextureKnob` each carry the Fluent key, the range and the
read/write pair, and the spawn, the write-back and the reseed all walk the same
rows — so a knob cannot exist in one of the three and not the others. The
reference's own scalings are in them: ambient and sun at a third, the two blues
at a half, glow size as `2 − r/20` and glow focus as `b/−5`.

**The sun and moon needed an inverse that did not exist.** `SkySettings` stores
a rotation, and a slider needs the angle back:
`sl_proto::rotation_to_azimuth_altitude` is the counterpart to the existing
`azimuth_altitude_to_rotation`, with a round-trip test across the `atan2` branch
cut and both poles.

**The three lifecycle knobs of the parity addendum are in**, as
`EnvironmentManualTransitionTime`, `EnvironmentPersistAcrossLogin` and
`EnvironmentRepeatedTogglesShared` under a new `[environment]` settings section:

- *The transition* is a real cross-fade. `EnvironmentState` keeps the settings
  that were on screen and blends toward the new ones, and every renderer now
  samples through `sky_at` / `water_at` rather than reaching into `.settings` —
  which is the difference between "the settings in force" and "what this frame
  should draw", and only one of those was a field. Only *manual* changes fade; a
  grid reply is not something the user did. A live editor drag never fades
  (`set_local_instant`), or a fade restarted each pixel would smear the preview
  behind the hand.
- *Persistence* is one hidden account-scoped setting holding the pin and the
  three local tracks as JSON, restored once after the account scope loads. "Use
  Shared Environment" clears it, so a personal environment the user dropped does
  not come back at the next login.
- *Repeated toggles* fold into the menu's one `set_fixed` choke point: picking
  what is already pinned un-pins it. Off by default, as in the reference.

The World ▸ Environment menu gained **Personal Lighting…**, gated on the same
`@setenv` restriction the presets are.

**A sweep found a latent hole while blessing.** The floater-chrome check that
throws every window past the corner and asks whether it is still grabbable threw
it a fixed 400 px — measured from the *middle* of its title bar, so a window
wider than 800 landed short of the clamp and passed the grabbable half without
the clamp ever having run. Nothing was over 760 wide until this window. The
throw now clears the window's own size.

**The parcel layer is rendered, so the precedence is the whole
region ⊂ parcel ⊂ local.** Parcel replies used to be logged and dropped. Now the
agent's parcel is mirrored from `SlAgentParcel` (id *and*
`parcel_environment_version`, so an edit under a standing agent re-requests, as
the reference does), a parcel-scoped `RequestEnvironment` goes out on the
region's retry clock, and the reply lands in its own layer above the region and
below the local one.

The two questions that made this look unanswerable both have answers in the
sources:

- *What does a parcel with no override send?* OpenSim's
  `ViewerEnvironment.DefaultToOSD` — an `is_default` map with **no** `day_cycle`
  — for every parcel that has not set one, which is most of a region. Our
  decoder turns an absent `day_cycle` into an empty one, so the reference's two
  clearing arms (`!mDayCycle`, then `isTrackEmpty(TRACK_WATER)` /
  `isTrackEmpty(TRACK_GROUND_LEVEL)`) collapse into one predicate here: no water
  track or no ground-level sky track means *no override*, and the region's sky
  stands.
- *What does the parcel layer actually replace?* The day cycle and its length
  and offset, and nothing else — `setEnvironment(ENV_PARCEL, dayCycle,
  dayLength, dayOffset, version)`. Track altitudes stay the region's, because
  the reference assigns `mTrackAltitudes` in the region branch only; a parcel
  moving the sky-track bands under an agent climbing through them would be a
  bug, and there is a test pinning it.

Stepping over a parcel line drops the old override immediately rather than
holding it until the new parcel answers: the region's environment is always a
defensible thing to draw and the parcel behind you never is. A reply for a
parcel the agent has since left is dropped, as the reference drops one whose id
is not the agent parcel's.

## Not done — and why

- **No trackball for the sun and moon.** The two angle sliders are the whole of
  the state; the reference's trackball is a second way to drive them and wants a
  widget the toolkit does not have. Filed as
  [[viewer-ui-virtual-trackball]], which adopts it here once the widget
  exists — the reference keeps the sliders beside it, so this is an addition
  rather than a replacement.
- **No sun / moon beacon checkboxes.** They are the beacons feature's state
  ([[viewer-beacons-control]]), not this window's; the reference only puts them
  here for convenience.

## Verified

`cargo test --release -p sl-viewer-environment` — 8 knob tests green (every sky
and water knob round-trips, a water knob leaves its packed siblings alone, the
sun and moon are placed separately, the colour scales round-trip, a sun pick
keeps its alpha, an unset texture shows the built-in default).
`cargo test --release -p sl-viewer-world-scene --lib -- environment::` — 25
green, including six over the fade and the saved environment and six over the
parcel layer (a parcel override renders over the region; a parcel with no
override keeps the region's sky; a reply for another parcel is dropped; walking
off a parcel drops its environment; a parcel cannot move the track altitudes;
the local layer still wins over a parcel).
`cargo test --release -p sl-proto --lib` — the two new angle round-trip tests
green.

Not verified live: the window's layout, the colour and texture pickers, the fade
and the parcel layer were not driven against a grid. The parcel layer is the one
worth driving — it is the only part whose behaviour depends on what a grid
actually sends, and OpenSim's Default Region has parcels to walk between.
