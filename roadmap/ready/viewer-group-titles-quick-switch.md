---
id: viewer-group-titles-quick-switch
title: Group titles & favourite groups quick-switch
topic: viewer
status: ready
origin: Firestorm full-parity audit (2026-08-19)
refs: [viewer-social-groups, viewer-social-group-profile,
       missing-out-batch-6]
---

Context: [context/viewer.md](../context/viewer.md).

Firestorm's Group Titles floater (`fsfloatergrouptitles.cpp`,
`floater_fs_group_titles.xml`, toolbar command `group_titles`) flattens
every group membership into one list of group × role-title rows with
the active title highlighted; double-click or the Activate button both
activates that group and selects that role's title in one step, with a
refresh button and a filter box. It is the fast path for people who
switch tags often — the alternative is opening each group profile's
General tab.

Two companion pieces belong in the same surface: **favourite groups**
(`fsfavoritegroups.cpp`) pin chosen groups to the top of the list,
persisted per account; and the **per-region auto-switch**
(`fsgrouptitleregionmgr.cpp`) auto-activates a configured group/title
when entering a specific region (e.g. estate staff tags).

Ours has the groups list with Activate
(`sl-client-bevy-viewer/src/groups.rs`, [[viewer-social-groups]] done)
and role data via the group-profile roster
([[viewer-social-group-profile]] done), but no title-centric
quick-switch surface, no favourites, and no per-region auto-switch.
The wire side is already done: GroupRoleDataRequest per group plus
GroupTitleUpdate / ActivateGroup ([[missing-out-batch-6]]).

Reference (Firestorm, read-only):
`indra/newview/fsfloatergrouptitles.cpp`,
`indra/newview/fsfavoritegroups.cpp`,
`indra/newview/fsgrouptitleregionmgr.cpp`,
`indra/newview/skins/default/xui/en/floater_fs_group_titles.xml`.
