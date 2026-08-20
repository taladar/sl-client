---
id: viewer-contact-set-presence-extras
title: Contact sets — per-set autoresponse and notify settings
topic: viewer
status: ready
origin: split from [[viewer-contact-sets]] once its presence consumer landed
  (2026-08-20)
refs: [viewer-contact-sets, viewer-do-not-disturb-away]
---

Context: [context/viewer.md](../context/viewer.md).

[[viewer-contact-sets]] deliberately left the reference's per-set *behaviour*
settings unbuilt, because nothing consumed them yet. Two of the three consumers
now exist, so the settings can be carried:

- **Per-set autoresponse overrides.** The reference stores three reply texts
  per set (`ContactSetAutoresponseMode`: `BUSY`, `AUTORESPONSE`,
  `AUTORESPONSE_NONFRIENDS`) and `LGGContactSets::getAutoresponseForFriend`
  consults them *before* the global reply, so "my partner gets a different
  Unavailable message" works.
  Ours has the global replies and the mode precedence
  ([[viewer-do-not-disturb-away]]'s `reply_for`) but no per-set layer: the
  hook is one lookup in `presence::reply_text`, keyed on the sender's sets,
  plus the per-set fields in the contact-sets file and the panel rows to edit
  them. A resident in several sets needs a rule (the reference takes the first
  matching set in its own order).
- **Per-set notify / sort-by-online-status.** "Tell me when anyone in this set
  comes online" wants the friends-online notice path; "sort this set by online
  status" is a contact-sets panel ordering knob.

Still out of scope for the same "no consumer" reason: the global default colour
(`globalSettings`), which only matters once something tints residents who are in
no set at all.

Reference (Firestorm, read-only): `lggcontactsets.cpp` (the per-set map, the
autoresponse lookup), `panel_contact_sets.xml`.
