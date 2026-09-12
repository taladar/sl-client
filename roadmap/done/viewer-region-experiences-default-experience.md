---
id: viewer-region-experiences-default-experience
title: RegionExperiences — the estate's default experience
topic: viewer
status: done
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

## Done (2026-09-12)

The `default` key now travels the whole way: decoder → event → sim encoder →
fake-grid fixture → the Experiences tab's sticky row and its two picker
exclusions.

- **Wire.** `parse_region_experiences` returns a `RegionExperienceLists`
  (`allowed` / `blocked` / `trusted` / `default_experience`) instead of a
  triple, which also retires the two `clippy::type_complexity` waivers the
  tuple carried. `build_region_experiences_response` takes the same struct and
  writes `default` **only** when there is one — the reference reads the key by
  presence, so a null id would say the same thing at more length.
  `parse_region_experiences_request` decodes a `default` a POST body should not
  have rather than rejecting it.
- **Sim.** `SimExperiences::region_default` with its own
  `set_region_default_experience` setter, deliberately not a fourth argument to
  `set_region_lists`: the three lists are what the POST replaces wholesale,
  and the default is an estate property the viewer only reads. So
  `apply_region_lists` leaves it alone — and it *must*, because the id does come
  back inside the posted `trusted` array, which a POST that re-read the default
  from its body would then have nothing to distinguish.
- **Fake grid.** A fifth hand-written record, `Fake Grid Estate Default`
  (offset 5), land-scoped and in **none** of the three lists — so a Key row
  carrying it can only have come from `default`, and the Allowed picker's
  exclusion has something to exclude (land-scoped is exactly what that picker
  otherwise offers).
- **Viewer.** `AboutRegionState::default_experience`, set only by a reply that
  names one (`content.has("default")` — an omission is silence, not a clear);
  appended to the Key list when the trusted array does not already name it;
  `experience_is_sticky` refuses its Remove in `remove_experience` *and* hides
  that row's Remove button; and the Allowed / Blocked Adds open the picker with
  `OpenExperiencePicker::excluded` set to it.
- **Picker.** `OpenExperiencePicker::excluded` — the reference's second,
  `FilterMatching` filter. Applied beside `filter_admits` rather than inside it
  because it needs no resolved record, and reset per open (`restart` now takes
  the whole open, so a second Add that names no exclusion does not inherit the
  first one's).

### Divergence from the reference, deliberate

The reference **greys** Remove for a sticky selection
(`LLPanelExperienceListEditor::checkButtonsEnabled`) because its Remove is one
button for the whole list and has to stay put while the selection moves. Ours
is a button per row, so it is **hidden** on the sticky row — the same statement
made in the place it is about.

### Still unobserved: does SL send the key?

The task asked for evidence from a real grid first. There is none to be had
without an estate-managed region on aditi, so the field is decoded leniently
(absent / `<undef/>` / unparsable → no default) and the conformance case
`experience_admin_contributor` now records a **`region_default`** metric:
`-1` the cap did not answer, `0` it answered without the key, `1` it named an
experience. The next aditi run of that case answers the question in the record
rather than in a comment.

Verified: the wire round trips (both directions, with and without the key),
`SimCaps`' GET and POST both carry it, the fake-grid fixture keeps it out of
the three lists, and the viewer's pin / sticky / re-post cycle does not double
the row. Not yet eyeballed in the running viewer.
