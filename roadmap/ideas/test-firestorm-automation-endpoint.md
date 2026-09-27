---
id: test-firestorm-automation-endpoint
title: The patched Firestorm answers the same automation protocol
topic: test
status: ideas
origin: viewer automation design (2026-09-28)
points: 13
blocked_by: [viewer-automation-protocol, viewer-automation-remote-transport]
refs: [test-crosscheck-ui-scenes, test-firestorm-harness-skin-selection]
---

Context: [context/automation.md](../context/automation.md).

[[test-crosscheck-ui-scenes]] needs both viewers put into the same UI state
— a floater open on a subject, a tab chosen, a row scrolled into view — and
concluded that the portable answer is locate-and-act, as a web test
framework does. Our viewer's side is the automation mechanism; this is the
other side.

## Shape

- The `test-harness` branch of the Firestorm fork listens on the same kind
  of socket and answers the same `Request`s, resolving locators against
  its `LLView` tree: XUI `name=` as the test id, `getChildView(name,
  recurse)` as `within`, control classes mapped to roles.
- Selectors stay per-viewer: logical names mapped per side, with a
  per-viewer type so one viewer's selector cannot be handed to the other.
- Only the subset a UI scene needs: open floater, select tab, scroll into
  view, select row, hover, open menu, snapshot.

Promote once [[test-crosscheck-ui-scenes]] has answered its scoping
question in favour of the full locate-and-act layer.
