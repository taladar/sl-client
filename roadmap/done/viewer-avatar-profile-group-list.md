---
id: viewer-avatar-profile-group-list
title: Avatar profile — make the 2nd-Life groups a proper (clickable) group list
topic: viewer
status: done
origin: user request (2026-07-27), after wiring viewer-social-group-profile
refs: [viewer-social-group-profile, viewer-social-profiles]
---

## Done (2026-07-27)

The 2nd-Life tab (now retained-mode, `build_second_life_structure` +
`update_second_life`) renders the groups as a
**bounded, scrollable, full-width** list of clickable **group-name** rows
(`spawn_profile_group_row` + `ProfileGroupRow` + `on_profile_group_open`),
sorted alphabetically (case-folded), reconciled in place by a set-signature so a
groups reply builds them once. **Double-clicking** a row (its own
`ProfileGroupClick` tracker) opens the group profile floater
(`group_profile::OpenGroupProfile { group }`). It is the practical way to reach
the group profile before the search floater lands and while the logged-in avatar
is in no groups: open any resident's profile and double-click one of their
listed groups.

**Deferred (matching the reference, small follow-ups):** the group **insignia**
thumbnail (an early rows-with-a-`Button`-and-thumbnail layout collapsed the row
to the fixed thumbnail box, and the reference group list has no insignia anyway
— a plain name list; fold insignia into [[viewer-group-insignia-editing]] if
wanted) and **bolding the groups shared with the viewed avatar** (Firestorm
shows shared groups bold). The double-click interval is a per-widget const
pending [[viewer-consolidate-double-click-interval]].

Context: [context/viewer.md](../context/viewer.md).

The avatar profile floater's **2nd Life** tab ([[viewer-social-profiles]]) shows
the avatar's groups (`AvatarGroupsReply`) as a plain column of **static name
labels** — it is not a real list. Make it a proper group list matching the
reference (`llpanelprofilesecondlife` group list):

- each group row is **clickable** and opens the group [profile
  floater](../ready/viewer-social-group-profile.md) via
  `group_profile::OpenGroupProfile { group }` (the avatar profile was the entry
  point a user reached for first);
- show each group's **insignia** thumbnail beside the name (the reply carries
  the group id → fetch the insignia like other UI textures), and elide the
  overflow with a scroll rather than the current fixed `max_height` clip;
- carry the group id through `AvatarGroupMembership` (already available) so the
  row has it to open on.

Small, self-contained: the group profile floater and its `OpenGroupProfile`
message already exist; this is the avatar-profile-side list widget + wiring.
