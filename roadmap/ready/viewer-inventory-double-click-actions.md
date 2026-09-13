---
id: viewer-inventory-double-click-actions
title: What a double-click opens, per inventory item type
topic: viewer
status: ready
origin: user report (2026-09-13, found opening a notecard)
refs: [viewer-inventory-context-actions, viewer-inventory-open-and-properties,
  viewer-notecard-editor, viewer-task-inventory-open-and-save-back]
---

Context: [context/viewer.md](../context/viewer.md).

**Double-clicking a notecard in the inventory tree does not open it** — the
right-click **Open** does, and the two should not disagree. Found while opening
a notecard to check its body (2026-09-13).

A double-click is the fastest way to open anything in the reference viewer, and
what it does is **per item type** rather than one action: it is the reference's
`LLInventoryPanel::onItemDoubleClicked` → `LLInvFVBridge::openItem`, and each
bridge answers it differently.

So this is not "wire the notecard case" — it is a table, and the table is the
task:

| type | what a double-click does in the reference |
| --- | --- |
| notecard | open the notecard editor |
| script | open the script editor |
| landmark | open About Landmark (the reference does **not** teleport) |
| texture / snapshot | open the texture preview |
| sound | play it locally |
| animation | open the animation preview (play / stop) |
| gesture | open the gesture editor |
| clothing / body part | **wear** it |
| object | wear / attach to its last attachment point |
| calling card | open the avatar's profile |
| settings | open the environment editor |
| material | open the material editor |
| folder | expand / collapse it |
| calling card in Friends, a link | follow the link to its target |

Two rules the table hides, both worth stating because they are where a naive
implementation goes wrong:

- **a link resolves first** — a double-click on an inventory link acts on the
  item it points at, not on the link;
- **a worn item's double-click is "take off"**, not "wear again", for the types
  where wearing is the action.

Where the actions already exist (the notecard and script editors, the texture
preview, About Landmark, wear) this is a dispatch, not new behaviour; the
missing ones (sound local-play, the animation preview) can land as they are
built, provided the dispatch has somewhere inert to send them.

Do the same for the **object contents** list (a prim's task inventory), which
has its own double-click and the same per-type answer —
[[viewer-task-inventory-open-and-save-back]] owns the "open it into its editor"
half of that.

Reference (Firestorm, read-only): `llinventorybridge` (`openItem` per bridge),
`llinventorypanel` (`onItemDoubleClicked`), `llpanelmaininventory`.
