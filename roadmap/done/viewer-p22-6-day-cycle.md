---
id: viewer-p22-6
title: Day cycle
topic: viewer
status: done
origin: VIEWER_ROADMAP.md — Phase 22 — Sky & atmosphere (day cycle, EEP)
---

Context: [context/viewer.md](../context/viewer.md).

**P22.6. Day cycle.** Interpolate the `LLSettingsDay` keyframes over
region time (`getBlendedSettings`) to animate the sky and sun through the
day, replacing P22.2's active-keyframe (unblended) selection with the smooth
blend between the bounding keyframes.

**Done.** Pure `sl-proto` addition, then a viewer swap. In
`sl_proto::types::environment`: `SkySettings::blend(&self, other, factor)`
interpolates one sky frame toward another the way the reference
`LLSettingsBase::blend` does over the sky settings map — every numeric channel
(haze scalars, colours, cloud/glow parameters, radii, star brightness, …) is
linearly interpolated, the sun and moon rotations are **slerped** (the
reference marks `sun_rotation` / `moon_rotation` as slerp keys — shortest-arc,
with a normalised-lerp fallback for near-parallel inputs), and the discrete
non-blendable settings (frame name + the six texture ids) snap to whichever
frame is nearer (`factor > 0.5` picks `other`, matching the reference's
`mix > 0.5 ? other : this`). A new private `bounding_keyframes(track,
position)` finds the `(lower, upper)` day-cycle keyframes bracketing the
current normalised time and the blend factor between their keyframe times,
wrapping across the day boundary at both ends (upper wraps to the first frame
after the last keyframe, lower to the last before the first) and
special-casing a single-keyframe track to a factor-`0.0` self-blend; and
`EnvironmentSettings::blended_sky_settings(altitude, position)` ties them
together — selecting the altitude track (P22.2's
`sky_track_for_altitude`), bracketing the position, and returning the blended
(owned) `SkySettings`, falling back to any defined frame / holding the lower
frame when the upper is missing. The unblended `active_sky_settings` is kept
for the borrow-returning callers/tests. In the viewer, `sky.rs`'s five drivers
(`setup_sky` / `drive_sky` / `drive_sun_moon_discs` / `drive_clouds` /
`drive_stars`) now pull `blended_sky_settings` in place of
`active_sky_settings` every frame, so the whole sky stack (dome atmosphere,
scene sun/moon light + ambient, sun/moon discs, cloud layer, star field)
animates continuously. 8 new `sl-proto` unit tests (bounding-keyframe
bracketing + wrap + single-frame case; blend
scalar/endpoint/slerp/texture-snap; `blended_sky_settings` interpolation +
default-cycle no-op); `cargo test -p sl-proto` green (233).
Verified live on OpenSim: the **Default region ships a real 8-sky-frame day
cycle** (`day_length=14400s`, `day_offset=57600s`), so the blend is genuinely
exercised — pinning `SL_VIEWER_SKY_DAY_POSITION` to `0.25` vs `0.75` renders
two distinctly different skies (~7 % mean per-pixel difference; sky avg RGB
`[211,235,255]` vs `[244,254,255]`) with the placeholder-sphere avatar visibly
lit from a different sun direction (upper-left daylight vs shadowed), proving
the interpolated sun rotation and sky settings drive the scene with no
rendering regression.
