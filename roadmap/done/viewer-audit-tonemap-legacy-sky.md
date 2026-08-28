---
id: viewer-audit-tonemap-legacy-sky
title: ACES tonemapping is applied to legacy skies the reference exempts
topic: viewer
status: done
origin: static code audit (2026-08-26)
points: 2
refs: [viewer-audit-scene-change-guards-day-cycle]
---

Context: [context/viewer.md](../context/viewer.md).

`sl-viewer-world-scene/src/tonemap.rs:209` applies `RenderTonemapMix`
unconditionally — the file contains **zero** references to `can_auto_adjust` or
`reflection_probe_ambiance`, while the flag is already derived one module over
(`exposure.rs:790`, `can_auto_adjust: reflection_probe_ambiance == 0.0`) and
simply never consumed.

The reference exempts legacy skies entirely: `llsettingssky.cpp:2066` returns a
`0.0` tonemap mix for them, and `pipeline.cpp:7912` selects
`gNoPostTonemapProgram` when probe ambiance is 0. Both aditi and the local
OpenSim serve legacy skies, so this is wrong on **every grid currently tested
against**.

Adjacent, same function: `:213` sets `tonemap.exposure` unclamped while
`tonemap_mix` one line up is clamped.

## Fixed (2026-08-28)

`SlTonemap` grew a `no_post` field (in the u32 slot the std140 padding used to
occupy), driven each frame from the active sky by `refresh_tonemap_settings`,
and `tonemap.wgsl` takes the reference's `NO_POST` path when it is set: it
returns `clamp(source.rgb, 0, 1)` and nothing else.

The audit named two reference sites; they are not the same strength, and the
stronger one is what got ported:

- `LLSettingsSky::getTonemapMix` returns `0` for a classic sky, which would
  leave `mix(exposed_linear, curve, 0)` — i.e. the frame still multiplied by
  `RenderExposure * exposureMap`.
- `LLPipeline::tonemap` binds `gNoPostTonemapProgram` for the same sky, and
  that program's `postDeferredTonemap.glsl` never calls `toneMap` at all. So
  the exposure multiply goes too.

Both are ported (the mix is zeroed as well as the branch taken), because the
reference sets both and a zero mix is the thing a reader looking for
`getTonemapMix` expects to find. Under the branch the mix is dead, which the
shader comment says.

The two reference conditions are spelled differently —
`canAutoAdjust() && !RenderSkyAutoAdjustLegacy` for the mix,
`getReflectionProbeAmbiance(auto) == 0` for `no_post` — but our decode collapses
`mCanAutoAdjust` to `reflection_probe_ambiance == 0` (an EEP sky authoring an
ambiance of exactly `0` is indistinguishable from a legacy one), and under that
collapse the two agree exactly. One `is_classic_sky` therefore serves both, and
says so.

`can_auto_adjust` is read from the `ExposureRange` resource `drive_sky` already
publishes, so the tone mapper tracks the same altitude-blended sky frame the sky
dome is drawn from rather than re-deriving one.

`SL_VIEWER_TONEMAP_FORCE_POST` pins the exemption off, so a capture pair with
and without it shows what the exemption is worth on the grid in front of you —
which is the only way to judge this one, both test grids serving legacy skies.

Adjacent fix: `RenderExposure` is now held to the reference's
`llclamp(exposure(), 0.5f, 4.f)`, in `refresh_tonemap_settings` and in the
`SL_VIEWER_EXPOSURE` override alike (the mix's env override was already
clamped). `refresh_tonemap_settings` also re-derives the mix from the store /
env every frame instead of leaving the live field alone on a failed read: the
classic-sky zero is written into that field, so carrying it forward would make
the exemption stick after the sky stopped being a legacy one.

### Tests

Four in `tonemap.rs`, on the two pure functions the system is now built from:
the classic-mode truth table all four ways round (legacy vs EEP × auto-adjust on
vs off), the `SL_VIEWER_TONEMAP_FORCE_POST` override of it, `getTonemapMix`'s
zero-whatever-the-setting-says, and the reference's exposure clamp bounds.
