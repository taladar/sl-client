---
id: viewer-avatar-picker-multi-pick
title: Avatar picker — pick several residents at once
topic: viewer
status: ready
origin: user question (2026-08-21) while building viewer-minimap-menu-multi-avatar
blocked_by: []
refs:
  [
    viewer-conference-start-ui,
    viewer-contact-sets,
    viewer-people-lists-multi-select,
  ]
---

Context: [context/viewer.md](../context/viewer.md).

The shared avatar picker returns exactly one resident: it keeps
`selected: Option<usize>` and answers with `AvatarPicked { agent }`. The
reference's picker has a **multi-select mode** (`LLFloaterAvatarPicker` is
opened with an `allow_multiple` flag), and it is the natural front end for
every "several residents" action there is: starting a conference, inviting to
a group, and Add Resident… on a contact set.

Scope: an opt-in multi mode on the picker request (single stays the default,
so no existing caller changes), `Ctrl` / `Shift` selection in its results list,
and a reply carrying the whole list — with the single-pick reply expressed as
the one-element case rather than a second channel.

Deps in spirit rather than order: [[viewer-conference-start-ui]] is the first
caller that needs it (invite N to an ad-hoc conference), and the contact-set
panel's Add Resident… is the second ([[viewer-people-lists-multi-select]]).
Either can land first; whichever does should not grow its own picker.

Reference (Firestorm, read-only): `llfloateravatarpicker.{h,cpp}`
(`allow_multiple`, `getSelectedAvatarIds`), `floater_avatar_picker.xml`.
