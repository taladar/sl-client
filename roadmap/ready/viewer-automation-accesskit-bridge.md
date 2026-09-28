---
id: viewer-automation-accesskit-bridge
title: Feed AccessKit from the semantic UI model
topic: viewer
status: ready
origin: viewer automation design (2026-09-28)
points: 8
blocked_by: [viewer-automation-semantic-custom-widgets]
refs: [viewer-a11y-screen-reader]
---

Context: [context/automation.md](../context/automation.md).

The semantic model and a screen-reader tree are the same information: role,
name, state, value, bounds. [[viewer-a11y-screen-reader]] plans to build the
latter; building it *from* the former means one audit of names and roles
serves both, and the automation sweep keeps the accessibility tree
complete for free.

## Wanted

- Emit `AccessibilityNode`s (role, label, disabled, toggled, expanded,
  selected, value, numeric range) from the model for every UI node; keep
  them updated incrementally rather than re-snapshotting every frame.
- Enable the Linux AT-SPI adapter (the cfg-guarded `accesskit_unix` stanza
  described in [[viewer-a11y-screen-reader]]).
- AccessKit exists only under a winit window: headless runs have none, and
  nothing in the automation path may read from it.
- This covers scope items 1–3 of [[viewer-a11y-screen-reader]] (adapter,
  label convention, custom-widget nodes); live regions, action handling and
  accessible text input stay there.

Acceptance: Orca under niri reads the menu bar, a floater's title and a
disabled button as disabled; a unit test checks the emitted nodes against
the model for a registry floater.
