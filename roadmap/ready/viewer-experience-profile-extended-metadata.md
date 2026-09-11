---
id: viewer-experience-profile-extended-metadata
title: Experience profile — the marketplace link, the logo, and the owning group
topic: viewer
status: ready
origin: split out of [[viewer-experiences-floater]] when the profile window was
  built (2026-09-11)
refs: [viewer-experiences-floater, viewer-social-group-profile]
---

Context: [context/viewer.md](../context/viewer.md).

The Experience Profile window
(`sl-viewer-notices/src/experience_profile.rs`) shows and edits every field of
an experience record except three, all of which need machinery it does not have
yet:

- **The marketplace store link** and **the logo texture**. Both live inside
  `ExperienceInfo::extended_metadata`, an opaque **LLSD-XML** string the wire
  layer carries verbatim; the reference parses it with an `LLSDXMLParser` and
  re-serialises it on save (`LLFloaterExperienceProfile::updatePackage`). No
  LLSD-XML parser is exported to the viewer crates — `sl-client-bevy`
  re-exports the `Llsd` value type but no XML codec — so wiring these two up
  starts with deciding what the viewer's LLSD document API is, not with the
  window.
- **Re-assigning the owning group** (the reference's `Group_btn` →
  `LLFloaterGroupPicker`). Needs the group picker the viewer has not grown yet,
  and `ExperienceUpdate` carries no `group_id` field — the cap takes one, so
  this is a protocol edit as well as a UI one.

Until then the window **preserves** what it cannot show: the blob goes back to
`UpdateExperience` byte-for-byte, and the owner is not part of the update at
all, so an administrator editing the name cannot silently delete their own
store link. That is the property to keep when this task lands — and the one
worth a test.

The Owned tab's **acquire** button (`LLFloaterExperiences::sendPurchaseRequest`)
is a separate gap of the same shape: there is no purchase command in our
protocol surface at all. File it here as a note rather than as scope; it wants
its own task once the commerce surface exists.

Reference (Firestorm, read-only): `llfloaterexperienceprofile.cpp`
(`refreshExperience`, `updatePackage`, `onPickGroup`),
`floater_experienceprofile.xml`.
