---
id: viewer-auto-reject-offers
title: Auto-decline teleport/friendship/group-invite modes
topic: viewer
status: done
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

## Parity-audit addendum (2026-08-19)

Parity-audit extension: an auto-ignore mode for ad-hoc/conference chat
sessions (`FSIgnoreAdHocSessions` — silently leave/never open incoming
conference sessions), with a friends exemption
(`FSDontIgnoreAdHocFromFriends`); the optional nearby-chat report line
for an ignored session (`FSReportIgnoredAdHocSession`) is already part
of [[viewer-generated-chat-notices]]. Also from the privacy panel: a
toggle to still show group invitations for groups the avatar has
already joined (`FSShowJoinedGroupInvitations`) — post-accept offer
behaviour otherwise lives in [[viewer-offer-accept-behavior-options]].

## Built

New `auto_reject.rs` in the viewer owns the policy; the offers host, the
conversations ingest and the presence replies consume it.

- **The decision is one pure function.** `reject_for` takes the five mode
  flags and two facts about the offer — is the sender a friend, is the
  inviting group one we are already in — and answers with the `RejectKind`
  that swallowed it, or `None`. The offers host
  ([`offers_invites.rs`](../../sl-viewer-people/src/offers_invites.rs))
  runs it in front of every card, ahead of the Do Not Disturb deferral:
  an offer being answered automatically has nothing to defer.
- **A rejection answers, it does not just drop.** The mode's canned reply
  goes out as a `DoNotDisturbAutoResponse` IM (the same envelope the
  presence replies use, so the sender's viewer marks it automatic), and
  then the offer is **declined on the wire** — a deliberate departure from
  the reference, which sends the reply and lets the offer rot pending on
  the simulator. A teleport *request* is the one case with nothing to
  decline: it carries no lure, so the reply is the whole answer. A blank
  reply text means "reject them, but say nothing".
- **Group invitations.** Rejected silently (no canned reply — the
  invitation comes from the *group*, and the reference answers neither),
  and the same suppression covers an invitation to a group the agent is
  already a member of, unless `ShowJoinedGroupInvitations` asks for those.
- **Ad-hoc conferences.** The ignore mode is applied where the invitation
  is ingested (`conversations.rs`): `DeclineChatInvite` goes out and no tab
  is opened, so the session never appears. Only conferences — a group IM
  invitation is a group the user chose to be in. The optional nearby-chat
  report line for an ignored session stays with
  [[viewer-generated-chat-notices]], which owns that family.
- **The autoresponse item.** A mode reply can carry an inventory item with
  it (`AutoresponseItemUUID`), given to the sender right after the text and
  noted in the conversation. The blocked-sender reply never sends it —
  that reply exists to tell someone they are blocked, not to hand them a
  present. The item is chosen from its own inventory context menu (**Send
  with Autoresponses**, gated on copy+transfer, since an autoresponse gives
  it away again and again — the same gate as the reference's drop target),
  and the chat preferences tab shows and clears the stored id. The
  reference's drag-and-drop preferences target is not ported; the context
  menu is this UI's way of naming an item.
- **The UI.** Comm ▸ Online Status grew the three mode check items in the
  reference's order and wording, each raising the reference's mode-set
  notification on the rising edge (the catalogue entries already existed).
  Preferences ▸ Chat gained an *Automatic rejection* section: the three
  toggles again, the two canned replies, the two friends exemptions, the
  joined-group and ad-hoc suppressions.

Not carried:

- `RejectTeleportOffersModeWarning` — the reference raises it when the user
  tries to *send* a teleport request while their own reject mode is on. The
  viewer has no "request a teleport from someone" action yet (only Offer
  Teleport), so there is no call site; the catalogue entry is already there
  for when one lands.

Unit-verified (the reject decision, the friends exemptions, the wire
decline each rejection sends, the offer-dialog classification). The live
two-avatar check — a real lure / friendship offer / group invite arriving
with each mode on — is still outstanding.
