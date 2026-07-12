---
id: api-g1
title: Attachments
topic: api
status: done
origin: SL_API_ROAD_MAP.md
---

Context: [context/api.md](../context/api.md).

## G1 — Attachments

Rez/wear/detach rigid and scripted attachments (distinct from the existing
clothing/body `SetWearing` path). Messages: `RezSingleAttachmentFromInv`,
`RezMultipleAttachmentsFromInv`, `ObjectAttach`, `ObjectDetach`, `ObjectDrop`,
`RemoveAttachment`, `UpdateAttachment`. New `AttachmentPoint` type. Server:
surface attach/detach as `ServerEvent`. OpenSim-testable.

- [x] G1 attachments end-to-end (client, server, both runtimes, REPL, tests,
  book). New `AttachmentPoint` enum + `RezAttachment` type; commands
  `AttachObject`, `DetachObjects`, `DropAttachments`, `RemoveAttachment`,
  `RezAttachment`, `RezAttachments` (`ObjectAttach`/`ObjectDetach`/`ObjectDrop`/
  `RemoveAttachment`/`RezSingleAttachmentFromInv`/
  `RezMultipleAttachmentsFromInv`);
  matching `ServerEvent`s decode each on the simulator side. `UpdateAttachment`
  (Low 331) is intentionally **not** wrapped: it is a `Trusted` simulator→
  dataserver message ("DO NOT ALLOW THIS FROM THE VIEWER"), so it has no
  viewer- or `SimSession`-facing role; it remains reachable as a raw
  `AnyMessage` if ever needed.
