---
id: viewer-region-experiences-panel
title: Region / Estate floater — Experiences tab
topic: viewer
status: ready
origin: Firestorm full-parity audit (2026-08-19)
refs: [viewer-region-options-estate, viewer-experiences-floater,
  viewer-parcel-config-missing-writes, protocol-27]
---

Context: [context/viewer.md](../context/viewer.md).

The reference Region/Estate Experiences panel
(LLPanelRegionExperiences) manages the estate's three experience lists —
Trusted, Allowed, Blocked — with add via an experience picker and
per-row remove, plus read-only captions explaining where each list
applies.

Our About Region Experiences tab is a permanently-disabled placeholder
(`sl-client-bevy-viewer/src/about_region.rs`; its module doc claims the
write path "is its own roadmap item", but until now no task covered the
region half). The protocol is fully paired in sl-proto: the
`RegionExperiences` cap with `RequestRegionExperiences` /
`SetRegionExperiences` (`sl-proto/src/session.rs`) and
`Event::RegionExperiences` (`sl-proto/src/event.rs`), delivered by
[[protocol-27]]. Implementing this means building the three-list tab
over those commands, estate-manager gated, reusing whatever experience
picker [[viewer-experiences-floater]] grows. The parcel-side experience
lists stay with [[viewer-parcel-config-missing-writes]] (no per-parcel
message in our scope enum yet).

Reference (Firestorm, read-only):
`indra/newview/llfloaterregioninfo.cpp` (LLPanelRegionExperiences),
`indra/newview/skins/default/xui/en/panel_region_experiences.xml`.
