---
id: viewer-ui-menu-search
title: Menu search field
topic: viewer
status: done
origin: split from viewer-ui-menu-bar's status area (2026-07-20)
blocked_by: [viewer-ui-menu-bar]
refs: [viewer-ui-status-bar, viewer-ui-text-input-widget]
---

**Done (2026-07-20).** Presentation chosen: the reference's **dropdown
filtering** (`LLStatusBar::onUpdateFilterTerm` / `hightlightAndHide`), not a
separate results list. A single-line search field sits **in the menu bar**, just
after the last menu (`src/menu_search.rs`, spawned by `src/menu_bar.rs`). Its
text feeds a new `menu::MenuFilter` resource, and `src/menu.rs` applies it as a
drop-down is built: while a term is active, a menu's drop-down shows only the
entries whose label matches (drawn in a warm accent), hides the rest, keeps a
submenu whose subtree carries a match, and shows a whole menu whose own label
matched. Typing **auto-opens the first bar menu (in bar order) that has a
match** (`open_filtered_menu`), so a term shows its result immediately; hovering
another top menu opens *its* filtered drop-down through the same path
(`switch_menu_on_hover`). `Escape` clears the term and closes the menu at once.
(Stepping between menus by left/right arrow is deferred to
[[viewer-ui-menu-keyboard-nav]].)

The search box also carries a circular **×** clear button (shown only while a
term is present) that empties the field — the reference's search-editor
affordance. Building the box turned up and fixed a latent `crate::menu` bug: an
absolutely-positioned drop-down that stretched its rows on the cross axis was
grown far taller than its content by taffy, leaving dead space below the last
entry (starkest on the one-line "(no entries yet)" placeholder). Popups now
align to the start and rows fill width via an explicit `width: 100%`, so every
menu hugs its content while the hover highlight stays a full-width bar.

(A first pass also built a flat `path ▸ label` results list — the design's other
candidate — but it was dropped: the menus themselves *are* the results, so a
second surface was redundant.)

Did **not** wait on `viewer-ui-text-input-widget` (parley-blocked): the field
uses `bevy::text::EditableText` directly, single-line, the same pattern the
inventory search field already uses. When the reusable text-input widget lands,
this field can adopt it. Tests: 2 live in `menu_search` (field → filter, Escape
clears), 4 filter tests in `menu` (hide non-matching, matched-label shows all,
submenu kept for a nested match, `subtree_matches_filter`).

Context: [context/viewer.md](../context/viewer.md).

The reference viewer's **menu search** — a small text field in the status area
(`search_menu_edit` in `panel_status_bar.xml`, driven by
`LLStatusBar::onUpdateFilterTerm`) that finds an action *within the menus*: type
a few letters and the menu bar filters to the entries whose label matches, so a
command is reachable without knowing which menu holds it.

Builds directly on the menu widget ([[viewer-ui-menu-bar]] / the `crate::menu`
mechanism): the menus are already a **tree of data** (`MenuDef` / `MenuCommand`
with a stable `action` string and a `label`), so a search is a pure filter over
that tree — walk the whole bar's entries, match the query against each label
(and, usefully, against the `action` id and the `>`-joined menu path), and
present the hits. Two presentations worth considering, in reference-fidelity
order: (1) filter the open menu / dropdowns to the matching entries the way the
reference does, and (2) a flat results list of `menu-path > label` that
activates the entry's `action` directly (arguably better than the reference,
since the tree walk already produces the path). Activating a hit writes the same
`UiAction { element, action }` the menu would, so no per-entry rewiring.

Needs a single-line text input, so it waits on [[viewer-ui-text-input-widget]].
Belongs beside the other status-area elements ([[viewer-ui-status-bar]]) but is
its own task because it is a *menu* interaction (a filter over the menu tree),
not a status read-out.

Reference (Firestorm, read-only): `indra/newview/llstatusbar.{h,cpp}`
(`mFilterEdit`, `onUpdateFilterTerm`, `updateMenuSearchPosition`),
`newview/skins/default/xui/en/panel_status_bar.xml`.
