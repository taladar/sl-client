---
id: viewer-group-notice-attachments
title: Group notices — attach on send, open on receive
topic: viewer
status: ready
origin: user request (2026-07-27), while implementing viewer-social-group-profile
refs: [viewer-social-group-profile]
---

Context: [context/viewer.md](../context/viewer.md).

The group profile floater's Notices tab ([[viewer-social-group-profile]])
composes and sends notices and shows a received notice's body, but treats
**attachments** minimally: sending never attaches an item, and a received
notice's attachment is only flagged (`group-notice-has-attachment`), not
openable. Complete the attachment surface:

- **Send with an attachment.** Let the compose area accept an inventory item
  (an inventory-item drag/drop onto the compose area, or an item picker) and
  pass it as `GroupNoticeAttachment { item_id, owner_id }` to
  `Command::SendGroupNotice`. The item must be copy+transfer for the grid to
  accept it.
- **Open a received attachment.** The notice body arrives as an
  `InstantMessageReceived` with the `GroupNotice` dialog; its `binary_bucket`
  carries the attachment descriptor. Decode it and offer an **Accept** that
  copies the item into inventory (the reference's notice-attachment accept),
  reusing the inventory-offer accept path.

The wire command already carries the optional attachment; this is the viewer
UI + the received-bucket decode/accept.
