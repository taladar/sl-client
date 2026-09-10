---
id: viewer-environment-density-profiles
title: Render the atmospheric density profiles
topic: viewer
status: ready
origin: Split out of viewer-environment-fixed-editor (2026-09-09)
refs: [viewer-environment-fixed-editor, viewer-p22-4]
---

Context: [context/viewer.md](../context/viewer.md).

An EEP sky carries three atmospheric-scattering density profiles —
`rayleigh_config`, `mie_config`, `absorption_config`, each an array of
layers with a width, an exponential term and scale, a linear term and a
constant (plus a Mie layer's `anisotropy`). The reference feeds them to
`LLAtmosphere`, which precomputes the scattering tables its deferred sky
shaders sample.

This viewer already **carries and authors** them: `sl_proto::DensityLayer`
decodes and re-encodes them verbatim, and the sky settings editor's Density
tab edits layer 0 of each ([[viewer-environment-fixed-editor]]). What is
missing is the half that makes them visible — the sky renders from the
legacy WindLight formula (`blue_density`, the haze scalars, the
multipliers), so those sixteen sliders change the asset and not the picture.

Wanted: a scattering model that reads the three profiles, or a documented
decision that this viewer's sky stays the legacy formula and the profiles
are authored-and-forwarded data forever. The second is a defensible answer
— every other viewer reads them, so an asset edited here is still correct
elsewhere — but it should be a decision on the record rather than a gap.

The editor needs nothing further either way. If the renderer lands, the
existing tab starts previewing; the note in `settings_editor.rs` saying it
does not is then the thing to delete.

Reference (Firestorm, read-only): `llsettingssky.cpp`
(`rayleighConfigDefault` and friends), `llatmosphere.cpp`,
`panel_settings_sky_density.xml`, `llpaneleditsky.cpp`
(`LLPanelSettingsSkyDensityTab`).
