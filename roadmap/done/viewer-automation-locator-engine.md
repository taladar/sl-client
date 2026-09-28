---
id: viewer-automation-locator-engine
title: The locator engine — strict resolution and actionability
topic: viewer
status: done
origin: viewer automation design (2026-09-28)
points: 8
blocked_by: [viewer-automation-semantic-ui-model,
  viewer-automation-semantic-custom-widgets]
refs: [test-crosscheck-ui-scenes]
---

## Done (2026-09-28)

The engine lives in `sl-viewer-automation`, over the semantic model:

- **Resolution** (`find_all`, `find_one`). `within` resolves first and
  strictly: the scope must be exactly one node, and only its descendants
  are searched. Matches come in reading order (depth first, roots back to
  front), then `nth` picks one. `find_one` is an action's resolution: none
  is `NotFound`, several is `Ambiguous` with every candidate (children
  stripped). The proto's `Ambiguous` message now lists them too, through a
  new one-line `Display` for `UiNode` (role, name, key, test id, box).
- **`Pursuit`**: one action's wait, polled once a frame by its caller.
  Each poll snapshots the UI and resolves strictly; several matches fail at
  once. With one match the checks run in `ActionabilityCheck` order and
  the first failure is reported: `Visible` (hidden or clipped), `InViewport`,
  `Stable` (the same bounds in two consecutive polls), `Enabled`,
  `Editable`, and `ReceivesEvents` (the model's covered test). An
  `Intent` picks the checks: `Hover` skips `Enabled`, and `Fill` adds
  `Editable`. A `Fill` on something that is not a text field fails at once
  with `NotActionable`, since that can never change. A missing scope waits
  as `Attached`. Deadlines are per pursuit: 600 frames or 10 s by default.
  A timeout carries the failing check, but only when there was one node to
  check, plus the last nodes seen.
- **Aim point**: `UiModel::aim_point` returns the centre of the node's
  *visible* part, its box cut by the scroll clip and the viewport. It is
  the same point the covered test is made at.
- **Reveals**:
  - `scroll_into_view` scrolls the innermost scroll area or virtual list
    that hides the node just far enough to show it. It writes
    `ScrollPosition` or `VirtualList::scroll_by`, as a browser driver's
    `scrollIntoView` does, rather than sending wheel input.
  - When nothing matches, the pursuit pages the virtual lists at or under
    the scope, one viewport per step, each from its top. A list searched
    to its end without a match is put back where it was.
  - `open_floater(id)` sets the window's `UiPanelShown`, exactly as
    `SL_VIEWER_OPEN_FLOATER` does (`FloaterOp` has no open), and returns
    the window's locator.
- **`Route`**: a sequence of gestures whose nodes are each pursued. It
  says what to do where; the caller makes the gesture through the real
  input path.
  - `menu_path(keys)`: click the bar menu, hover each submenu line, click
    the entry. It works by Fluent key, so it is locale-independent.
  - `select_option(combo, option)`: open the combo, click the option
    inside it.
  - `pie_slice(slice)`: click a slice within the open pie (the new
    `PIE_MENU_NAME`).
  - A step whose node is already `Expanded` is skipped, so an open menu is
    not clicked shut.

Decisions worth knowing:

- **Virtual-list strictness covers the bound rows only.** A second match
  far down an unscrolled list is not seen, just as a browser driver does
  not see rows a virtualised table has not rendered. The alternative,
  paging every list to its end before every action, costs about three
  frames per page: 50 s for a 10 000-item inventory.
- There is no separate "visible text" criterion. A node's name is its
  visible text (a text node's content, a row's joined text), so `named` /
  `name_containing` is that locator.
- `menu_path` starts at the menu bar. A context menu that is already open
  is reached by pursuing its entries directly.

Tests:

- **Engine teeth** (`sl-viewer-automation`, 16 new, in an
  `InteractionTest` app, clicked through the real pointer at the aim):
  - covered, then uncovered;
  - disabled through an ancestor, still hoverable;
  - still moving (a system nudges it each frame), then at rest;
  - scrolled out of a scroll area, then scrolled to and clicked;
  - clipped with no way to scroll, which times out on `visible`;
  - aimed into the visible 10 px of a half-clipped row;
  - off screen;
  - attached late, and never;
  - ambiguous, failing at once with both candidates and nothing clicked;
  - read-only, and a `Fill` on a button;
  - item 400 of a 500-row virtual list paged to and clicked by name;
  - a missing row paged to the list's end, with the list put back.
- **Resolution units**: reading order, nested scopes, `nth`, an ambiguous
  or missing scope.
- **Viewer** (`automation_locator.rs`), each through the real pointer:
  - a menu path through a submenu fires its action;
  - an already open bar menu is not clicked shut;
  - a disabled entry times out on `enabled` with no action fired;
  - a combo's option is picked by name;
  - a pie slice is chosen by key;
  - a closed floater is opened by id.

Context: [context/automation.md](../context/automation.md).

A snapshot says what is there; a locator engine says *which* node a test
means and whether it can be acted on yet. This is where Playwright's
reliability comes from, and where hand-written tests most often flake.

## Wanted

- Resolve a `Locator` against the semantic model: role + name / name key /
  test id / text, `within` scoping, `nth`, state filters.
- **Strictness**: an action on a locator matching several nodes fails with
  the candidates listed (role, name, test id, bounds); a query may ask for
  all matches explicitly.
- **Actionability**, each check reported by name when it fails: attached,
  visible, inside the viewport, stable (bounds unchanged for N frames),
  enabled (no disabled ancestor), receives events (the hit test at the aim
  point lands on the node or a descendant).
- **Making things actionable** where a user would: scroll a scroll area or
  virtual list until the row is in view (virtual lists must bind the row
  first), open a floater by id (`FloaterCommand`, as
  `SL_VIEWER_OPEN_FLOATER` does), walk a menu path, open a combo, and pick a
  pie slice by label once a pie is open.
- Aim point: the centre of the visible part of the node, not of its box.

Acceptance: teeth tests for each actionability check (a covered button, a
disabled one, one scrolled out of a list, one still animating in); a
virtual-list row far down a long list is scrolled to and clicked by name;
an ambiguous locator's error lists every candidate.
