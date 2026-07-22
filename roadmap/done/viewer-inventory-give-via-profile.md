---
id: viewer-inventory-give-via-profile
title: Give inventory by dropping onto a profile
topic: viewer
status: done
origin: split from viewer-inventory-context-actions (2026-07-21) — the drop
  target does not exist yet
blocked_by: [viewer-social-profiles]
refs: [viewer-inventory-context-actions]
---

Context: [context/viewer.md](../context/viewer.md).

Dropping a dragged inventory row onto an avatar's **profile floater** gives
it to them — the third give-by-drag target the reference supports. The other
two are live (`inventory_drag.rs`): an avatar in-world / their name tag, and
a People-list row (both resolve through `AgentDropTarget` /
`AvatarPickTarget`). Once [[viewer-social-profiles]] exists, its floater just
needs to carry `AgentDropTarget(agent)` on its root and the existing drop
resolution picks it up — this task is that one-component wiring plus a test.

## Shipped (2026-07-22)

With [[viewer-social-profiles]]: the profile floater's root carries
`AgentDropTarget(target)` for another avatar's profile (removed for one's
own), and the drop resolution's avatar-target lookup now walks **ancestors**
(`agent_target_at`, `inventory_drag.rs`) — the hovered node inside the
floater resolves up to the root's target. The 2nd Life tab shows the
reference's "Share: drop inventory items here" hint. Test:
`agent_drop_target_resolves_through_ancestors`.
