---
id: viewer-contact-set-presence-extras
title: Contact sets — per-set autoresponse and notify settings
topic: viewer
status: done
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

## Built

A contact set now carries three behaviours beside its colour, each stored in
the reference's own field of the same `contact_sets.json` entry (`notify`,
`sort_by_online_status`, and the three `autoresponse_*_enabled` /
`autoresponse_*` pairs), so a set configured in either viewer means the same
thing in the other. Every field reads `#[serde(default)]`: a set written before
this task, or transcribed by hand, is all-off rather than a failed read.

**Per-set replies.** `ContactSets::autoresponse_for` answers with the smallest
set the sender is filed under that carries a reply for that mode, ties broken by
name. That rule is not what this task file guessed — the reference does **not**
take the first matching set; `getAutoresponseForFriend` tracks `best_set_size`
and takes the smallest, exactly as `getFriendColor` does. Matching it is also
the answer that reads right, and the one our `color_of` already gives: the set
with three people in it says more about someone than the set with eighty.
`presence::reply_text` consults it before the settings store, so the whole hook
is the reference's `getAutoresponseTextForAvatar` shape — per-set text, else the
global one. Only the three *mode* replies have the layer: the away and blocked
replies are statements about the user rather than about the sender, and the
reference gives them none either. An override that is switched off, or on but
blank, is not an override — the global reply stands rather than the sender
hearing nothing.

**Per-set notify.** `ContactSets::notifies` is the reference's `notifyForFriend`
— true when *any* set the resident is in asks for it — and the friend online /
offline toast reads it as the second way a notice can be enabled, beside the
global `ChatOnlineNotification`. The reference's two-part gate needs its own
master switch, so the new account setting `ContactSetsNotificationToast`
(default off, the reference's default) joins the alerts tab under the friends
row: with the global toggle off, a friend in a notifying set is still announced.
The decision is therefore per friend rather than one early return.

**Sort by online status.** The panel's member sort takes the flag as a leading
key when a real set is chosen, online first, falling through to the table's own
column keys within each group — the reference's
`FSAvatarItemOnlineStatusComparator`,
which likewise falls back to the name. `FriendsModel::is_online` is the presence
it reads: the buddy cache is the only presence the protocol gives us, so a
member who is not a friend sorts with the offline.

**The UI.** The set-settings floater grew the reference's five checkboxes and
its three reply editors — the two behaviour toggles, then one
toggle-over-multiline-field block per mode. The reply fields commit on losing
focus and on the floater turning to another set (the reference commits on focus
lost too), and the toggle carries the field's current text with it, so switching
an override on answers with what is already typed rather than with nothing.
Everything still goes through `RequestContactSet`, so the model's guards remain
the one way in.

**Greyed buttons** (asked for while reviewing the above). Every one of the
panel's twelve action buttons is now greyed and inert whenever it does not apply
— *Configure…*, *Delete Set* and *Add Resident…* on a pseudo-set (there is no
*All Sets* to configure), the member actions with nobody selected, *Rem Alias…*
with nobody aliased, *Rem DN…* with the display name already suppressed. One
predicate, `ContactSetsButton::is_enabled`, is read by both the greying pass and
the press handler, so the look and the behaviour cannot drift;
`InteractionDisabled` rides along as the state marker, but the press handler
asks the predicate rather than trusting it (the marker is advisory for a
hand-rolled button). The greying itself is the **skin's**, not a constant of
ours: a new `--control-bg-disabled` token in both skins, and the
`.sk-disabled-surface` / `.sk-disabled-text` rules in `common.css` layered over
the base `.sk-button` / `.sk-text` a button and its label now carry — two rules
because bevy_ui has no style inheritance, and a base class underneath because
dropping the disabled one has to fall back to *something*. Adopting those base
classes also moves this feature's buttons (the panel's twelve and the two
floaters' five) onto the shared button look the rest of the viewer uses.

That also resolves the note [[viewer-contact-set-pseudonyms]] left behind:
greying its three alias buttons "would read as a bug" only while the other nine
stayed inert-but-lit, and now none of them do.

Found while live-checking this and **not** fixed here: the set chooser stopped
dropping down once, after Pseudonyms was picked, and did not reproduce on the
next run — filed as [[viewer-combo-stops-opening]] with the four candidate
causes it rules in or out, and `ui_combo`'s open/close decision now logs at
`debug!` so the next occurrence names its own cause.

Deliberately not built, and not silently dropped: the **nearby-chat** half of
the per-set notice (`FSContactSetsNotificationNearbyChat`). Its global sibling
`OnlineOfflinetoNearbyChat` — the "friend online/offline to nearby chat"
emitter — is owned by [[viewer-generated-chat-notices]], and the per-set line is
one call to `ContactSets::notifies` once that emitter exists; adding the per-set
half first would give it nothing to gate.

Unit-verified: the button-enablement rules for all twelve, the smallest-set
reply rule with the off / blank cases, the notify *or* across a resident's sets,
the round trip through the reference's field names (and the bare older entry),
the online-first sort, and that `reply_text` answers from a set with no settings
store at all. The interactive checks — the settings floater's five checkboxes
and three reply editors, their persistence across a relog, and the online-first
ordering — are outstanding, and the **per-set reply actually being sent** shares
the two-avatar live check still outstanding from [[viewer-do-not-disturb-away]].
