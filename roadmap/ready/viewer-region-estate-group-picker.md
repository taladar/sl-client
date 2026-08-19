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

## Parity-audit addendum (2026-08-19)

The group picker this task builds (`floater_choose_group.xml` in the
reference) is a **generic reusable picker**, not an estate-only widget:
the same surface serves group notices, avatar-profile group selection,
and About Land General's group assignment. In particular, wire About
Land's "Set…" (change parcel group) through it — the write path already
exists as `ParcelUpdate.group_id` (`sl-proto/src/types/parcel.rs`), and
`AboutLandAction` in `sl-client-bevy-viewer/src/about_land.rs` has no
group-set action today. Build it group-picker-shaped like
`avatar_picker.rs`, then reuse everywhere.
