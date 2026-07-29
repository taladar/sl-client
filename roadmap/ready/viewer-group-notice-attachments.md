---
id: viewer-group-notice-attachments
title: Group notices — attach on send, open on receive
topic: viewer
status: ready
origin: user request (2026-07-27), while implementing viewer-social-group-profile
refs: [viewer-social-group-profile, viewer-group-notice-display]
---

Context: [context/viewer.md](../context/viewer.md).

The group profile floater's Notices tab ([[viewer-social-group-profile]])
composes and sends notices and shows a received notice's body, but treats
**attachments** minimally: sending never attaches an item, and a received
notice's attachment is only flagged (`group-notice-has-attachment`), not
openable. The received-notice toast ([[viewer-group-notice-display]]) now
**shows** an attached item (icon + name) but likewise does not yet copy it into
inventory. Complete the attachment surface:

- **Send with an attachment.** Let the compose area accept an inventory item
  (an inventory-item drag/drop onto the compose area, or an item picker) and
  pass it as `GroupNoticeAttachment { item_id, owner_id }` to
  `Command::SendGroupNotice`. The item must be copy+transfer for the grid to
  accept it.
- **Open a received attachment.** The notice arrives as an
  `InstantMessageReceived` with the `GroupNotice` dialog; its `binary_bucket`
  carries the attachment descriptor — already decoded by
  `InstantMessage::group_notice()` and shown (icon + name) on the
  received-notice toast ([[viewer-group-notice-display]]). What remains is the
  **Accept** that copies the item into inventory (the reference's
  notice-attachment accept, an `IM_GROUP_NOTICE_INVENTORY_ACCEPTED` with the
  target folder in the bucket), reusing the inventory-offer accept path, wired
  on both the toast and the Notices tab.

The wire command already carries the optional attachment; this is the viewer
UI + the received-bucket decode/accept.

**Acknowledgement gating.** Closing a group-notice toast without accepting an
attachment should **decline** it (`IM_GROUP_NOTICE_INVENTORY_DECLINED`, the
reference `IOR_DECLINE` on close). That server message — and any accept — must
fire **only for a freshly-received notice with a live inventory offer**, never
for a notice re-opened from history or viewed in the Notices tab (whose offer no
longer exists on the server). The display task ([[viewer-group-notice-display]])
deliberately emits **no** `SlCommand` on close, so wire the decline here keyed
on the presence of a live offer, not merely on the card closing.
