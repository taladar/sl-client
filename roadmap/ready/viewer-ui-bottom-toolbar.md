---
id: viewer-ui-bottom-toolbar
title: Bottom toolbar (button bar)
topic: viewer
status: ready
origin: gap noticed reviewing the UI cluster (2026-07)
blocked_by: [viewer-ui-widget-scaffold]
refs: [viewer-ui-menu-bar, viewer-ui-floater-basic, viewer-chat-input-bar, viewer-volume-panel, viewer-quick-preferences]
---

Context: [context/viewer.md](../context/viewer.md).

The viewer's **bottom button bar** — the row of toggle buttons that open the
main floaters (Inventory, Appearance, Map, People, Chat, Camera controls, …),
the reference viewer's persistent bottom toolbar. Each button toggles its
floater / panel (e.g. the Inventory button toggles
[[viewer-inventory-folder-tree]], which today is only reachable by `Ctrl+I`).

Per the layout conventions the bar sizes to content, wraps / reflows rather than
overflowing, and mirrors under RTL. Which floaters exist to toggle grows as the
UI cluster lands; the bar is the host.

Reference (Firestorm, read-only): `llbottomtray` / the toolbar buttons
(`lltoolbar`).

## Bottom area neighbours (2026-07-18)

The button bar is the bottom-most strip; the reference viewer stacks several
other controls **above** it, which mostly have their own tasks — this task hosts
the button bar and the overall bottom-area layout, they fill in the rest:

- **Nearby chat bar** — [[viewer-chat-input-bar]].
- **Audio controls** — the master volume button / slider, opening
  [[viewer-volume-panel]].
- **Voice controls** — talk / mute for the session. **Signalling only**: joining
  / leaving the voice channel and self-mute state are in scope
  ([[viewer-voice-audio]] tracks the transport, which is out of scope), but the
  "who is speaking" indicators and audio itself are **not** (see the
  voice-signalling-only scope decision).
- **Quick preferences** — [[viewer-quick-preferences]].

The bar and these controls follow the content-sizing / RTL-mirroring
conventions.
