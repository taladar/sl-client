---
id: viewer-notecard-editor
title: Notecard viewer & editor (rich text with embedded items)
topic: viewer
status: in-progress
origin: user request (2026-07)
refs: [viewer-lsl-editor-widget, viewer-notecard-format,
  viewer-inventory-folder-tree, viewer-url-linkification,
  viewer-task-inventory-open-and-save-back]
---

Context: [context/viewer.md](../context/viewer.md).

Open, read, edit and save notecards. Easy to mistake for "a text box", which is
why it is worth stating plainly: **a notecard is not plain text.** The asset is
Linden text carrying **embedded inventory items** — drop a landmark, an object
or another notecard into the body and it sits *inline* in the text as a
clickable item — and the viewer linkifies URLs and SLURLs in the prose around
them. Notecards are how SL ships instructions, landmark packs and freebies, so
this is a load-bearing reader, not a nicety.

Present already: `AssetType::Notecard`, the `UpdateNotecardAgentInventory` cap,
asset fetch and the create/update flow (conformance case
`test-notecard-create-update`). The format decode itself lives in the pure
`sl-notecard` crate ([[viewer-notecard-format]]) — a format decoder has no
business living in a widget. Still to add on the protocol side:
**`UpdateNotecardTaskInventory`** — we have the *agent-inventory* cap but not
the one for a notecard living **inside a prim**, which is exactly where most
read-me
notecards are.

## The editor: the same widget problem, one step harder

The LSL editor widget ([[viewer-lsl-editor-widget]]) already has to solve "Bevy
0.19's editable text is `parley::PlainEditor`, so it takes **one style for the
whole buffer** and cannot colour a range". A notecard needs that *and*
**non-text objects inline in the flow** (the item icons).

The good news is that parley already models this: it has **inline boxes**
precisely for embedding arbitrary boxes in a text layout. So the fork that gives
the script editor per-range brushes is the same fork that gives the notecard
editor inline items — **one rich-text widget, two consumers.** That is why
[[viewer-lsl-editor-widget]] is a hard prerequisite: "per-range colour" and
"inline boxes plus per-range colour" are different designs, and the widget must
be built knowing the second is coming.

Also needed: dropping an inventory item into the body (drag-and-drop from the
inventory tree, [[viewer-inventory-folder-tree]]), clicking an embedded item to
open/wear/save it, clickable URLs and SLURLs in the text
([[viewer-url-linkification]]), and the usual save/permissions path. Note the
embedded items carry their own permissions — copying a notecard copies its
contents, so the item-permission rules matter and should not be quietly ignored.

Reference (Firestorm, read-only): `llpreviewnotecard`, `llviewertexteditor` (the
embedded-item machinery — items are represented as private-use characters in the
text and resolved through an embedded-item table), `llfloaternotecard`.

## Implemented so far (2026-07-27)

The editor's non-rich-text half is built; the rich, inline part waits on the
rich-text widget ([[viewer-lsl-editor-widget]]), so this stays in progress.

Done:

- **`sl-notecard`** gained `Notecard::with_edited_text` — a pure, unit-tested
  reconciliation of the embedded-item table against an edited body: it drops the
  items whose private-use marker the resident deleted, renumbers the survivors
  by order of first appearance, and clones an item whose marker was
  copy-pasted (the reference's copy-on-paste), so editing prose can never
  corrupt or orphan an embedded item on save.
- **`sl-client-bevy-viewer/src/edit_notecard.rs`** — a dedicated floater
  (`EditNotecardPlugin`) opened by the inventory **Open** action (routed here
  from `inventory_properties`, replacing the old read-only notecard preview) and
  by **double-clicking a notecard in the Object Contents floater / Content tab**
  (the reference's `openItem`). It fetches and decodes the asset, and shows it
  **read-only** — a note, a non-editable text block, no Save — when it is not
  modifiable, or **editable** (a multi-line field) otherwise. A
  [`NotecardSource`] carries the "opened-from-task" provenance so Save writes
  back to the right place. Embedded items are **listed** below the body (icon +
  name + type). A gallery specimen (`notecard-editor`) sweeps the layout
  headlessly; Fluent keys in all four locales.
- **Save back — agent and task.** Agent-inventory notecards save over
  `UpdateNotecardAgentInventory`; task-inventory (in-prim) notecards save over
  the new `UpdateNotecardTaskInventory` cap (**Save Back to Object**), with a
  saving / saved / failed status line. On the protocol side this added the
  `CAP_UPDATE_NOTECARD_TASK_INVENTORY` cap (declared + requested),
  `UpdatableAssetType::task_cap`, an `AssetUpdateLocation` enum on
  `Command::UpdateInventoryAsset` (agent vs task, mirroring
  `ScriptUploadLocation`), and the `{task_id, item_id}` uploader body builder —
  reusing the existing two-step CAPS uploader.
- **Permissions.** Agent editability is the item's own `MODIFY` bit; task
  editability is the two-level rule — the object's modify **and** the item's
  modify bit (a redacted/nil task `asset_id` can't be opened at all).

Still to do (each needs machinery owned by another task):

- **Inline clickable embedded items** in the text flow, and **drag-and-drop
  adding** an item into the body — both need the rich-text widget's inline boxes
  ([[viewer-lsl-editor-widget]]) plus a drag source
  ([[viewer-inventory-folder-tree]]). Today the private-use markers stay in the
  edit buffer (rendering as placeholder glyphs) and the items are listed rather
  than drawn inline.
- **URL / SLURL / `secondlife:///app` linkification** of the prose
  ([[viewer-url-linkification]]).
- Opening **other** task-inventory item types (scripts, gestures, …) into their
  own editors, and the **script** Save-Back-to-Object half, remain
  [[viewer-task-inventory-open-and-save-back]] — this task wired only the
  notecard case of `openItem`.
