---
id: viewer-contact-sets
title: Contact sets — named, coloured contact groups
topic: viewer
status: done
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

## Built

`contact_sets.rs` is the model: the sets, their members and colours, the
per-account `contact_sets.json`, and one guarded way in — a
`RequestContactSet` the panel, its two floaters and the avatar pie all
write, so the name rules (trimmed, non-empty, not one of the names the
file or the chooser already means) and the null-agent refusal live in one
place. A refused *rename* raises the reference's own
`RenameContactSetFailure`, because a rename that silently does nothing is
the one refusal the user cannot otherwise see.

The colour rule is the reference's, and it is the interesting decision:
someone in several sets takes the colour of the **smallest** set they are
in. The set with three people in it says more about a person than the set
with eighty, and ties break by set name so the answer never depends on
hash order. `color_of` is the whole query API the tinting consumers
(radar, name tags, chat) will read, so each of those lands on its own.

The file is the reference's layout on purpose — a top level keyed by set
name, each with a `color` and a `friends` map — so a list exported from
Firestorm's `settings_friends_groups.xml` ports across by transcription.
Its internal keys (`globalSettings`, `extraAvs`, `Pseudonyms`) are
recognised and skipped rather than read as sets, and an entry that is not
shaped like a set is dropped instead of failing the whole load. One
addition: a `names` map remembering what each member was called when they
were filed, because a set outlives everyone's presence and a list of
UUIDs is not a list of people. As in the render-exception store, a live
name resolution is mirrored over it but never written into it.

`contact_sets_panel.rs` is the fourth People sub-tab, laid out like the
Blocked list beside it: a set chooser (with the reference's *All Sets* and
*No Sets* pseudo-sets), a filter, and a sortable virtualized member table
whose names are tinted with the colour the model gives that person — so
what the panel shows is what the rest of the viewer will show once the
consumers land. New Set… / Delete Set / Remove from Set go through the
reference's own notifications (`AddNewContactSet`, `RemoveContactSet`,
`RemoveContactFromSet`), which the notification catalogue already carried;
Configure… opens the set-settings floater (name + Rename, colour swatch
over [[viewer-ui-color-picker]]); Add Resident… uses the shared avatar
picker, and Move to Set… the add-to-set floater in move mode.

The avatar pie's **Add ▸ Add to Set** slice — a placeholder since the pie
was built, at its reference address — now opens that floater, which is
also the answer to "which set?" that a pie cannot give (it cannot grow a
slice per set). The avatar **profile** floater grew the same entry beside
Block, as the reference has it.

One widget fix fell out of the fourth sub-tab: a horizontal tab strip was
bounded by the *panel prose* width (320 px), so four short tabs in the
wide People pane grew scroll arrows with room to spare either side.
Horizontal strips now have their own bound (about eight tabs), the mirror
of the vertical strip's height bound, and the layout test pins both ends
of it.

A new set gets a distinct colour from a small palette instead of the
reference's default grey: the one thing a set *is* is its colour, and
starting every set invisible makes the picker a required step rather than
a correction.

**Deliberately not built**, each for the same reason — no consumer here
yet, and a stored knob nothing reads is a lie:

- the reference's per-set *notify* / *sort by online status* / three
  autoresponse settings (they belong with the presence toasts and
  [[viewer-do-not-disturb-away]]);
- the global default colour (`globalSettings`), which only matters once
  something tints people who are in no set;
- **pseudonyms and display-name removal**, split out to
  [[viewer-contact-set-pseudonyms]] — they want a hook in the name cache
  so a renamed person is renamed everywhere, not in this panel alone.
