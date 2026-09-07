---
id: viewer-new-notecard-unreadable-on-opensim
title: A notecard created on OpenSim opened as "could not be read"
topic: viewer
status: done
origin: found live-checking the keyed notecard editor on the local grid
  (2026-09-06)
refs: [viewer-notecard-editor, viewer-keyed-floater-audit]
---

Context: [context/viewer.md](../context/viewer.md).

Every notecard made with **New Notecard** on the local OpenSim grid opened as
*"This notecard could not be read."* — including one created seconds earlier.
The log named it exactly:

```text
WARN sl_viewer_asset_editors::edit_notecard: failed to decode notecard
  0c9c28eb-…: expected "Linden text version " but found "\0"
```

## What the grid actually stores

The viewer's create path is the reference's — `CreateInventoryItem` with
`AssetType::Notecard`, which asks the **server** to make the asset. Reading the
grid's own store settles what it makes:

```text
$ sqlite3 Asset.db "select UUID, Name, Type, length(Data) from assets where …"
0c9c28eb-…|New Note|7|1
$ sqlite3 Asset.db "select hex(substr(Data,1,40)) from assets where …"
0c9c28eb-…|00
```

One byte, `0x00`. Second Life writes a valid empty Linden-text container for a
new notecard, which is why this never showed on aditi; OpenSim writes a
placeholder byte. Both mean "this notecard has nothing in it yet".

## The fix

`decode_notecard_asset` in `edit_notecard.rs` reads an **all-zero (or empty)**
payload as an empty notecard, so a fresh notecard opens as an empty editable
one and the first Save writes a proper container (after which it decodes
normally on every grid).

Deliberately narrow, so it is a reading of the grid's "empty" rather than a
swallowed error: a valid notecard always starts with `Linden text version `, so
no truncation of a real one can be mistaken for empty, and any other malformed
asset still fails, logs and shows the unreadable status —
`an_empty_asset_reads_as_an_empty_notecard` pins all three cases.

## Not the editor's keying

Found while live-checking the per-notecard windows
([[viewer-keyed-floater-audit]]), but independent of them: the same decode ran
in the singleton editor, and the same four notecards fail there. The keying is
what made it obvious — every window showed the same message.
