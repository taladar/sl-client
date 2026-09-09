---
id: viewer-remove-attachment-message-is-a-no-op
title: '`Command::RemoveAttachment` sends a message no grid handles'
topic: viewer
status: bugs
origin: noticed detaching test fixtures via sl-repl while chasing
  viewer-prim-attachment-worn-but-not-rendered (2026-09-09)
refs: [viewer-hud-attachments-not-composited]
---

Context: [context/viewer.md](../context/viewer.md).

`Command::RemoveAttachment { attachment_point, item_id }` encodes the
`RemoveAttachment` LLUDP message (`Circuit::send_remove_attachment`). Nothing
receives it:

- **OpenSim has no handler.** `LLClientView` registers
  `DetachAttachmentIntoInv` and `ObjectDetach`; `RemoveAttachment` appears
  nowhere in the tree, so the packet is parsed and dropped.
- **The reference viewer never sends it.** Firestorm sends
  `DetachAttachmentIntoInv` (`llvoavatarself.cpp`, by item id) and
  `ObjectDetach` (`llagentwearables.cpp`, `llselectmgr.cpp`,
  `llviewerjointattachment.cpp`, `rlvlocks.cpp`, by local id). The
  `RemoveAttachment` name exists only in the prehash table.

Observed live on the local grid: four `remove_attachment` commands were sent and
acknowledged, no error was reported, and all four attachments were **still
worn** on the next login. The same four detached immediately with
`detach_attachment_into_inventory`.

**Not user-facing.** The viewer's own detach paths already send the right
things — `Command::DetachAttachmentIntoInventory` from the inventory actions and
`Command::DetachObjects` from the attachment pie — so only the `sl-repl` /
library surface can reach the dead command.

## What to decide

Either drop `Command::RemoveAttachment` (and its `sl-repl` entry) as a command
no server implements, or keep the message encoder for wire completeness — it is
a real message in `message_template.msg`, and the server direction may still
want to *parse* it — while removing the client-facing command that silently does
nothing. The rule this workspace already follows is that an error is never
hidden, and a command that cannot work is worse than one that fails.
