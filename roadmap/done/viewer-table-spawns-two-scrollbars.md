---
id: viewer-table-spawns-two-scrollbars
title: Seven tables carry two stacked scrollbars, because the consumer adds one
  the widget already spawned
topic: viewer
status: done
origin: found fixing [[viewer-table-scrollbar-overlays-last-column]] (2026-09-12)
points: 1
refs: [viewer-table-scrollbar-overlays-last-column, viewer-ui-table-widget]
---

Context: [context/viewer.md](../context/viewer.md).

`spawn_table` (`sl-viewer-ui-widgets/src/ui_table.rs`) spawns the viewport's
scrollbar itself — "the scrollbar every long table needs". Seven consumers
spawn a **second** one on the same viewport straight after:

```text
let table = spawn_table(&mut commands, table_column, &RADAR_TABLE);
…
spawn_virtual_scrollbar(&mut commands, table.viewport);
```

- `sl-viewer-people/src/radar.rs:917`
- `sl-viewer-people/src/blocked.rs:538`
- `sl-viewer-people/src/contact_sets_panel.rs:791`
- `sl-client-bevy-viewer/src/asset_blacklist.rs:426`
- `sl-client-bevy-viewer/src/avatar_render_floater.rs:457`
- `sl-viewer-rlv/src/rlv_behaviours.rs:551`
- `sl-viewer-rlv/src/rlv_locks.rs:385`

(`rlv_console.rs:476` is **not** one of these — it passes a viewport of its
own, not a `TableHandle`'s, so its bar is the only one that viewport has.)

The two tracks are identical and pinned to the same edge, so they land exactly
on top of each other and the panel looks right — which is why this has gone
unseen. Both are driven by `drive_virtual_scrollbars`, both show and hide
together, and a press reaches only the topmost thumb, so the duplicate is inert
rather than wrong. It is still two nodes, two thumbs and two drag observers per
table for nothing, and a reader of any of those seven panels is told something
false about who owns the bar.

## The fix

The seven calls are gone; `spawn_table` is the one place a table's bar is
spawned, and each consumer's `spawn_virtual_scrollbar` import went with the
call. `rlv_console.rs` keeps its call: its viewport is hand-rolled rather than
a `TableHandle`'s, so that bar is the only one that viewport has — which is
also why `spawn_virtual_scrollbar` stays public.

Making the entry point refuse a viewport that already has a bar was
considered and not done: it takes `&mut Commands`, so it cannot see the
viewport's existing children at all, and a system that swept for duplicates
would be more machinery than the mistake is worth now that a test pins it.

## How to verify

A widget test pins it where it happened: a table's viewport has exactly one
`virtual-list:scrollbar` child after `spawn_table`. The seven panels are
otherwise untouched — the bar they show is the one the widget always spawned,
and the removed one was painted underneath it.
