---
id: viewer-add-friend-offers-silently
title: Add Friend sends the offer silently — no message dialog, no feedback
topic: viewer
status: done
origin: seen on aditi while live-checking [[viewer-conference-start-ui]]
  (2026-08-21)
refs: [viewer-social-profiles, viewer-notification-catalogue-friends-people,
  viewer-avatar-radar]
---

Context: [context/viewer.md](../context/viewer.md).

Clicking **Add Friend** in a resident's profile appears to do nothing: no
dialog asks for the accompanying message, and nothing afterwards says the offer
went out.

**The offer is actually sent.** Verified live on aditi: the two `sl-repl-tokio`
peers both logged
`instant_message_received(InstantMessage { .. dialog: FriendshipOffered .. })`
from the clicking avatar. So this is purely the missing UI around a working
command, not a dead button — but from the user's side the two are
indistinguishable, which is the whole complaint.

## What the reference does

`LLAvatarActions::requestFriendshipDialog` (`llavataractions.cpp:154`) raises
the **`AddFriendWithMessage`** notification — a text-input dialog pre-filled
with "Will you be my friend?" — and only sends `OfferFriendship` on OK, with
whatever the user typed. It also refuses self-friendship with `AddSelfFriend`
and adds the resident to Recent People.

Ours (`avatar_profile.rs`, `ProfileAction::AddFriend`) writes
`Command::OfferFriendship { to_agent_id: target, message: String::new() }`
immediately — no prompt, an always-empty message, and no confirmation line.

## Fix

1. Raise `AddFriendWithMessage` before sending. The notification is **already
   in the catalogue** (`notifications.rs`, `name: "AddFriendWithMessage"`), so
   this is wiring a text-input notification to the send, not authoring one
   ([[viewer-notification-catalogue-friends-people]] is where it landed).
2. Send the typed message as the offer's `message` (the recipient sees it in
   the offer).
3. Refuse the self case with `AddSelfFriend` rather than sending.
4. Say it happened — the reference's post-send feedback — so a working offer
   never again looks like a dead button.

**Every Add Friend entry, not just the profile's.** The same silent send is in
the radar's `"add-friend"` arm (`radar.rs`) and anywhere else
`Command::OfferFriendship` is written; the prompt belongs on the shared path so
they all gain it at once. A **multi**-selection should ask **once** and offer
to everyone, the way the multi-avatar menus already treat one action over a
list.

## How to verify

Live, with a second avatar that can accept: the dialog appears, the typed
message arrives with the offer, cancelling sends nothing, and clicking on
oneself is refused. The `sl-repl-tokio` peers do **not** accept friendships, so
they can confirm the offer *arrives* (as above) but not the accept half.

Reference (Firestorm, read-only): `llavataractions.cpp`
(`requestFriendshipDialog`, `callbackAddFriendWithMessage`),
`notifications.xml` (`AddFriendWithMessage`, `AddSelfFriend`).

## Done (2026-09-11)

Every Add Friend affordance now writes a **`RequestFriendship`**
(`sl-viewer-world-api`, beside `RequestBlock`) instead of the wire command, and
`sl_viewer_people::add_friend` is the one place that answers it: it refuses the
agent itself with `AddSelfFriend`, raises `AddFriendWithMessage`, sends
`Command::OfferFriendship` carrying what was typed, and confirms with
`FriendshipOffered`. Seven call sites moved onto it — the avatar / attachment
pie (`avatar_menu.rs`), the radar, the minimap, the profile floater, the
inspector popup, search, and a `secondlife:///…/requestfriend` link
(`slurl_dispatch.rs`).

`FriendshipOffered` was **not** in the catalogue (the coverage TSV had it as
`done` under [[viewer-dialog-offers-invites]], which handles the *incoming*
offers). It is ported now, with `notification-friendship-offered` in the English
bundle; the TSV row moved to `ported` under this task.

**One prompt at a time, and nothing dropped.** A `NotificationResponse` names
the template it answers, not the raise, so two live `AddFriendWithMessage`
dialogs would be indistinguishable and the first answer would offer friendship
to the second's residents. `FriendshipOfferQueue` therefore keeps one batch
outstanding and queues the rest — a click that arrives while a dialog is up
waits its turn rather than being discarded. Answering frees the slot and raises
the next dialog in the same frame (the send system runs before the ask).

**A multi-selection is one request.** The radar's per-agent loop (and its
already-a-friend filter) moved into the shared path: one dialog names the whole
selection, and the typed message goes to each. Two deviations from the
reference, which never offers to more than one resident: the dialog body and the
confirmation name several residents comma-joined, and `[NAME]` / `[TO_NAME]`
carry the shown label rather than an agent SLURL (a notification body is plain
text here, not linkified).

Not carried over: the reference also files the target under **Recent People**,
which this viewer has no model for — [[viewer-recent-people]] is that gap,
with the other ten sites the reference files from.

Tests (`add_friend.rs`): the ask happens before any send and the typed message
is what goes on the wire; Cancel and a button-less dismissal send nothing and
free the prompt; self-friendship raises the tip instead; a selection asks once,
offers each, and drops an existing friend; a second request waits for the first
dialog and is answered with its own message. The radar and avatar-pie tests now
assert the request rather than the command, and the pie is checked for sending
*nothing* on the wire itself.

Still live-unverified (needs a second avatar that can accept): that the typed
message arrives with the offer on the receiving side.
