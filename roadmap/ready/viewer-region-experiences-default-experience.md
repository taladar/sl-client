---
id: viewer-region-experiences-default-experience
title: RegionExperiences — the estate's default experience
topic: viewer
status: ready
origin: split out of [[viewer-region-experiences-panel]] when the Experiences
  tab was built (2026-09-12)
refs: [viewer-region-experiences-panel, protocol-27]
---

Context: [context/viewer.md](../context/viewer.md).

A `RegionExperiences` GET reply can carry a fourth key beside the three id
arrays: `default`, the estate's **default experience**. The reference reads it
(`LLPanelRegionExperiences::processResponse`), appends it to the **Key**
(trusted) list, and marks it *sticky* — a row the panel shows but refuses to
remove (`LLPanelExperienceListEditor::setStickyFunction`, which is also what
greys the Remove button for it). It is filtered out of the Allowed and Blocked
pickers as well (`FilterMatching(mDefaultExperience)`), since the estate's own
default is not something to allow or block.

We decode only `{ allowed, blocked, trusted }`
(`sl_wire::parse_region_experiences` → `Event::RegionExperiences`), so the row
is simply absent from our Experiences tab. Nothing is *lost* by this — the
write posts back whatever the grid sent, so a default the grid already had in
`trusted` round-trips — but an administrator cannot see which experience the
estate defaults to, and our panel would let them try to remove it if the grid
did list it (the region refuses, and the reply reverts the row, so the failure
is visible rather than silent).

Scope: one optional `ExperienceKey` through the decoder, the event, the
sim-side encoder and the fake grid's fixture, then the sticky row and the two
picker exclusions in `about_region`'s Experiences tab. Worth checking against
a real grid first — aditi's reply is the only evidence of whether SL still
sends the key at all, and a field we cannot observe is one to decode
leniently.

Reference (Firestorm, read-only): `llfloaterregioninfo.cpp`
(`LLPanelRegionExperiences::processResponse`, `refreshFromRegion`),
`llpanelexperiencelisteditor.cpp` (`setStickyFunction`,
`checkButtonsEnabled`).
