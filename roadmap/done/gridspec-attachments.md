---
id: gridspec-attachments
title: Attachments on each grid: attach, detach, limits, HUDs, temporary
topic: gridspec
status: done
origin: gridspec survey (2026-10-03)
refs: [gridspec-survey, viewer-remove-all-attachments,
  fake-grid-own-attachments-and-region-moves, server-world-sit-and-attach]
---

Context: [context/gridspec.md](../context/gridspec.md).

## Done (2026-10-20)

The `attachments` census (2av, live grids; three runs on aditi, four on
OpenSim, the last of each with all 403 answers held) wears copies of a cube
as A while B listens: a wear, an add and a replace on one point, an item worn
again and one nobody has, `ObjectDetach`, `DetachAttachmentIntoInv`, two at
once each way, a HUD, a drop, an `ObjectAttach` from the world, a probe
script's `llAttachToAvatar` and `llAttachToAvatarTemp`, one more than the
limit in one compound message, a teleport away and back, and a new login
(`book/src/gridspec/appearance.md` § Attachments). Findings:

- An attachment is a new object parented to the avatar with only an
  `AttachItemID` name-value and a nil `OwnerID` on both grids; Second Life
  gives other viewers `OBJECT_OWNER_MODIFY`. A temporary attachment's
  `AttachItemID` is its own id on both, as the viewer's
  `is_temp_attachment` assumes.
- An attach from the world or by a script, and a drop, re-send the same
  object under the same ids — nothing is killed. A drop removes the item.
- *Replace* takes one attachment off the point on Second Life, all of them
  on OpenSim (`WearReplacesAll`). Second Life alerts `RezAttemptFailed` for
  an unknown item; both ignore an item already worn.
- `ObjectDetach` restates the item (event queue on Second Life, UDP on
  OpenSim); `DetachAttachmentIntoInv` restates nothing.
- A HUD reaches its wearer only, but Second Life sends others its kill.
- The limit is 38 on both; the 39th is refused silently.
- Across a teleport an attachment keeps its full id under a new local id;
  Second Life kills the avatar, never its attachments, for the agent and for
  the viewers left behind. Both grids re-wear the outfit at a new login.

Fake grid: large, unchanged —
[[fake-grid-own-attachments-and-region-moves]] now carries the measured
model, and takes the census offline when it lands.

Viewer: the multi-wear (`RezMultipleAttachmentsFromInv`) went out as one
packet of every item, where the reference packs four to a packet under one
compound id; an outfit of a few dozen attachments would not fit a datagram.
It packs four now (`sl-proto` circuit, held by a lifecycle test). The viewer
already reads "worn" off the attachment objects that arrived, keys them by
region and local id, and drops an avatar's attachments with it (unit-tested
in `sl-viewer-world-api`'s object graph), which is what a Second Life
teleport needs; neither grid needs a login re-wear from the COF. A live e2e
stage holds it end to end on both grids (`e2e_objects`,
`a_worn_prim_follows_the_avatar_on_a_live_grid`): a prim rezzed, taken and
worn with Add from the inventory window is drawn on the avatar once and
listed on the Worn tab, stays so — same full id, new local id — across a
teleport away and back, and goes from both when taken off.

The census ends by walking A back to the build location: its teleport and
relogin left A at Mauve's telehub, where the next live stage logging in at
`last` could rez nothing.

## Known already

OpenSim stamps `AttachItemID` and kills on detach; `RemoveAttachment` ignored on
live grids; the fake grid drops every attach / detach message.

## Discover

`attach_detach` on aditi plus multi-attach, HUD (second avatar must not see it),
drop, temp attach (scripted object), limits.

## Document

`book/src/gridspec/appearance.md` § Attachments.

## Fake grid

Large — [[fake-grid-own-attachments-and-region-moves]] and
[[server-world-sit-and-attach]].

## Viewer

Worn only after arrival; new local id per region.
