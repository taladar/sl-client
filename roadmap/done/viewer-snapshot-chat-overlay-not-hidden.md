---
id: viewer-snapshot-chat-overlay-not-hidden
title: Snapshot include-UI-off leaves the nearby-chat overlay in the shot
topic: viewer
status: done
origin: observed live testing viewer-flexi-resettle-after-snapshot (2026-07-30)
refs: [viewer-snapshot-floater, viewer-flexi-resettle-after-snapshot]
---

Context: [context/viewer.md](../context/viewer.md).

With the snapshot floater's **Include UI** toggle **off** (the default), the
saved shot still shows the transient **nearby-chat overlay** lines — most
visibly the snapshot floater's own "snapshot saved: `<path>`" notice, which
therefore leaks into the *next* snapshot taken shortly after (its fade hold is
still running when the next shutter fires). The photo is supposed to be a clean
world view.

Root cause: `snapshot_floater.rs`'s `start_capture` hides the UI for the shot
by setting `Display::None` on **`UiRoot`** only. But the chat overlay container
(`chat.rs`'s `setup_chat_overlay`) is spawned as a **top-level**
absolute-positioned UI node — it carries no `ChildOf(UiRoot)` — so it sits
*outside* the hidden subtree and stays on screen through the capture. (The HUD
subtree is hidden by its own separate toggle, so this is specifically the
local-chat heads-up overlay.)

Fix directions (pick one):

- Parent the chat overlay under `UiRoot` at spawn, so the include-UI hide
  covers it like every other panel (simplest; check nothing else relies on it
  being a separate root, e.g. `position_chat_overlay`'s absolute anchoring —
  which should be unaffected since it stays absolutely positioned).
- Or have the capture's hide step also hide the chat overlay container
  explicitly (a second entity to toggle alongside `UiRoot`), the way the HUD is
  handled — more surface, but keeps the overlay a root if that matters.

A quick audit for **other** non-`UiRoot` top-level UI nodes is worth doing at
the same time: any of them would leak into an include-UI-off shot too.

Note this is cosmetic and independent of the flexi re-settle fix that surfaced
it; the snapshot save itself works.

## Resolution

Took the first fix direction: `setup_chat_overlay` (`chat.rs`) now spawns the
`ChatOverlayContainer` with `ChildOf(UiRoot)` (and runs
`.after(UiScaffoldSystems::SpawnRoot)`), so the include-UI-off `Display::None`
on `UiRoot` reaches it like every other panel. It stays absolutely positioned,
so anchoring against the full-window root is identical to anchoring against the
window, and `position_chat_overlay` finds it by marker (not by parent), so it is
unaffected. Pinned by a client-side test (`overlay_is_parented_under_ui_root`).

The requested audit of other non-`UiRoot` top-level UI nodes:

- **Avatar name tags** (`avatars.rs` `spawn_label`) are the one other
  default-visible top-level node, but they are **deliberately left as-is**:
  in the reference viewer name tags are world-space HUD text governed by a
  separate "show names" control, not the snapshot Show-UI toggle, so hiding them
  with include-UI-off would diverge. Gating them belongs to a future show-names
  toggle, not to this include-UI hide.
- **Pipeline-status overlay** (`diagnostics.rs`) and the **pick inspector**
  (`avatar_menu.rs`) are debug/opt-in (F3 / `SL_VIEWER_PIPELINE_OVERLAY`, and
  `SL_VIEWER_DEBUG_PICK`); a developer who enabled a debug overlay generally
  wants it in the debug shot, so they are left alone.
- All notification/toast/dialog cards, the flycam button, media controls, the
  emoji picker, the selection rect, and the demo panels already parent under
  `UiRoot` (via a channel/scrim/host that is itself `ChildOf(UiRoot)`), so they
  are already covered.
