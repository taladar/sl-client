---
id: viewer-ui-menu-keyboard-nav
title: Keyboard traversal of an open menu
topic: viewer
status: done
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

---

## Done

Landed in `src/menu.rs` as a `MenuKeyboard` resource (a single writer of the
highlight the hover system already paints) plus the `menu_keyboard_nav` system,
in the self-managed spirit the task asked for — a keyboard-driven highlight
index reusing the existing `MenuEntryAction` dispatch and the same submenu
open/close the mouse path uses, **not** the upstream focus machinery.

**What shipped, against the wanted list:**

- Block-axis arrows (`↓`/`↑`) step the highlight, wrapping and skipping disabled
  entries; `Enter`/`Space` activate; `Escape` closes (already wired).
- Inline-axis arrows follow the writing direction: inline-end opens the
  highlighted submenu (landing on its first entry) or, on a leaf, switches to
  the next top menu at the bar; inline-start closes a submenu back to its
  branch, or steps to the previous top menu at the top.
- **Jump keys** — the underlined mnemonic, active only once keyboard navigation
  has begun (a bevy_text `Underline` toggled on the mnemonic text span). See the
  scope note below on the assignment algorithm.
- The whole open path stays lit (top menu → branch → submenu entry), not just
  the leaf.

**Getting *into* the menu by keyboard (enabling pieces beyond the literal
task):**

- `Tab` to a bar button then `Enter`/`Space`/`↓` opens it (`open_focused`). The
  `Tab`-reachability the task assumed had regressed — a focus-release system was
  clearing bar-button focus every frame the menu was closed; fixed by only
  releasing focus the *menu* captured (mouse / context menu / tap-`Alt`), never
  focus the user placed with `Tab`.
- **tap-`Alt`** opens the primary bar into keyboard navigation — the reference's
  `LLMenuBarGL::checkMenuTrigger`, *not* Windows-style `Alt`+letter (the SL
  viewer has no such thing; `Alt`+letter combos are only explicit per-command
  accelerators). Armed on `Alt` down, disarmed by any other key or by mouse
  motion, so an Alt-drag camera orbit/zoom does not pop the menu.

**Scope notes (reviewed live and accepted):**

- Jump keys use "first free alphanumeric letter of the label per menu"
  (collision-free), not the reference's shared-word `createJumpKeys` algorithm —
  so a specific letter can differ from Firestorm's choice.
- tap-`Alt` opens the first top menu's drop-down immediately; the reference
  highlights the first *closed* top menu and waits for `↓`. Functionally
  equivalent for keyboard entry.

**Two follow-ups filed from the live review:** the keyboard-focused element
draws no visible ring → [[viewer-ui-focus-ring-visible]]; and the inventory gear
menu is clipped to its floater →
[[viewer-ui-inventory-gear-menu-clipped]] (a pre-existing clip bug this task
surfaced, not a regression).

Tests: `src/menu.rs` gains unit coverage of the jump-key assignment and a set of
keyboard-navigation integration tests (open, arrow-step, disabled-skip, jump,
submenu open/close, mnemonic underline) driven through the full widget runtime.
Verified live on the local OpenSim grid.
