---
id: viewer-minimap-menu-avatar-actions
title: Minimap context menu — remaining avatar actions (More Options)
topic: viewer
status: blocked
origin: split from viewer-minimap-interactions (2026-07-23)
blocked_by: [viewer-block-list, viewer-report-abuse, viewer-derender-blacklist]
refs:
  [
    viewer-minimap-interactions,
    viewer-region-options-estate,
    viewer-avatar-moderation-actions,
    api-g17,
  ]
---

Context: [context/viewer.md](../context/viewer.md).

The minimap's avatar context menu ([[viewer-minimap-interactions]]) ships
More Options ▸ IM / Add Friend / Offer Teleport / Block, routed to the
shared avatar-action channels. The reference's remaining entries each
need a backend that does not exist yet; wire each one into the minimap
menu (and the other avatar-action surfaces — pie, people panel, profile)
as its backend lands, through the **same shared layer**, never a
per-menu reimplementation:

- Call (voice), Map (locate a mappable friend), Share (inventory give),
  Pay (money transfer to an avatar), Request Teleport, Teleport To
  (self-teleport to the avatar's position), Invite To Group, Get Script
  Info.
- Unblock (needs the mute-list mirror — [[viewer-block-list]]).
- Report ([[viewer-report-abuse]]).
- Freeze / Parcel Eject / Estate Kick / Teleport Home / Ban — all five
  are now [[viewer-avatar-moderation-actions]], which owns the shared
  per-avatar layer (the two visibility predicates and one guarded
  request channel per action) that this menu, the avatar pie and the
  radar's row menus all want. Freeze's receive side was [[api-g17]];
  the estate half waits on [[viewer-region-options-estate]].
- Derender / Derender + Blacklist ([[viewer-derender-blacklist]]).

Deps carry the partial order for the three with concrete task files;
the rest gate on their backends existing at all.
