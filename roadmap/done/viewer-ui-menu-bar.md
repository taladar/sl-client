---
id: viewer-ui-menu-bar
title: Top menu bar
topic: viewer
status: done
origin: gap noticed reviewing the UI cluster (2026-07)
blocked_by: [viewer-ui-context-menu]
refs: [viewer-ui-bottom-toolbar, viewer-ui-status-bar, viewer-ui-menu-search, viewer-ui-menu-keyboard-nav]
---

Context: [context/viewer.md](../context/viewer.md).

The viewer's **top menu bar** — the horizontal strip of pull-down menus
(Avatar / Comm / World / Build / Content, or Firestorm's Me / Comm / World /
Build / Content / Help arrangement) — each opening a line-based menu of actions
and toggles. Builds on the line-based menu mechanism
([[viewer-ui-context-menu]]); this task is the bar itself, the menu tree, and
wiring each entry to its command / floater toggle.

Per the layout conventions the bar and its menus size to content and reflow on a
font-size / locale change.

Reference (Firestorm, read-only): `llmenubarview`, `menu_viewer.xml`.

## Done (2026-07-20)

The bar itself and the top-level menu **names** are done, on the reusable
line-menu widget ([[viewer-ui-context-menu]]); the live tree is
`src/menu_bar.rs` (`TopMenuBarPlugin`). Per the request that shaped the scope,
this landed as "names now, entries as they land": the six standard menus —
Avatar, Comm, World, Build, Content, Help — stand up so future tasks have a home
to hang a command in, and only the entries with a live target today are wired —
**Quit** and the **Inventory** window toggle (with a `✓` when it is open). An
empty menu shows a single disabled placeholder so it is a real (openable) menu
that reads as "not populated yet". A future task adds a `MenuItemDef` to a
`static` menu and wires its `action` string in one dispatch
(`handle_top_menu_actions`); nothing about the bar changes.

The bar is content-sized and top-leading (its own convention), leaving the top
edge's right side free for the diagnostics overlay and the status area. Hovering
a second top menu while one is open switches to it (the reference's
`LLMenuBarGL::handleHover`).

**Reusable, and the inventory shares it.** The reference's inventory window does
**not** use a File/Edit menu bar — it uses
**gear / view / add dropdown buttons** (`LLMenuButton` + `LLToggleableMenu`). So
the shared unit is the *menu button that drops a line menu*, exposed as
`crate::menu::spawn_menu_button`. The inventory window now carries a
**⚙ gear menu** built on it (`src/inventory.rs`), wired to the Expand All /
Collapse All actions it already has; the rest of the reference gear entries are
a placeholder for future inventory tasks.

**Split out to follow-ups (were the "status area" of this task):**

- Menu **search** field → [[viewer-ui-menu-search]].
- The rest of `llstatusbar` — region / parcel name, agent position, parcel
  permission icons, L$ balance, time, FPS — → [[viewer-ui-status-bar]].
- Keyboard traversal of an open menu → [[viewer-ui-menu-keyboard-nav]].
- Translating the menu **labels** through Fluent (they are `&'static str` today)
  → folds into the per-domain entry tasks as they add real entries.
