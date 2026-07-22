---
id: viewer-contact-sets
title: Contact sets — named, coloured contact groups
topic: viewer
status: ready
origin: Vintage-parity coverage audit (2026-07-22)
blocked_by: [viewer-social-people-panel]
refs: [viewer-avatar-radar, viewer-name-tags-decorations, viewer-ui-color-picker]
---

Context: [context/viewer.md](../context/viewer.md).

Firestorm's **contact sets**: user-defined named groups of residents (not SL
groups — purely client-side), each with a colour, used to organise a large
friends list and to tint that person everywhere they appear — the contacts
list, the radar ([[viewer-avatar-radar]]), name tags
([[viewer-name-tags-decorations]]) and chat names. A resident can belong to
several sets; pseudonyms/notes per entry are part of the reference feature.

Scope: the contact-set model persisted in the account dirs, a Contacts-tab UI
to create/rename/recolour sets and add/remove residents (colour choice via
[[viewer-ui-color-picker]]), the add-to-set entry in the avatar context menu,
and a small query API the tinting consumers (radar, tags, chat) read so each
lands independently.

Reference (Firestorm, read-only): `lggcontactsets`,
`floater_fs_contact_add.xml`, `floater_fs_contact_set_configuration.xml`,
`panel_people_contact_sets.xml`.

Builds on: the People panel ([[viewer-social-people-panel]]) and the
per-avatar account dirs.
