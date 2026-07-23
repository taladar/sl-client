---
id: viewer-auto-reject-offers
title: Auto-decline teleport/friendship/group-invite modes
topic: viewer
status: blocked
origin: debug-settings/chat-lines survey (2026-07-23)
blocked_by: [viewer-dialog-offers-invites]
refs: [viewer-do-not-disturb-away]
---

Context: [context/viewer.md](../context/viewer.md).

Standalone persistent modes (Comm ▸ Online Status; independent of DND)
that silently auto-decline classes of incoming offers:

- **Reject teleport offers and requests**
  (`FSRejectTeleportOffersMode`), optionally exempting friends
  (`FSDontRejectTeleportOffersFromFriends`), with a per-type canned
  response text (`FSRejectTeleportOffersResponse`).
- **Reject all friendship requests**
  (`FSRejectFriendshipRequestsMode` + response).
- **Reject all group invites** (`FSRejectAllGroupInvitesMode`).
- **Inventory item as autoresponse** (`FSAutoresponseItemUUID`): the
  autoresponse modes ([[viewer-do-not-disturb-away]] owns the reply-text
  machinery) can additionally send a configured inventory item to the
  sender.

Scope: the mode toggles + per-type response texts in settings and the
Comm ▸ Online Status menu, consumed by the inbound offer/invite dispatch
— decline silently, optionally send the canned reply, and suppress the
notification.

Reference (Firestorm, read-only): `World.SetRejectTeleportOffers`,
`World.SetRejectAllGroupInvites`, `World.SetRejectFriendshipRequests`
(`menu_viewer.xml` Comm ▸ Online Status), the `FSReject*` per-account
settings.

Builds on: the offer/invite dialog dispatch (blocked task) — the
auto-reject policy is a filter in front of those dialogs.
