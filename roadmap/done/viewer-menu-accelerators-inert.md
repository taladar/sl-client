---
id: viewer-menu-accelerators-inert
title: Menu accelerators are drawn but dead (Ctrl+P / Ctrl+T / Ctrl+F / Ctrl+U)
topic: viewer
status: done
origin: Firestorm full-parity audit (2026-08-19)
refs: [viewer-input-modifier-chords, viewer-ui-menu-bar, viewer-image-upload]
---

Context: [context/viewer.md](../context/viewer.md).

The menu bar drew accelerator labels via `MenuCommand::accel` and nothing
dispatched them: the rendering was display-only, every chord that worked had a
bespoke keyboard system somewhere else, and Ctrl+P (Preferences…), Ctrl+T
(Conversations) and Ctrl+F (Search…) were labels promising a shortcut that did
nothing. Ctrl+U (Upload ▸ Image…) sat on a greyed UNIMPLEMENTED entry and would
have lied the same way once [[viewer-image-upload]] landed.

## What landed

`sl-viewer-ui-widgets/src/menu_accel.rs` — one system in `MenuWidgetPlugin`
that walks every mounted `MenuHost` (the top bar's menus, a gear button's
drop-down, the inventory `+` menu), matches the pressed chord against the
accelerator drawn on each entry, and writes the entry's own `UiAction`: the same
message the click path writes, routed by the same handler. A bar gets its
shortcuts by being spawned, and a future `.accel("Ctrl+…")` is live the moment
it is authored.

- `Accelerator::parse` reads the very string the entry draws, so there is one
  spelling of a shortcut in the codebase and the label is it. It knows the
  modifier names, the letters and digits, F1–F12 and the named editing /
  navigation keys; anything else is `None`, which the pinning tests turn into a
  failure rather than a silent dead label.
- The match is on the **exact** modifier set (the reference's
  `mask == (mAcceleratorMask & MASK_NORMALKEYS)`), so Ctrl+Shift+L does not fire
  Ctrl+L's entry and Ctrl+L does not fire Ctrl+Shift+L's.
- `enabled_when` / `visible_when` are honoured, which is exactly what keeps
  Ctrl+U inert while Upload ▸ Image… is greyed — and makes it live, with no
  further wiring, the day the uploader lands.
- `accelerators(menu)` is the pinning surface: the live bar's test asserts that
  every drawn label parses, that no two entries claim one chord (both would
  fire), and pins the twelve entries that carry a shortcut, so gaining or losing
  one is a deliberate edit a reviewer sees.

## The bespoke handlers are gone, and had to be

The task said the existing chord handlers "can collapse onto it". They **must**:
a dispatcher that fires Ctrl+M *beside* a bespoke Ctrl+M would toggle the world
map twice, and Ctrl+Z would undo twice — a double-fire, not a duplicate. Deleted
in this change, each replaced by the accelerator drawn on its own menu entry:

- `Ctrl+Q` — `session::handle_quit_input` → Avatar ▸ Quit.
- `Ctrl+I` — `inventory::toggle_inventory` → Avatar ▸ Inventory.
- `Ctrl+M` — `world_map::toggle_world_map_shortcut` → World ▸ World Map.
- `Ctrl+B` — `edit_tool::toggle_build_floater_on_ctrl_b` → Build ▸ Build Tools.
- `Ctrl+Z` / `Ctrl+Y` — the chord branch of `drive_undo_redo` → Build ▸ Undo /
  Redo.
- `Ctrl+L` / `Ctrl+Shift+L` — the chord branch of `drive_link_unlink` → Build ▸
  Link / Unlink.
- `Ctrl+Alt+Shift+S` — `debug_settings::toggle_debug_settings_shortcut` →
  Advanced ▸ Debug settings….

Each of those systems re-checked the modifiers and its own availability by hand;
the menu entry's `enabled_when` now *is* that check, which is what makes the
label and the keyboard incapable of disagreeing. What they each did by hand is
now done once: the undo / link chords bailed on `InputContext::TextEntry` (the
dispatcher's own rule, below), and the floater toggles resolved their target by
stable floater id rather than through a lazily-built module resource — which is
exactly what `toggle_floater` does on the menu path, so a first open still
works.

## A collision the fix would otherwise have created

The world's action map (`input_action.rs`) binds **bare** keys and, until now,
ignored modifiers entirely: `F` flies, `M` drops to mouselook. So the moment
`Ctrl+F` became live it would *also* have toggled flying, and `Ctrl+M` would
have entered mouselook on its way to the world map — the latter a bug that was
already there, under the bespoke `Ctrl+M` handler, and unnoticed.

`update_action_input` now resolves **no** binding while `Ctrl` or `Alt` is held:
that keystroke is an accelerator, not a movement key. This is the reference's
own rule (`LLViewerInput::handleKey` compares the whole modifier mask, so a
bare-key binding does not fire under a modifier), and `Shift` is deliberately
excluded from the test — it *is* bound (Run) and is not an accelerator modifier,
so `Shift+W` still runs forward. Pinned by
`input_action::tests::a_chord_is_not_a_movement_key`.

Giving the bindings modifiers of their own is still
[[viewer-input-modifier-chords]]; this is the narrow filter that keeps the two
worlds from firing at once until then.

## Where the keyboard has to be — a deliberate divergence

The task said "honouring the world-keyboard focus gate". Taken literally that
would kill every accelerator while a *button* holds focus, which no viewer does.
What landed instead, stated as three cases:

- **a focused text field takes every chord.** The viewer's text editor claims
  Ctrl chords of its own (select-all, copy, paste, undo) and nothing here can
  know whether it wanted this one, so the menu yields wholesale rather than
  racing it. The reference does the opposite — `LLViewerWindow::handleKey`
  offers a modified chord to the menu bar *before* the focused control unless
  the focus declares accelerators — so this costs a Ctrl+P typed into the chat
  bar, and buys the guarantee that nothing here can eat a keystroke meant for
  text.
- **a focused widget keeps the modified chords** and stands the bare ones down:
  there is nothing on a button for a Ctrl chord to collide with, while an
  unmodified accelerator is one keystroke from being typed.
- **an open drop-down** takes unmodified keys as its jump keys, which is the
  reference's own rule (`LLMenuBarGL::handleAcceleratorKey`).

## Verified

`cargo test -p sl-viewer-ui-widgets --lib menu_accel` — ten headless tests: the
label grammar; the walk (including into a submenu, which is never on screen);
the chord running its entry; the exact-modifier match in both directions; a
greyed entry ignoring its chord and going live with it; a hidden entry likewise;
the text-field, widget-focus and world cases. Plus the live bar's own pinning
test in `menu_bar.rs`, the action-map collision test above, and the viewer
crate's own 254 lib tests.

Not verified live: whether every one of the twelve chords does the right thing
in a running session is a manual pass over a logged-in viewer, and the actions
they now route through are the same ones the menu picks already used.

Reference (Firestorm, read-only):
`indra/newview/skins/default/xui/en/menu_viewer.xml` (shortcut=),
`indra/llui/llmenugl.cpp` (`LLMenuItemCallGL::handleAcceleratorKey`,
`LLMenuBarGL::handleAcceleratorKey`), `indra/newview/llviewerwindow.cpp`
(`handleKey`'s focus-versus-menu order).

Left for [[viewer-input-modifier-chords]]: the chords that are *not* drawn on a
menu entry (the quick snapshot, the edit-drag modifiers) and rebindability —
this dispatcher binds a chord by authoring it in a menu, which a rebinding UI
still cannot reach.
