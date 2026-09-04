---
id: viewer-ui-widget-interaction-suite
title: Deep interaction tests for the stateful widgets
topic: viewer
status: done
origin: user request (2026-07) — end manual re-testing of UI interactions
points: 8
blocked_by: [viewer-ui-interaction-harness, viewer-ui-keyboard-text-harness]
---

Context: [context/viewer.md](../context/viewer.md).

The contract sweep proves reactions exist; the stateful widgets need
scenario tests on top:

- `virtual_list.rs` scrolling via real `AccumulatedMouseScroll` +
  `HoverMap` — the row virtualisation window moves, no dead rows;
- `ui_combo` open/select/dismiss;
- `ui_color_picker` and `ui_texture_picker` drag-on-gradient;
- `ui_table` header-click sorting;
- `ui_tab` switching (all four edge orientations);
- `ui_search` type-to-filter;
- `menu.rs`/`menu_bar.rs` open-navigate-dismiss, including keyboard
  navigation and accelerators;
- divider/resize drags where panels have them.

Each widget test drives the same synthetic pointer/keyboard and asserts
emitted actions plus widget state; committed pinning tables where the
widget has an address space (menu items, combo entries).

## Landed (2026-09-05)

One `mod scenarios` per widget, beside the widget, driving
`sl-viewer-testkit`'s synthetic pointer and keyboard:

| Widget | Covered |
| --- | --- |
| `ui_combo` | open / re-click-to-close, pick an option (selection, `ComboChanged`, closed value text), re-picking the active option, outside-press dismiss, and the **address table** — option *N* is `combo-option:N`, shows the label the table names, in list order |
| `ui_tab` | a click switches the tab in **all four** placements (one app per placement), re-clicking the open tab is inert, divider drag resizes the strip and follows the pointer back |
| `ui_table` | header click sorts + the arrow follows, border drag resizes **without** sorting, click / `Ctrl`+click / `Shift`+click selection, wheel over the rows (and *not* over the header), scrollbar-thumb drag |
| `ui_color_picker` | swatch click opens on the swatch's colour, a track drag sets the channel by the track's own geometry, OK commits, a drag released far outside the track still drives it |
| `ui_texture_picker` | swatch click opens on the swatch's texture, a quick choice previews without committing, OK commits, Cancel reverts to the opened-on texture |
| `ui_search` | type-to-filter end to end: keystrokes narrow a consumer's list, backspace widens it, the clear button restores it, placeholder / clear visibility track the term |
| `menu` | click opens, **sweeping** the pointer switches menus with no second click, a branch's submenu opens on hover and closes on leaving, an entry click emits its action, outside press and `Escape` dismiss, and the mixed gesture — opened by click, walked with the arrows, committed with `Enter` |
| `floater` | title-bar drag moves the window (and its chrome travels with it), grip drag resizes the content area, close button closes |

Three things the build settled, worth keeping:

- **`sl-viewer-ui-core` cannot dev-depend on the testkit.** The testkit is
  built *on* that crate, so the dev-dependency links **two** copies of it
  into one test binary — the `cfg(test)` one and the harness's — and their
  `UiRoot` are different types: a fixture's `Res<UiRoot>` fails parameter
  validation on a resource plainly sitting in the world (a bare
  "Resource does not exist" panic from a system with no name). The virtual
  list's pointer coverage therefore lives in `ui_table`'s scenarios, through
  its main consumer, which also gives the wheel a header it must ignore.
  `virtual_list.rs`'s module docs say so and point at it.
- **`drain_actions` needs `enable_action_recording`** on the app; a
  `UiAction` recorder is not part of `InteractionTest::build`.
- **Accelerators are pinned as broken.** `menu::tests::scenarios::
  a_drawn_accelerator_is_still_inert` asserts `Ctrl+I` on the fixture bar
  does nothing, against [[viewer-menu-accelerators-inert]] — the same
  inverted-canary shape the contract sweep's `LayoutClaim::KnownBroken`
  uses, so the day a generic dispatcher lands the test fails and has to be
  replaced by its positive counterpart in that commit.

Deliberately out of scope, each already somebody else's task: the texture
picker's inventory tree (the harness has no inventory; the quick choices
are the picker's own controls), styling under hover / focus
([[viewer-ui-styling-interaction-tests]]), and recorded geometry
([[viewer-ui-baseline-regressions]]).
