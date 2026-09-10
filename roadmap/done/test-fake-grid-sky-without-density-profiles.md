---
id: test-fake-grid-sky-without-density-profiles
title: A sky frame with no scattering profiles is a region with no environment
topic: test
status: done
origin: pointing Firestorm at the fake grid ([[test-firestorm-fake-grid-crosscheck]], 2026-09-10)
points: 3
refs: [test-firestorm-fake-grid-crosscheck, protocol-experience-environment-push]
---

Context: [context/testing.md](../context/testing.md).

The first Firestorm capture against the fake grid found it, and it was
invisible from this side: **the reference viewer throws the region's whole day
cycle away**, silently as far as the picture is concerned. Its log does not:

```text
Missing required setting 'rayleigh_config' with no default.
Missing required setting 'absorption_config' with no default.
Missing required setting 'mie_config' with no default.
Sky setting named 'Default' validation failed!
No skies defined.
Must have at least one water and one sky frame!
Invalid day cycle for region
```

`LLSettingsSky::settingValidation` lists all three atmospheric-scattering
profiles as **required with no default** (`llsettingssky.cpp`), and
`Validator::verify` fails a required field it cannot fill. A sky frame that
fails validation is dropped from its track, a day cycle with no sky frames
fails `initialize`, and `LLEnvironment::recordEnvironment` refuses the lot —
so the region ends up with no environment at all and the viewer draws its own
built-in default sky.

`sky_settings_to_llsd` never wrote them, because `SkySettings` never modelled
them: the module said so out loud ("intentionally not parsed here"), on the
grounds that this workspace's renderer takes its atmosphere from the
`legacy_haze` block. That reasoning covers the *decoder*. It does not cover the
encoder, which has two users that both put the document on a wire a reference
viewer reads — the fake grid's `ExtEnvironment` GET, and the client's own
environment PUT.

## What it cost

Everything downstream of "the region has a sky", none of it reported as an
error:

- every Firestorm frame of every scenario was lit by Firestorm's default sky
  rather than by the region's;
- the cross-check runner's `--day-position` did nothing at all, because the
  harness pins the sun by sampling *the region's day cycle* and there was
  none — two runs four hours of simulated time apart came back
  indistinguishable;
- and the precondition
  [[test-firestorm-fake-grid-crosscheck]] recorded as met on 2026-09-01 — "the
  stock region environment is a real single-keyframe cycle, so two snapshots
  are comparable" — was met on the wire and not in the viewer.

## The fix

`DensityLayer` in `sl-proto`: `width`, `exp_term`, `exp_scale`, `linear_term`,
`constant_term`, and the Mie-only `anisotropy` the reference omits when it is
zero. `SkySettings` carries the three profiles as layer lists, the codec reads
and writes them, `legacy_windlight_default` seeds the reference's own defaults
(`rayleighConfigDefault` / `mieConfigDefault` / `absorptionConfigDefault`), and
a frame blend interpolates layer for layer where the two lists have the same
shape and snaps at the halfway mark where they do not — interpolating between
a one-layer and a two-layer profile would invent a third shape neither frame
asked for.

Modelling them rather than emitting a constant is what makes the **server**
half honest: a viewer that PUTs a custom atmosphere and reads it back now gets
its own atmosphere, where before the grid would have handed back defaults.

## A parallel implementation on `ui-features`, and the one line that differs

`6953ffc4` on the `ui-features` branch reached the same three fields
independently, for the settings-asset editor: the same `DensityLayer`, the
same pair of codec helpers, the same three reference-default constructors.
Whoever merges the two should expect a conflict in
`sl-proto/src/types/environment.rs` and `session/conversions.rs` and can take
either shape — **except for one line, where the two branches genuinely
disagree and this one is right**:

- there, `legacy_windlight_default` leaves the three profiles **empty**, and
  the encoder omits a key whose profile is empty, reasoning that "the
  reference substitutes its own defaults for an absent one";
- here they are **seeded** with the reference's defaults, because it does
  not. `Validator(SETTING_RAYLEIGH_CONFIG, /* required */ true, …)` has no
  `mDefault`, so `verify` fails outright, and the log above is what that
  costs.

Both are wanted, and they are not in tension once stated apart: a frame
*decoded* from a document keeps whatever profiles it had (the editor's
round-trip property), and a frame *constructed* in code carries the
reference defaults (this one), because a constructed frame is going on a wire
to a viewer that requires them.

## How it was verified

The same way it was found: a Firestorm capture against the fake grid, checking
that `sky pinned at day position …` appears in the viewer log and that two
runs at different `--day-position` values differ. Unit round-trips cover the
codec.
