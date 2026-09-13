---
id: viewer-notecard-embedded-item-opens
title: Opening an embedded item, instead of copying it
topic: viewer
status: ready
origin: user report (2026-09-13); split out of viewer-notecard-editor
refs: [viewer-notecard-editor, viewer-notecard-inline-items,
  viewer-inventory-double-click-actions, viewer-key-material-editor]
---

Context: [context/viewer.md](../context/viewer.md).

Clicking an embedded item in a notecard currently does one of three things: a
**calling card** opens the avatar's profile, a **texture** opens the texture
preview, and **everything else** is offered as a copy into inventory behind the
`ConfirmItemCopy` dialog. That last case is too wide. The reference opens
several types **in place**, without a copy — a landmark being the obvious one: a
notecard that hands out a location should not make you keep the landmark to see
where it points.

The reference's `LLViewerTextEditor::openEmbeddedItem` switches on the asset
type, and the types it does something *other* than copy for are:

| type | what it does |
| --- | --- |
| landmark | open it — the place profile, from which you teleport |
| texture / snapshot | open the texture preview (**done**) |
| calling card | open the avatar's profile (**done**) |
| sound | play it locally, no copy |
| animation | open the animation preview (play / stop) |
| notecard | open that notecard in its own window |
| everything else | copy into inventory, behind the confirmation (**done**) |

Note how this overlaps [[viewer-inventory-double-click-actions]]: the same
"what does this type open into" table, reached from a different surface. They
should share one dispatch rather than growing two, and whichever lands first
should be written as the shared one.

Two things the copy path must keep, because they are the reason it exists: a
type with no opener still offers the copy, and the copy stays behind the
confirmation — the reference never silently adds to your inventory.

Reference (Firestorm, read-only): `llviewertexteditor` (`openEmbeddedItem`,
`showCopyToInvDialog`), `llpreviewnotecard`.
