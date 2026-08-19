---
id: viewer-offer-accept-behavior-options
title: Post-accept inventory-offer behaviours
topic: viewer
status: ideas
origin: Firestorm full-parity audit (2026-08-19)
refs: [viewer-dialog-offers-invites, viewer-auto-reject-offers,
       viewer-inventory-context-actions]
---

Context: [context/viewer.md](../context/viewer.md).

What happens *after* an inventory offer is accepted — a settings family
our offer dialogs (done [[viewer-dialog-offers-invites]]) don't carry:
select/show the new item in the inventory panel (`ShowInInventory`),
auto-open accepted notecards/textures/landmarks (`ShowNewInventory`),
still show a notification when auto-accept is on
(`FSShowAutoAcceptInventoryInNotifications` — pairs the implemented
`AutoAcceptNewInventory` setting in `offers_invites.rs`), use the
legacy accept/decline message format
(`FSUseLegacyInventoryAcceptMessages`), open inventory after taking a
snapshot to inventory (`FSOpenInventoryAfterSnapshot`), and emit the
give-inventory particle effect
(`FSCreateGiveInventoryParticleEffect`).

Adjacent, from the privacy panel: show group invitations even for
groups the avatar already joined (`FSShowJoinedGroupInvitations`) —
the reject side of offer policy lives in [[viewer-auto-reject-offers]].
Each toggle is a small branch in the accept path of the offer handler
or the inventory panel ([[viewer-inventory-context-actions]] owns the
show-in-inventory plumbing).

Reference (Firestorm, read-only):
`indra/newview/skins/default/xui/en/panel_preferences_privacy.xml`,
`indra/newview/llviewermessage.cpp` (offer accept paths).
