---
id: viewer-ui-inventory-gear-menu-clipped
title: Inventory gear menu is clipped to the floater it drops from
topic: viewer
status: bugs
origin: live review of viewer-ui-menu-keyboard-nav (2026-07-20)
refs: [viewer-ui-context-menu, viewer-ui-menu-bar, viewer-ui-floater-basic]
---

Context: [context/viewer.md](../context/viewer.md).

The inventory floater's **gear (options) menu** opens when its gear button is
pressed, but the drop-down is **clipped to the floater's bounds**: only the part
of the menu that overlaps the window is drawn, and everything below the window
edge is cut off. Observed live on the inventory floater.

**Cause.** The floater's content slot sets `overflow: Overflow::clip()`
(`src/floater.rs`, "a window is a boundary, so nothing renders past the window
edge"), which gives its whole subtree a `CalculatedClip`. The gear menu is a
[[viewer-ui-menu-bar]]-style `spawn_menu_button` whose drop-down popup is
spawned **`ChildOf` the button** (`crate::menu::open_host` →
`build_menu_popup`), and the button lives inside the clipped content. A
`CalculatedClip` is inherited by all descendants regardless of
`position_type: Absolute` or `GlobalZIndex`, so the popup — though it draws at
the menu z-index, above the floater — is still clipped to the floater rectangle.

Note the free (anchorless) **context menu** path does *not* have this bug: it
spawns its popup under a `FreeContextMenu` anchor parented to the **UI root**
(`open_context_menus`), so it escapes every floater's clip. Only the
button-anchored menus (`spawn_menu_button` / `open_host`, and submenus under
them) inherit an enclosing clip.

**Fix direction.** A menu popup must render **outside** any clipping ancestor.
Options:

- Parent the drop-down at the UI root (like the context-menu path) while keeping
  its [`Popover`] anchored to the button entity — Popover positions against the
  anchor's global rect, not its parent, so the visual placement is unchanged but
  the clip is escaped. Submenus (`manage_submenus` builds them `ChildOf` the
  branch row) need the same treatment or they re-inherit the clip.
- Or give menu popups an explicit clip override so an ancestor `Overflow::clip`
  does not reach them.

Verify against both the inventory gear menu **and** a menu-bar menu that would
overhang a clipping panel, and re-check the context-menu path still works (it
already parents at the root).
