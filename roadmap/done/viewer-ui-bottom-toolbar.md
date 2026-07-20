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

## Done

`src/bottom_toolbar.rs` — `BottomToolbarPlugin`. A bottom-anchored, full-width
area (absolute, `LogicalInset` at `block_end`/both inline edges 0, so it mirrors
under RTL) holding an **upper stack** for the neighbour controls and, below it,
the **button bar** — a full-width surface whose toggle buttons are centred and
**wrap upward** (`FlexWrap::WrapReverse`) when the window is too narrow, so a
wrapped line stacks above rather than off the bottom of the screen.

Buttons come from a `static TOOLBAR_BUTTONS` table in the reference order
(Inventory, Appearance, Map, Mini-map, People, Conversations, Camera). Only
**Inventory** has a live floater today, so it is the only wired one: pressing it
flips the inventory window's `UiPanelShown` (`handle_toolbar_actions`, the same
flip the top menu bar's Avatar ▸ Inventory does) and it **lights** while the
window is open. The rest ship as **disabled, non-focusable placeholders** —
mirroring how the top menu bar shipped its menu *names* ahead of their entries —
so the bar reads as the reference's persistent toolbar while being honest that
those toggles are not wired. A future floater task flips one `ToolbarTarget`
from `Unlanded` to a real target and wires its branch in
`handle_toolbar_actions`.

The three button looks (enabled / lit / disabled) are painted in Rust from one
`ToolbarButtonVisual` table (`update_toolbar_button_states`), so the skin CSS
(`.sk-toolbar-bar`, `.sk-toolbar-button`) carries only the surface / corner and
does not fight the per-frame state paint — the same split the menu bar uses for
its Rust-painted hover.

Beyond the button bar, this task owns the **bottom-area layout host**: a
`BottomArea { area, upper, bar }` resource is published so the neighbour
bottom-edge controls (nearby chat bar, volume, voice, quick preferences — each
its own task) parent themselves into `upper`, above the buttons, regardless of
the order they land in. (Its fields carry an `#[expect(dead_code)]` reminder
until the first neighbour consumes them.) The button bar's "Conversations"
toggle opens the chat *window*; it is distinct from the always-visible
nearby-chat *input* bar that will live in the upper stack.

Registered as a swept element (`spawn_bottom_toolbar_specimen`, id
`bottom-toolbar`) so the gallery / harness check the enabled / lit / disabled
layouts across every script, size and direction; localized via
`bottom-toolbar-*` Fluent keys. Buttons are **text-labelled**, not iconned —
fully localizable and swept, with icons left as a future refinement.

The pre-existing debug chat overlay (`src/chat.rs`) was lifted off the bottom
edge (`CHAT_BOTTOM_INSET`) so it clears the new bar — a fixed clearance, since
that overlay is a placeholder the nearby-chat bar will replace.
