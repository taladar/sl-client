---
id: viewer-snapshot-postcard
title: Snapshot destination — postcard / e-mail
topic: viewer
status: ready
origin: split from viewer-snapshot-floater (2026-07) — disk shipped first
blocked_by: [viewer-snapshot-floater]
refs: [api-g16, viewer-photo-hosting-upload, viewer-snapshot-to-inventory]
---

Context: [context/viewer.md](../context/viewer.md).

Add the **postcard / e-mail** destination to the snapshot floater
([[viewer-snapshot-floater]]): send the captured snapshot to an e-mail address
with a subject, a message and the sender's name, the reference viewer's
`Snapshot → Email` panel. The floater already owns the framed preview, the
resolution / format controls and the captured image; this task is the
destination panel plus the send.

The **send path already exists**: `Command::SendPostcard` (the `SendPostcard`
UDP message, Low 412) landed with [[api-g16]], along with the decoded
`Postcard` type. So the work here is UI, not protocol: a to-address / from-name
/ subject / message form, feeding the current capture as the postcard image, the
usual field validation, and surfacing the send result. Note the reference caps
the postcard image to a smaller JPEG than a free disk save, so the resolution
picker constrains (or re-encodes) for this destination the way the
inventory-texture path ([[viewer-snapshot-to-inventory]]) constrains for its
own.

External photo-sharing sites are a **different** destination with their own auth
story — that is [[viewer-photo-hosting-upload]], not this task.

Reference (Firestorm, read-only): `panel_snapshot_postcard.xml`,
`llpanelsnapshotpostcard`, `llfloatersnapshot`.

Builds on: [[viewer-snapshot-floater]] (the floater and the captured image),
[[api-g16]] (`Command::SendPostcard`).
