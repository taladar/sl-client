---
id: viewer-give-calling-card
title: Give Calling Card from the avatar menus
topic: viewer
status: ready
origin: Firestorm full-parity audit (2026-08-19)
refs: [viewer-avatar-context-menu, viewer-attachment-context-menu,
  missing-out-batch-1, test-calling-card]
---

Context: [context/viewer.md](../context/viewer.md).

The reference offers **Give Calling Card** on the other-avatar and
attachment-other menus (`Avatar.GiveCard`; More ▸ Give Card in the pie
variants). The protocol side is finished: the calling-card offer/accept
exchange landed in [[missing-out-batch-1]] and is live-verified by
[[test-calling-card]] (plus its aditi variant). But no viewer surface
sends the offer — our `give-card` slices in
`sl-client-bevy-viewer/src/avatar_menu.rs` and `attachment_menu.rs` are
UNIMPLEMENTED placeholders, and no roadmap task tracked the verb.

Scope: wire the two pie slices (avatar-other and attachment-other route
through the shared avatar handler) to the calling-card offer command,
and add the People-panel / profile entry points if they fall out
trivially from the same action. On the receiving side the standard offer
toast applies — the notification-catalogue entries for calling-card
offers already exist.

Reference (Firestorm, read-only):
`indra/newview/skins/default/xui/en/menu_avatar_other.xml`,
`menu_pie_avatar_other.xml`; `indra/newview/llviewermenu.cpp`
(`Avatar.GiveCard`).
