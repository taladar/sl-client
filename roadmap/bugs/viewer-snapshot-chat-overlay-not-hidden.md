---
id: viewer-snapshot-chat-overlay-not-hidden
title: Snapshot include-UI-off leaves the nearby-chat overlay in the shot
topic: viewer
status: bugs
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
