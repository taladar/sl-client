---
id: viewer-social-groups-activate-none
title: The group list offers no way to wear no group (and so no title)
topic: viewer
status: done
origin: noticed while live-checking the fixed Groups pane on the local grid
  (2026-09-12)
refs: [viewer-social-groups, viewer-groups-pane-empty-on-opensim]
---

Context: [context/viewer.md](../context/viewer.md).

Every row of the Groups pane activated *a* group. Nothing anywhere in the
viewer sent `Command::ActivateGroup(None)` — the wire's "wear nothing" — so
once a group was worn there was no way back to no active group and no title.
The `Option` had been in the command all along; no caller ever passed `None`.

## The reference's shape

`LLGroupList` (`mShowNone`) adds a **"none" row at the top** of the list
whenever the agent is in at least one group — suppressed for a member of
nothing, where it would have nothing to switch away from. Its
`onContextMenuItemEnable` then splits the actions: Info, IM, Leave and Call
need a *real* group (`real_group_selected`), while **every** row including
"none" can be activated, except the one already worn.

## What was built

- [`GroupChoice`](../../sl-viewer-world-api/src/lib.rs) — `NoGroup` or
  `Group(GroupKey)`, carried by `GroupRow` in place of a bare key. An enum
  rather than the reference's null-UUID sentinel, so "wear nothing" and "wear
  the group whose id is nil" cannot be confused, and so each action that needs
  a real group has to say so in its own signature instead of remembering to
  test for a magic id.
- `GroupsModel::ordered()` prepends the row for a non-empty list and marks it
  active when no group is worn — "no title" reads as a state with a marker on
  it, not as an absence. Its `name` stays empty: the label is a localised
  string (`groups-none`) the UI supplies, keeping presentation out of the model.
- `action_enabled` is the single predicate behind **both** the buttons' greying
  and their press refusal. Bevy's `InteractionDisabled` is advisory (see
  [[sl-client-widget-interaction-disabled]]), so a greyed button that still
  fired would be a lie; routing both through one function makes greyed mean
  inert by construction. A row double-click no longer tries to open an IM for
  the "none" row either.

## Worth knowing

This introduced a **disabled look** for these action buttons
(`ACTION_DISABLED_BACKGROUND` plus a dim label), which neither this pane nor
the Friends pane beside it had. The Friends column was given the same treatment
straight after, in [[viewer-social-friends-disable-unusable-actions]], from the
same predicate-shared-with-the-refusal shape.

## How it was verified

Unit: the none row leads a non-empty list and is absent from an empty one; the
active mark moves to it when nothing is worn; `Activate` on it produces
`ActivateGroup(None)` while Info / IM / Leave produce nothing; and the
enablement table matches the reference's for every row / action pair. Each half
of the earlier list fix was disabled in turn to confirm its test failed without
it.

Live-verified on the local OpenSim grid: the `(none)` row leads the list,
activating it clears the title and moves the active mark to it, and the action
column greys exactly what each row cannot do.
