---
id: viewer-remove-attachment-message-is-a-no-op
title: '`Command::RemoveAttachment` sends a message no grid handles'
topic: viewer
status: done
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

## Why nothing receives it: it is a sim → dataserver message

The template settles the question the live run only hinted at.
`RemoveAttachment` (Low 332) sits directly below `UpdateAttachment` (Low 331),
and the two share a comment style that names the direction:

```text
// Simulator informs Dataserver of new attachment or attachment asset update
// DO NOT ALLOW THIS FROM THE VIEWER
{ UpdateAttachment Low 331 Trusted Zerocoded …

// Simulator informs Dataserver that attachment has been taken off
{ RemoveAttachment Low 332 NotTrusted Unencoded …
```

So it never travelled viewer → simulator at all: it is the *pair* to
`UpdateAttachment`, the simulator telling the asset/inventory side that a worn
item came off. That explains every observation at once — OpenSim has no handler
because a simulator is the **sender**, and the reference viewer never sends it
because a viewer is neither end. The `NotTrusted` flag is the only thing that
made it look viewer-sendable, and a flag that merely fails to forbid something
is not a direction.

## Resolution: removed from the client, and from the simulator's decode

A command that cannot work is worse than one that fails, so the whole path is
gone rather than left to be rediscovered:

- `Command::RemoveAttachment`, `Session::remove_attachment` and
  `Circuit::send_remove_attachment` — the viewer-side encode chain.
- The `remove_attachment` `sl-repl` command and its `format.rs` name, plus the
  `sl-client-tokio` / `sl-client-bevy` dispatch arms.
- `ServerEvent::RemoveAttachment` and the `SimSession` decode arm. Keeping the
  parse was the tempting half-measure, but its doc comment claimed "the client
  took off a worn item by inventory id" — a client action that cannot happen. A
  simulator that ignores this from a client is behaving exactly like OpenSim.
- The two tests that only covered the removed code (`lifecycle.rs`'s
  `remove_attachment_encodes_item_and_point`, `sim_session.rs`'s
  `client_remove_attachment_reaches_simulator`).

`AnyMessage::RemoveAttachment` stays: `sl-wire` mirrors the whole template, and
that is the one place where wire completeness is the point.

The finding is written down where someone would go looking for the missing
encoder — the doc comment on `Circuit::send_detach_attachment_into_inv`, which
is the by-item-id detach a viewer *may* send — and in
[`book/src/content/attachments.md`](../../book/src/content/attachments.md),
which now has a section on why this message is not a viewer message.

## Verification

The `sl-repl` table-parity tests
(`every_printable_name_is_a_command_the_registry_parses` and its siblings) cover
the command surface after the removal, and the sl-proto / sl-repl /
sl-client-tokio suites pass (1259 tests). No live run: the removal
takes away the only way to send the message, and the live evidence that it does
nothing was already gathered on the local grid before the fix.
