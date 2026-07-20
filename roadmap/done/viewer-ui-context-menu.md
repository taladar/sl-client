---
id: viewer-ui-context-menu
title: Line-based context menu widget
topic: viewer
status: done
origin: noticed as a missing fundamental while reviewing viewer-ui-widget-scaffold (2026-07)
blocked_by: [viewer-ui-widget-scaffold]
refs: [viewer-ui-radial-menu, viewer-object-context-menu, viewer-ui-skin-tokens, viewer-ui-menu-bar, viewer-ui-menu-keyboard-nav]
---

Context: [context/viewer.md](../context/viewer.md).

The **general mechanism** for a conventional line-based context / popup menu —
the other half of [[viewer-ui-radial-menu]], and the shape every non-world menu
wants (a menu bar, an inventory item's right-click, a list row's actions).

Much smaller than the pie, because upstream has most of it: `bevy_ui_widgets`
ships headless `MenuPopup`, `MenuItem`, `MenuButton`, `MenuLayout` and a
`PopoverPlugin`, all keyboard-navigable already. The work is largely to style
them against [[viewer-ui-skin-tokens]] and add what a viewer menu needs on top:
separators, checked / radio items, disabled items with a reason, keyboard
accelerators shown against their entry, and submenus.

**Both menu shapes must reach the same entries.** The reference makes pie vs.
line a *preference* (`UsePieMenu` in `newview/app_settings/settings.xml`), not
two separate feature sets, and so should we: a menu is a tree of entries, and
the two widgets are two presentations of it. Design the entry model here (or in
whichever of the two lands first) so it is shared rather than duplicated —
otherwise the object pie and the object context menu drift apart, which is
exactly what happened upstream.

Per the scaffold's conventions: no physical `left` / `right` in the API or the
styling — a submenu opens toward the **inline end** and flips at the screen
edge, which under an RTL locale is the other side of the screen with no separate
code.

Deliberately **not** in scope: which entries any given menu holds
([[viewer-object-context-menu]] and the other per-domain tasks).

Reference (Firestorm, read-only): `indra/llui/llmenugl.{h,cpp}` (`LLMenuGL`,
`LLMenuItemGL`, `LLContextMenu`), `newview/llviewermenu.cpp`, and the
`menu_*.xml` layouts under `newview/skins/` as a feature checklist.

## Done (2026-07-20)

Landed as `src/menu.rs` (widget mechanism) together with the top bar
([[viewer-ui-menu-bar]]) and the inventory gear menu, in one commit.

**Shape.** A menu is a tree of data — `MenuCommand` (label, `action` string, and
named `enabled_when` / `checked_when` / `visible_when` condition keys, plus an
accelerator label), `MenuItemDef::{Command,Submenu,Separator}`, `MenuDef`,
`MenuBarDef`. The entry vocabulary deliberately mirrors the pie's `PieAction`
(`label` / `action` / `when`) and both widgets dispatch the **same** way — a
`UiAction { element, action }` message — so a domain's entries can be shared
between the pie and the line presentation rather than drifting. Conditions are
named keys resolved from a `MenuConditions` component filled from the world
(empty in the gallery / tests), exactly like the pie. Everything ships: check /
radio marks (a `✓` in a fixed leading gutter), disabled-in-place vs
absent-when-hidden, accelerators drawn against the entry, separators, and
submenus that open toward the inline end and flip at the screen edge
(`bevy_ui_widgets`' `Popover`, folded against `UiDirection` so it mirrors under
RTL).

**Not built as planned — the widget is self-managed, not upstream-driven.** The
task expected to lean on `bevy_ui_widgets`' `MenuPopup` / `MenuButton` /
`MenuItem` activation and keyboard machinery. In practice the upstream
`Pointer<Press> → Activate → MenuEvent` chain did **not** fire in this app (a
press reached the button but `button_on_pointer_down` never produced an
`Activate`), and the `MenuPopup` focus lifecycle fought the viewer's own
input-focus context. So open / close / highlight are driven here off plain
**press observers** on each row (which do fire reliably): a bar button toggles
its menu, an entry runs it and closes the stack, a press that bubbles to the UI
root dismisses everything (each menu row `propagate(false)`s its own press), and
the highlight is Rust-painted (bevy_flair `:hover` did not read the same in the
gallery and viewer). See the `sl-client-viewer-ui-gotchas` memory for the
picking gotcha that was the crux. **Keyboard traversal of an *open* menu**
(arrow keys between entries) is the one deferred piece →
[[viewer-ui-menu-keyboard-nav]].

**Tests / gallery.** `crate::menu`'s tests pin the fixture bar's whole action
table (moving an entry is a loud diff, like the pie's address table), the
condition gating, and the opened-popup layout. The gallery registers the
**closed** bar as a `menu-bar` element and adds a header toggle that flips the
right-click surface between a pie and this line menu, so the two presentations
are compared side by side rather than one being a pre-opened card.
