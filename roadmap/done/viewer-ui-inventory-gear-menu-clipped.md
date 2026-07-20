---
id: viewer-ui-inventory-gear-menu-clipped
title: Inventory gear menu is clipped to the floater it drops from
topic: viewer
status: done
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

## Resolution

Took the **clip-override** option, not the reparent one. Investigating the
`Popover` source (`bevy_ui_widgets::popover::position_popover`) showed the first
suggested fix does not work as described: `Popover` has no separate anchor
field — it always positions the popup against its own `ChildOf` parent
(`parent.parent()`), so reparenting the popup to the UI root would also move its
positioning reference to the root. Escaping the clip therefore has to leave the
popup parented to the button.

Bevy 0.19's `bevy_ui` provides exactly the needed primitive:
[`OverrideClip`](https://docs.rs/bevy_ui) — a marker component that makes
`update_clipping_system` discard any inherited clip rect for that node (and,
per `bevy_ui::focus`, picking honours it too). Added `OverrideClip` to the popup
in `build_menu_popup`, which every menu popup routes through — top-level bar
menus, the gear button (`spawn_menu_button` / `open_host`), submenus
(`manage_submenus`), and the free context menu — so the escape is uniform. The
popup has `overflow: visible`, so its rows inherit the now-cleared clip and draw
in full as well. On the context-menu path (already root-parented, no inherited
clip) the marker is a harmless no-op.

Tests: `src/menu.rs` gains
`a_menu_popup_escapes_a_clipping_ancestor`, which spawns a menu popup under a
button inside an `Overflow::clip()` window and asserts the popup gets **no**
`CalculatedClip` — while a control sibling inside the same window does, proving
the scene really clips and the assertion is not vacuous.
