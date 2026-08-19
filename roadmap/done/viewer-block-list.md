---
id: viewer-block-list
title: Block / mute list UI
topic: viewer
status: done
origin: Vintage-parity coverage audit (2026-07-22)
blocked_by: [viewer-social-people-panel, viewer-ui-virtualized-list]
refs: [viewer-derender-blacklist]
---

Context: [context/viewer.md](../context/viewer.md).

The block-list surface over the fully-implemented mute protocol
(`protocol-9`): list every muted resident / object with type icons, unblock,
and the per-mute flag toggles (text / voice / particles / object sounds —
`MuteFlags`). The Vintage skin presents this as a **"Blocked Residents &
Objects" tab inside the People floater**, which is where ours goes; the
avatar and object context menus' Block / Unblock entries stay the quick path
in.

Includes the reference's **block-object-by-name** dialog (mute by name for
spammy objects you cannot click) and the mute-list-full error surface.
Distinct from render-side derendering ([[viewer-derender-blacklist]]) — this
is the server-side mute list.

Reference (Firestorm, read-only): `llpanelblockedlist`, `llmutelist`,
`floater_fs_blocklist.xml`, `floater_mute_object.xml`; Vintage
`panel_people.xml` (the added Blocked tab).

Builds on: `protocol-9` mute list and the People panel.

## Built

- `mutes.rs` widened from an id `HashSet` to the whole `MuteEntry` list
  (name / type / `MuteFlags`) plus a derived id index, so the hot per-frame
  `is_muted` query stays one hash lookup. New: `is_muted_aspect`, `entries`,
  `revision`, `is_full`, `has_by_name`, `entry`. A non-nil id matches by id
  alone; a `ByName` entry (nil id) matches by case-folded name.
- **One guarded way into the mute list.** Every Block affordance in the
  viewer now writes a `mutes::RequestBlock` instead of putting
  `Command::Mute` on the wire itself; `apply_block_requests` runs the
  reference's `LLMuteList::add` guards and only then sends. Converted: the
  avatar / object / attachment pie menus, the radar, the minimap, the avatar
  profile, the inspector popup, the friends list, the inventory-offer,
  script-dialog, script-permission, experience-permission and load-URL
  toasts, `secondlife:///…/mute` links, and the block list's own add paths
  and aspect toggles. The guards refuse blocking yourself, a Linden's text
  chat, a malformed or duplicate by-name entry, and an over-full list —
  raising `MuteLimitReached` / `MuteLinden` / `MuteByNameFailed`, catalogue
  entries that had no caller before.
- `blocked.rs` — the Blocked sub-tab: filter box, sortable virtualized
  Name / Type table, gear context menu (Unblock, the four aspect toggles,
  Profile), and the Unblock / Block Resident… / Block object by name…
  action buttons. Aspect toggles flip the exception bit and re-block the
  entry; excepting the last aspect removes it, as the reference does.
- `people.rs` grew the third sub-tab and its content slot; the
  Friends/Groups switch became a three-way. New `OpenPeopleSubTab` message
  (held until the deferred pane exists) fronts a named sub-tab.
- **Comm menu entries** — `Friends`, `Groups`, `Block List`, mirroring the
  reference's `Comm > Contacts / Groups / Block List`. Without them the
  three lists were only reachable by opening Conversations and clicking two
  levels of tabs. `floater::show_floater` (open, never close) is the new
  primitive these need.
- The "Block Object by Name" floater (the reference's
  `floater_mute_object.xml`), opened from the block list.
- `AvatarPicked` now carries the picked row's name, so a block records a
  real name rather than an empty one.
- `world_sounds::muted` honours the object-sounds exception, making that
  toggle actually do something client-side.

## Divergences

- **Type icons → a Type column.** Rows spell the kind out in a sortable,
  localized Type column instead of carrying an icon glyph. Same information,
  legible without a legend, and it is what the reference's own "sort by
  type" view option orders on.
- **The list-full guard is skipped for an update.** A `RequestBlock` whose
  target is already on the list adds no row (it is how the aspect toggles
  re-send), so the limit does not apply. The reference refuses those too,
  which would silently wedge the toggles at exactly the limit.
- Multi-select unblocking (a Firestorm addition) is not built: the list is
  single-select, like Linden's.

## Verification

Unit-tested: the filter, both sort orders, type-key distinctness, the
aspect-toggle algebra including the drop-on-last-aspect rule, Linden
detection, and every `check_block` guard (self, Linden text-chat-only,
by-name shape, by-name duplicates that ignore id-keyed entries, and the
limit with its update exemption). 1287 viewer tests pass; clippy clean.

Live-launched against the local OpenSim and reviewed via Comm ▸ Block List.
Not systematically exercised yet: an Xfer'd mute list with pre-existing
entries, the three refusal notifications firing in-world, and the
object-sounds exception actually silencing a sound.
