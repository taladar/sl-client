---
id: viewer-rlv-enforce-forced-actions
title: RLV — forced actions and the #RLV inventory sub-protocol
topic: viewer
status: blocked
origin: user request (2026-07); split from viewer-rlva-enforcement
blocked_by: [viewer-rlv-restriction-state, viewer-sit-stand-actions, viewer-inventory-folder-tree]
---

Context: [context/viewer.md](../context/viewer.md).

Perform the `=force` **forced actions** the object commands, driven by
[[viewer-rlv-restriction-state]]:

- `@sit:<uuid>=force` / `@unsit=force` — via the sit/stand path
  ([[viewer-sit-stand-actions]]);
- `@tpto:<x>/<y>/<z>=force` — force a teleport to a location;
- `@remoutfit=force`, `@attach:<path>=force` — outfit changes against the
  **shared `#RLV` inventory folder**.

The `#RLV` folder is a whole **sub-protocol of its own**: a folder tree the
object addresses **by path** ([[viewer-inventory-folder-tree]] supplies the
tree), with `@getinvworn` (which folders' contents are worn), `@findfolder`
(resolve a name to a path), and path-addressed attach/detach. Model the path
addressing and the worn-state reporting faithfully — content relies on the exact
folder-name matching rules the reference uses.

Reference (Firestorm, read-only): `rlvinventory.cpp` (the `#RLV` folder tree,
path addressing, `@getinvworn` / `@findfolder`), `rlvhandler.cpp` (`=force`
dispatch).

## Parity-audit addendum (2026-08-19)

Missing scope found by the audit: the full force-wear keyword matrix —
28 strings (`@attach`, `@attachall`, `@attachover`, `@attachallover`,
`@attachthis`, `@attachallthis`, `@attachthisover`,
`@attachallthisover`; the 8 `@addoutfit*` synonyms; the 4
`@*overorreplace` synonyms; `@detach`, `@detachall`, `@detachthis`,
`@detachallthis`, `@remattach`, `@remoutfit` as force commands) with
the WEAR_REPLACE vs WEAR_ADD vs WEAR_REMOVE × NODE vs SUBTREE ×
CONTEXT_NONE vs CONTEXT_OBJECT semantics encoded in the dictionary
flags (rlvhelper.cpp:256-283 + rlvinventory.cpp). Also `@detachme`,
`@sitground`, `@adjustheight`, `@setgroup=force`, `@fly=force`; the
`#RLV` folder-name matching rules (`+` stack prefix, `.` hidden folders,
the WearAddPrefix / WearReplacePrefix settings); the `@sharedwear` /
`@sharedunwear` gating restrictions (RLVaEnableSharedWear); the
ForbidGiveToRlv setting (accept llGiveInventory folders into #RLV); and
RLVaSharedInvAutoRename.
