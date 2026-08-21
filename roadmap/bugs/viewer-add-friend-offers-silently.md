---
id: viewer-add-friend-offers-silently
title: Add Friend sends the offer silently — no message dialog, no feedback
topic: viewer
status: bugs
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
