---
id: viewer-rlv-send-side-consumers
title: RLV — make the viewer's send paths ask the RlvActions façade
topic: viewer
status: ready
origin: found while wiring viewer-menu-touch-object (2026-09-27)
refs: [viewer-rlv-enforce-send-side, viewer-menu-touch-object]
---

Context: [context/viewer.md](../context/viewer.md).

[[viewer-rlv-enforce-send-side]] built `sl_rlv::RlvActions`, the reference's
`RlvActions` façade. It answers `can_touch`, `can_interact`, `can_edit`,
`outgoing_chat`, the teleport, sit, economy and IM checks, and more. Its own
"Verified" section closes with the gap this task is for: *still no consumer in
the workspace*. On 2026-09-27 that is still true. Outside `sl-rlv`, nothing
calls `RlvActions`, and no viewer crate tests an `RlvBehaviour::Touch*`,
`Interact`, `Edit` or `Sendchat` directly either.

So an RLV restriction a worn object sets is parsed and stored, and never
honoured on the way out. `@touchall`, `@fartouch`, `@edit`, `@sendchat`,
`@tplm` and the rest change nothing the user can do. The reference asks at
every choke point, for example `enable_attachment_touch` and
`handle_object_touch`, which both call `RlvActions::canTouch`.

What this needs:

- One `RlvActionSource` implementation over the viewer's world:
  `ObjectState` for link roots and attachment kinds, `AvatarState` for
  positions, the seat, and the open IM sessions.
- Every send path asks the façade before it writes its `SlCommand`, and the
  matching menu entry greys out when the façade says no. The paths are touch
  (a click, the object pie, the attachment pie, and the inventory's worn-object
  Touch from [[viewer-menu-touch-object]]), edit, rez, sit, teleport, chat and
  IM, pay and buy.
- A test per path that a held restriction refuses the command and greys the
  entry.

The façade puts each check at a choke point so there is one answer. The
wiring should keep that: one gate per command family, not a copy of the check
at every call site.
