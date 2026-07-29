---
id: viewer-dialog-offers-invites
title: Inventory / teleport offers + friendship / group invites
topic: viewer
status: done
origin: reference-viewer feature-cluster survey (2026-07); split from viewer-notifications-dialogs
blocked_by: [viewer-ui-notification-host]
---

Context: [context/viewer.md](../context/viewer.md).

The accept / decline dialogs the grid throws at the user: **inventory offers**,
**teleport offers / lures**, **friendship** and **group invites**, and **group
notices** (with their attachments). Each is a toast on the notification host
([[viewer-ui-notification-host]]) with accept / decline (and discard / mute)
buttons wired to the existing protocol replies.

Much of the underlying protocol — teleport offers, inventory offers, friendship
and group invites — is already handled; this task is the remaining dialog panels
and their accept / decline wiring on top of the notification host.

Reference (Firestorm, read-only): `llnotificationmanager`,
`lltoast*`, `llnotification*handler`.

Builds on: the teleport-offer, inventory-offer and invite protocol already done.

## Done

New viewer module `src/offers_invites.rs` (`OffersInvitesPlugin`), a sibling of
the script-dialog / load-url / script-permission toast hosts. It consumes
`Event::InstantMessageReceived` and, for each of the four offer / invite
dialogs, raises a **sticky** `Alert` card into the shared notification host,
each with its accent / glyph so the kinds read apart:

- **inventory offer** (`InventoryOffered` / `TaskInventoryOffered`): "{giver}
  has given you an item" + the item name, with **Accept** (file into the
  type-appropriate system folder — `default_folder_type`, agent root when
  absent), **Decline** (route to Trash) and **Block** (mute the giver +
  decline);
- **teleport offer / lure** (`LureUser`): "{offerer} has offered to teleport
  you" + the message, with **Teleport** / **Decline**;
- **friendship offer** (`FriendshipOffered`): "{agent} is offering to be your
  friend" + any custom message, with **Accept** (file the calling card) /
  **Decline**;
- **group invitation** (`GroupInvitation`): "{inviter} has invited you to join a
  group" + the message + any membership fee, with **Join** / **Decline**.

Each button writes the offer's existing protocol reply
(`AcceptInventoryOffer` / `DeclineInventoryOffer`, `AcceptTeleportLure` /
`DeclineTeleportLure`, `AcceptFriendship` / `DeclineFriendship`, and the new
`AcceptGroupInvitation` / `DeclineGroupInvitation`); the close **×** declines
conservatively (never a silent accept, never a dangling offer). The two replies
that need a destination folder resolve it from the live inventory **at click
time**. Not persisted across a relog — an offline-stored offer re-arrives as a
fresh offline IM at login. All card text is in `en/main.ftl`; four gallery
specimens are registered in `ELEMENTS` and swept by `ui_test`.

**Group-membership invitations needed protocol support** (the one addition
beyond viewer wiring): the reference group-invitation reply was not yet
modelled. Added to `sl-proto`: the two `ImDialog` bytes
(`IM_GROUP_INVITATION_ACCEPT` / `_DECLINE`, 35 / 36),
`InstantMessage::group_invitation()` decoding the reference 20-byte
`invite_bucket_t` (big-endian S32 fee + 16-byte role id; group id = the
invitation IM's sender, transaction id = its `id`) into
`GroupInvitationReceived`, `Session::accept_group_invitation` /
`decline_group_invitation` (the online UDP `send_join_group_response` IM path),
and — for an invitation that arrived while **offline** (a null session id) —
the `AcceptGroupInvite` / `DeclineGroupInvite` capability path: two new cap
constants (added to `REQUESTED_CAPABILITIES`) and a `group_invite_response_body`
(`{ "group": <uuid> }`) POSTed fire-and-forget. `Command::AcceptGroupInvitation`
/ `DeclineGroupInvitation` carry a `use_offline_cap` flag (set from
`id.is_nil() && offline`) that selects the path, wired through both the Bevy and
tokio dispatchers. Five unit tests (four decode: fee/role, OpenSim's zeroed
bucket, truncated bucket, wrong dialog; one for the cap body).

**Out of scope, deliberately:** group **notices** (listed in the task text) are
already their own done task ([[viewer-group-notice-display]]), so this module
does not touch them. Auto-decline modes are [[viewer-auto-reject-offers]]
(unblocked by this task); anti-spam throttling of these offers is
[[viewer-anti-spam-filter]].
