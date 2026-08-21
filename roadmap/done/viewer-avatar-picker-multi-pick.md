---
id: viewer-avatar-picker-multi-pick
title: Avatar picker — pick several residents at once
topic: viewer
status: done
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

## Built (2026-08-21)

The picker's selection became a list, and the reply with it: `AvatarPicked`
carries `Vec<PickedAvatar>` — the agent plus the label its row was showing —
and a single-resident request answers with one element rather than through a
second channel (a consumer that only ever wants one reads
`AvatarPicked::first()`). The mode is chosen at the call site by
`OpenAvatarPicker::one` / `::many`, the same `one` / `many` idiom
`OpenAddToContactSet` uses, so nobody has to remember what a bare `bool` field
meant.

`Ctrl` / `Shift` in the results list are **not** a second selection algebra.
`TableState::apply_click`'s body moved out into a free
`ui_table::apply_selection_click(selected, anchor, index, multi, ctrl, shift)`
that both the widget and the picker's plain column call, so a modified click
means the same thing in a list that happens not to be a table. The picker keeps
the anchor the same way the radar does — carried across a refresh by *agent*,
because the Near Me tab re-sorts as people walk: whoever is still listed stays
picked at their new index, whoever left drops out (a selected row that is no
longer shown would confirm invisibly).

The mode is not a forward-looking flag with no callers: every existing
requester now says which it wants, matching the reference call by call.
**Many** — the three estate access lists (`llfloaterregioninfo.cpp`: "avatar
picker yes multi-select"), the parcel access lists, contact sets' Add
Resident… (`fspanelcontactsets.cpp`), the render-settings exceptions
(`fsfloateravatarrendersettings.cpp`), and inventory **Share**
(`llavataractions.cpp`'s `give_inventory`, which gives the same stash to each
of them). **One** — Block Resident (the reference's is deliberately
`allow_multiple = false`; blocking several at once belongs with the
multi-select block *list*, [[viewer-people-lists-multi-select]]) and the estate
kick / send-home pair, which are about one resident.

One deliberate divergence: the reference's *parcel* access panel is multi on
Add to the ban list and single on Add to the allowed list — the allowed path
was simply never updated when the ban path grew its multi-pick — and two
buttons side by side that answer a modified click differently is worse than the
divergence. Both are `many` here.

Then unblocked, since landed: [[viewer-conference-start-ui]] took the picker up
for its add-participants ✚ (a pane's answer becomes an ad-hoc conference).
Still to come: the multi-select People panel lists
([[viewer-people-lists-multi-select]]), which needs no picker of its own.

Tests (six, on the picker's pure state): the single mode never selects more
than one however the user clicks, the many mode toggles and ranges, a click
past the last row is ignored, the picks come back in row order with their
labels, a refresh carries the selection by agent and drops who left, and
replacing the rows clears the selection.
