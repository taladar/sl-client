---
id: viewer-probe-auto-placement
title: Automatic reflection-probe placement and sky-only default probe
topic: viewer
status: ideas
origin: reflection-probe performance planning round (2026-07-22), Firestorm LLReflectionMapManager survey
refs: [viewer-p33-2, viewer-perf-probe-capture-content, viewer-perf-probe-scheduling]
---

Context: [context/viewer.md](../context/viewer.md).

Quality / reference-parity record from the perf round's Firestorm
survey, not itself a perf item. The reference maintains up to 256
resident probes: besides manual probe prims it **auto-places** one probe
per ~16 m occupied spatial-octree cell, refines each origin by
ray-casting toward the cell contents (`autoAdjustOrigin`), and keeps the
nearest set resident. *Because* coverage is then ubiquitous, its default
probe renders **only sky / water / terrain / clouds** — the cheap
capture the default probe wants.

The two halves are coupled: a sky-only default probe **without**
auto-placement would degrade the many SL scenes that contain no probe
prims (today our full-scene default probe is what covers them), which is
why sky-only-default lives here and not in
[[viewer-perf-probe-capture-content]]. Also the natural home for
revisiting `MAX_LOCAL_PROBES = 4` — auto-placement only makes sense with
a much larger rig pool, which in turn presupposes cheap captures and the
change-driven scheduler (refs, not blockers, while this is unshaped).
Bevy binds at most 8 probes per view (nearest win), so "resident" and
"bound" would diverge the way the reference's 256-vs-bound set does.
