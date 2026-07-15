# sl-notecard

Pure decoder / encoder for the Second Life / OpenSim **Linden-text** notecard
asset. It is the notecard counterpart of `sl-prim`, `sl-sculpt` and the other
format crates: **Bevy-free and I/O-free**, its only substantive dependency is
`sl-types`, so it can be tested, fuzzed and reused (a CLI, an inventory tool, a
bulk exporter) with no session and no grid.

A notecard is *not* plain text. On the wire it is a versioned container carrying
the notecard text **plus embedded inventory items** — the landmarks, objects and
other notecards a resident drops into the body, which the viewer renders inline
as clickable items. The text references each item positionally: a Unicode code
point `FIRST_EMBEDDED_CHAR + index` (version 2) — or a byte `0x80 | index` (the
legacy version 1) — stands in the text where the item is shown.

```text
Linden text version 2
{
LLEmbeddedItems version 1
{
count 1
{
ext char index 0
	inv_item	0
	{
		item_id	...
		permissions 0
		{
			base_mask	7fffffff
			...
		}
		asset_id	...
		type	landmark
		inv_type	landmark
		...
	}
}
}
Text length 20
See this place: <U+100000>
}
```

## Usage

- `Notecard::decode(bytes)` parses the container, the embedded-item table and
  the text body, undoing the `shadow_id` obfuscation and mapping both version's
  embedded markers onto the uniform `FIRST_EMBEDDED_CHAR + index` code points in
  `Notecard::text`.
- `Notecard::encode()` writes the notecard back out, always as the current
  **version 2** container (the way the reference viewer does on save), matching
  the simulator's field order, tab indentation and `%08x` mask formatting, so a
  notecard decoded from a live grid re-encodes byte-for-byte.
- `Notecard::embedded_references()` reports where each embedded item sits in the
  text; `Notecard::item_by_index(i)` resolves a reference to its
  `EmbeddedItem`.

Each `EmbeddedItem` is modelled as the inventory item it is — id, asset id,
asset / inventory type, name, description and the full **permission** masks
(copying a notecard copies its contents, so the permission bits are not
decoration). Unrecognised type names and unknown keyword lines are **preserved
verbatim** rather than dropped — this is somebody's inventory.

The format follows Firestorm's `LLNotecard` / `LLInventoryItem` legacy stream,
reimplemented idiomatically rather than copied.
