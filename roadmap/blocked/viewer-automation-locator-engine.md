---
id: viewer-automation-locator-engine
title: The locator engine — strict resolution and actionability
topic: viewer
status: blocked
origin: viewer automation design (2026-09-28)
points: 8
blocked_by: [viewer-automation-semantic-ui-model,
  viewer-automation-semantic-custom-widgets]
refs: [test-crosscheck-ui-scenes]
---

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
