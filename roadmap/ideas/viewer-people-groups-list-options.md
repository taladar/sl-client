---
id: viewer-people-groups-list-options
title: Contacts & groups list display options
topic: viewer
status: ideas
origin: Firestorm full-parity audit (2026-08-19)
refs: [viewer-social-people-panel, viewer-social-groups,
       viewer-table-widget-remaining, viewer-group-titles-quick-switch]
---

Context: [context/viewer.md](../context/viewer.md).

Presentation options for the shipped people panel
([[viewer-social-people-panel]]) and groups list
([[viewer-social-groups]]): contact/friends sort orders
(`FSContactsSortOrder`, `FSFriendListSortOrder`) and the name-format
choice for friend rows — username / display name / full name
(`FSFriendListFullNameFormat`). The wider friends-list column set
(permissions columns, search-filter toggle) is already scoped in
[[viewer-table-widget-remaining]]; this task is the sort/format layer
on top.

On the groups side, Firestorm keeps user-pinned favourite groups at the
top of the list (`FSFavoriteGroups`, per-account) — that pinning
belongs to [[viewer-group-titles-quick-switch]] and is only referenced
here. Showing invitations for groups the avatar already joined
(`FSShowJoinedGroupInvitations`) is an offer-dialog policy tracked as
an extension of the auto-reject/offer tasks, not here.

Reference (Firestorm, read-only): `indra/newview/fspanelcontacts.cpp`,
`indra/newview/fsfloatercontacts.cpp`,
`indra/newview/app_settings/settings.xml` +
`settings_per_account.xml` (FSContactsSortOrder, FSFriendList*,
FSFavoriteGroups).
