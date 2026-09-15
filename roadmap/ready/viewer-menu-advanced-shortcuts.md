---
id: viewer-menu-advanced-shortcuts
title: Advanced ▸ Shortcuts — the rest of the reference's submenu
topic: viewer
status: ready
origin: split out of [[viewer-wasd-moves-flycam-in-world]] while adding
  Joystick Flycam to it (2026-09-15)
refs: [viewer-ui-menu-bar, viewer-menu-accelerators-inert,
  viewer-menu-bar-fill-implemented-entries, viewer-camera-flycam]
---

Context: [context/viewer.md](../context/viewer.md).

`Advanced ▸ Shortcuts` exists with **one** entry, *Joystick Flycam*, added by
[[viewer-wasd-moves-flycam-in-world]]. The reference's submenu has eighteen.
This is the rest of it.

Firestorm has no *View* menu: everything the Linden viewer files there lives in
this submenu, which doubles as the bar's keyboard-shortcut index.

## The reference's entries

`menu_viewer.xml`, Advanced ▸ Shortcuts, in order:

| Entry | Chord | Backing |
| --- | --- | --- |
| Search | `Ctrl+F` | exists — Content ▸ Search |
| DoubleClick Teleport | `Ctrl+Shift+D` | exists — `double_click_teleport.rs` |
| Always Run | `Ctrl+R` | `movement.rs` run state |
| Fly | `Home` | exists — `Action::ToggleFly` |
| Close Window | `Ctrl+W` | exists — `floater.rs` |
| Close Window Group | `Ctrl+Alt+W` | new |
| Close All Windows | `Ctrl+Shift+W` | new |
| Snapshot to Disk | ``Ctrl+` `` | exists — `snapshot_floater.rs` |
| Mouselook | `M` | exists — `Action::ToggleMouselook` |
| **Joystick Flycam** | `Alt+Shift+F` | **done** |
| Reset View | `Esc` | exists — `camera::reset_camera_view` |
| Reset Camera Angles | `Shift+Esc` | new (`CameraRig::reset_orbit` without the mode reset) |
| Look at Last Chatter | `Ctrl+\` | new |
| Zoom In | `Ctrl+0` | `CameraRig::distance` step |
| Zoom Default | `Ctrl+9` | `CameraRig::distance` reset |
| Zoom Out | `Ctrl+8` | `CameraRig::distance` step |

## The blocker to settle first

**Most of these duplicate a command that already has a home in the bar**, which
is exactly what this viewer's menu bar currently forbids:

- `no_two_entries_in_the_bar_share_an_action` — `handle_top_menu_actions`
  matches on the action string alone, so two entries with one action are one
  entry with two labels, and [[viewer-ui-menu-search]] would offer both.
- `every_drawn_accelerator_is_a_chord_the_keyboard_dispatches` also pins that no
  two entries claim the same chord, and a shortcut-index copy claims the chord
  its original already draws.

So the reference's shape needs one of:

1. an **alias** marker on a `MenuCommand` — same action, excluded from both
   uniqueness checks and from menu search, with the accelerator drawn but
   dispatched by the original entry only; or
2. dropping the duplicated entries and shipping only the eight that have no
   other home (the camera / view group plus the two window-closing commands),
   accepting a deliberate divergence from the reference.

Option 1 is what "look exactly like the reference" asks for. Decide it here
rather than per-entry, because it is a statement about the whole bar.

## Also

`Esc` and `Shift+Esc` as *drawn* accelerators need care: `Escape` in the world
is already the camera reset, and `Escape` with a focused UI releases focus
([[viewer-input-focus-contexts]]). The accelerator dispatcher yields a chord to
a focused text field wholesale, so the label is honest — but check the
unmodified-accelerator-needs-the-world rule covers the focused-widget case too
before drawing it.

Reference (Firestorm, read-only):
`indra/newview/skins/default/xui/en/menu_viewer.xml` (Advanced ▸ Shortcuts),
`indra/newview/llviewermenu.cpp` (`View.*` callbacks).
