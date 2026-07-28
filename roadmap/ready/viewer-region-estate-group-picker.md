---
id: viewer-region-estate-group-picker
title: Group picker for the Region/Estate Access → Allowed Groups list
topic: viewer
status: ready
origin: user request (2026-07-28) — follow-up found while building the About
  Region floater (viewer-region-options-estate)
---

Context: [context/viewer.md](../context/viewer.md).

The Region / Estate floater's **Access** tab has four estate access lists
(managers, allowed residents, allowed groups, banned residents). Three are
fully wired — **Add** opens the avatar picker
(`crate::avatar_picker::OpenAvatarPicker`) and per-row **Remove** commits an
`estateaccessdelta` — but **Allowed Groups** currently only supports display
and per-row Remove: there is **no group picker**, so its **Add** button is
omitted.

This task adds a **group picker** widget (the group analogue of
`avatar_picker.rs` — search groups by name / list the agent's groups) and
wires it into the About Region Access tab so a manager can add a group to the
allowed-groups list. The write path already exists
(`Command::UpdateEstateAccess` with `EstateAccessDelta::AllowedGroupAdd`, target
`OwnerKey::Group`); this is the missing picker plus the `AddAllowedGroup`
action in `about_region.rs` (mirror `AddManager` / `AddAllowed` / `AddBanned`).

A group picker is also reusable elsewhere (e.g. parcel group assignment).

Reference (Firestorm, read-only): `llfloatergrouppicker`,
`panel_region_access.xml` (the allowed-groups sub-tab Add button).
