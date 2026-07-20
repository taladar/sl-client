---
id: viewer-ui-menu-keyboard-nav
title: Keyboard traversal of an open menu
topic: viewer
status: ready
origin: deferred piece of viewer-ui-context-menu (2026-07-20)
blocked_by: [viewer-ui-context-menu]
refs: [viewer-ui-menu-bar]
---

Context: [context/viewer.md](../context/viewer.md).

Keyboard navigation **within an open menu** — the one piece
[[viewer-ui-context-menu]] deferred. Today a bar button is `Tab`-reachable and
opens, but stepping through the open list is mouse-only.

Wanted (the reference's `LLMenuGL` keyboard behaviour): arrow keys move the
highlight between entries; `Enter` / `Space` activates the highlighted entry;
`Escape` closes (already wired); arrow-toward-the-inline-end opens a submenu and
arrow-toward-the-inline-start closes it; and the reference's **jump keys** — the
underlined mnemonic letter, active only once keyboard navigation has begun, that
jumps to / activates the matching entry.

**Why it was deferred, and the constraint that shaped it.** The widget is
deliberately *self-managed* (`src/menu.rs`): open / close / highlight run off
plain press observers, because `bevy_ui_widgets`' `MenuPopup` focus lifecycle —
the thing that would have brought arrow navigation for free — did not fire its
activation chain in this app and fought the viewer's own input-focus context
(see the context-menu Done note). So this task adds keyboard traversal **in the
same self-managed spirit**: drive a highlighted-entry index from key input and
reuse the existing `MenuEntryAction` dispatch and `manage_submenus` open/close,
rather than re-adopting the upstream focus machinery. The highlight is already a
component the hover system paints; keyboard just becomes a second writer of it.

Reference (Firestorm, read-only): `indra/llui/llmenugl.{h,cpp}`
(`LLMenuGL::handleKey` / `handleJumpKey` / `createJumpKeys`, the arrow-key and
mnemonic handling).
