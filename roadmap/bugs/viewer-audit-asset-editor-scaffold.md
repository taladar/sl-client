---
id: viewer-audit-asset-editor-scaffold
title: The wearable editor reports a save that has not happened, and claims other editors' results
topic: viewer
status: bugs
origin: static code audit (2026-08-26)
points: 5
---

Context: [context/viewer.md](../context/viewer.md).

`sl-viewer-asset-editors` is one editor scaffold written three times, and the
divergent third copy carries the bugs:

- `edit_wearable.rs:1012` — `set_status(..., "Saved a copy to inventory.")` is
  written immediately after `Command::UploadAsset` is queued.
  `report_wearable_save` only handles the in-place `Save` path, so a failed CAPS
  upload leaves the editor **claiming success**.
- `edit_wearable.rs:1038` —
  `if let SlSessionEvent::InventoryAssetSaved { success, .. } = &event.0 && edit.saving`
  ignores the event's item id, so **any** inventory-asset save completing while
  this editor is saving is reported as its own. The siblings do it correctly via
  `pending_save: Option<Uuid>` (`edit_notecard.rs:145`, `edit_script.rs`).
- `edit_notecard.rs:194` / `edit_script.rs:214` — neither has dirty tracking or
  an unsaved-changes guard. Both are single-instance floaters and `open_*` tears
  the content down and refetches, so opening a second notecard or script — or
  just closing the floater, which is `closable: true` — **discards unsaved edits
  with no prompt**. Only `edit_wearable` has a Revert.

The duplication that produced this: six helpers are byte-identical modulo one
string literal between `edit_notecard.rs:796-887` and `edit_script.rs:542-606`,
`:733-757` (`tear_down`, `spawn_status`, `set_status`, `spawn_note`,
`spawn_body_field`, `spawn_save_button`), plus seven duplicated constants and
inline colour literals — about 130 lines. The state structs, `spawn_*_floater`,
`open_*`, `ingest_*_asset`, `BuiltEditor`, `populate_editor`, `attach_save` and
`report_*_save` are structural clones.

Scope: fix the three defects, then unify onto one `AssetEditor<T>` scaffold so
there is no third copy to diverge again.
