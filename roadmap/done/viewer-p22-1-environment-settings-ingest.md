---
id: viewer-p22-1
title: Environment-settings ingest
topic: viewer
status: done
origin: VIEWER_ROADMAP.md — Phase 22 — Sky & atmosphere (day cycle, EEP)
---

Context: [context/viewer.md](../context/viewer.md).

The scene has one static directional light today. This phase renders the SL
sky with its atmospheric model, driven by the region's Environment (EEP)
settings and animated through the day cycle. Its ingested settings also feed
Phase 23 (water) and Phase 24 (shadows).

**P22.1. Environment-settings ingest.** Parse region / parcel EEP
settings (`LLSettingsSky` / `LLSettingsWater` / `LLSettingsDay`) with a
legacy WindLight fallback, wired to the viewer through a new
`EnvironmentUpdated` `SlEvent` (reuse the Phase 11 conformance environment
work; keep the parse Bevy-free). Reference: `LLEnvironment`.

**Done:** the parse + `Event::Environment` plumbing already existed from the
Phase 11 conformance work (`environment_from_llsd` in `sl-proto`, surfaced to
the viewer as `SlEvent(SessionEvent::Environment(..))` — no bespoke
`EnvironmentUpdated` variant needed, the generic `SlEvent` wrapper already
carries it). Net-new: a Bevy-free
`EnvironmentSettings::legacy_windlight_default` (+
`SkySettings::legacy_windlight_default` / `WaterSettings::legacy_default`)
in `sl-proto`, transcribing Firestorm's `LLSettingsSky::defaults` /
`LLSettingsWater::defaults` (incl. the legacy-haze `LLColor3`/`F32` fallbacks
and the position-0 sun/moon `convert_azimuth_and_altitude_to_quat` tracks); a
new viewer `EnvironmentState` resource (`environment.rs`) holding the current
settings + provenance (`EnvironmentSource::{Default,Region,Parcel}`), starting
at the legacy default; `request_environment` (asks for the whole-region
environment on each `RegionHandshakeComplete`) and `ingest_environment` (folds
the reply in, logs day length / offset / frame counts / cycle name). Also
re-exported `SkySettings` / `WaterSettings` / `DayCycle` / `DayCycleFrame`
from both runtime crates for parity (P22.2 needs them).

**Model note (region = default, parcel = override, altitude = sky track):**
the *region* environment is the baseline default; a *parcel* may override it
where the region flags permit, and within either the day cycle carries up to
four `sky_tracks` selected by camera altitude against `track_altitudes` (water
is a single region-wide track). P22.1 ingests the region baseline; requesting
the current parcel's override and picking the sky track by altitude are
render-time concerns for P22.2/P22.3, which read the already-stored
`EnvironmentSettings` (it carries `track_altitudes` + all `sky_tracks`).
