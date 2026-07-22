---
id: viewer-environment-day-cycle-editor
title: Day-cycle editor
topic: viewer
status: blocked
origin: Vintage-parity coverage audit (2026-07-22)
blocked_by: [viewer-environment-fixed-editor]
---

Context: [context/viewer.md](../context/viewer.md).

The EEP day-cycle editor: arrange sky (and water) settings as **keyframes
on tracks over a day timeline** — the ground sky track, the water track,
and the altitude sky tracks — with a scrubber previewing any time of day
live, per-keyframe editing (opening the fixed editors from
[[viewer-environment-fixed-editor]] in-place), track copy, day length
metadata, and save/load as a day-cycle settings asset. The P22.6 day-cycle
*interpolation* already renders such assets; this authors them.

Reference (Firestorm, read-only): `llfloatereditextdaycycle`,
`floater_edit_ext_day_cycle.xml`, `llsettingsdaycycle`.

Deps: [[viewer-environment-fixed-editor]] (the per-frame editors and the
settings-asset save path).
