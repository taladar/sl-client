---
id: viewer-friends-list-shows-a-sixth-rights-column
title: The Friends list shows six rights columns; the reference shows five
topic: viewer
status: bugs
origin: the user comparing the Friends list against the reference (2026-09-24)
refs: [viewer-skin-icon-set]
---

Context: [context/viewer.md](../context/viewer.md).

## Observation

Our Friends list (`sl-viewer-people/src/people.rs`, `RIGHT_COLUMNS`) draws
**six** rights columns, three per direction:

- "They can…": see me online, locate me on the map, edit my objects;
- "You can…": see them online, locate them on the map, edit their objects.

The reference draws **five**. `panel_fs_contacts_friends.xml` has
`icon_visible_online`, `icon_visible_map` and `icon_edit_mine` for what the
friend may do, but only `icon_visible_map_theirs` and `icon_edit_theirs` for
what the agent may do. It has no "you can see them online" column.

## Why the reference leaves it out

The user's point, and the reason the column is misleading rather than just
extra: the online right is not something you can see from your side. A
friend who has not let you see them online simply looks offline, and the
presence dot already shows exactly that. A column claiming to show whether
they granted it either repeats the dot or shows a flag the viewer cannot
observe in the way the column suggests. Check this against the wire before
fixing: confirm what `FriendRights` actually carries for the received
direction, and whether the grid ever sends the online bit for it. That
decides whether the column only duplicates information or is actively wrong.

## What to do

- Drop the received `SeeOnline` entry from `RIGHT_COLUMNS`, and with it the
  sixth header icon, the sixth rights cell a row spawns, its width, the
  `FriendRowParts::rights` array length and the sort column it offered.
- Keep the model's field if the protocol carries it; this is about what the
  list claims to show, not about what the session tracks.
- Re-check the rights header's grouping ("They" over three, "You" over two)
  and the People layout sweep once the table narrows.

## Done when

The Friends list shows the reference's five rights columns, and no column
claims to show a right the agent cannot observe.
