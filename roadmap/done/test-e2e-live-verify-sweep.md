---
id: test-e2e-live-verify-sweep
title: Turn pending live-verify checks into end-to-end tests
topic: test
status: done
origin: viewer automation design (2026-09-28)
points: 5
blocked_by: [test-e2e-pilot-suite]
refs: [test-e2e-sweep-relog, test-e2e-sweep-environment, test-e2e-sweep-rlv,
  test-e2e-sweep-single-viewer-ui, test-e2e-sweep-two-avatars,
  test-e2e-sweep-live-grid, server-fake-grid-agent-avatars-shared,
  server-fake-grid-im-relay, server-fake-grid-object-undo]
---

Context: [context/automation.md](../context/automation.md).

Several finished features are unit-verified but still carry a "check it
live" step that only a person can do today — interactive checks in the
viewer, UI states after a real login, behaviour between two avatars. Once
the pilot suite shows the mechanism holds, sweep those pending checks: each
becomes an end-to-end test (fake grid where it can, live grid where it
must), and the "pending live verify" note on the feature is retired.

## The list (gathered 2026-09-30)

A sweep of every status directory and the context files for a check still owed
found **78** items, and the working notes kept outside the roadmap added five
more (the colours and debug-settings tabs, About Landmark, the asset retry's
recovery path, the async fetchers' throughput). They sort into four kinds, and
each kind became its own task, sized by what the tests need rather than by the
item count:

| Kind | Task | Points |
| --- | --- | --- |
| a relog in the middle of a test | [[test-e2e-sweep-relog]] | 5 |
| the environment editors and their gates | [[test-e2e-sweep-environment]] | 8 |
| the RLVa console and windows | [[test-e2e-sweep-rlv]] | 5 |
| the rest of the single-viewer UI | [[test-e2e-sweep-single-viewer-ui]] | 8 |
| two avatars watching each other | [[test-e2e-sweep-two-avatars]] | 8 |
| what only a live grid has | [[test-e2e-sweep-live-grid]] | 13 |

Each task lists its items and the file that owes the check.

**Not tests, and staying with a person** — visual judgement and
performance measurement, where there is no pass/fail a test could state:
[[viewer-realtime-mirrors]]' reflective surface,
[[viewer-water-surface-fog-fallback-flat]]'s look from above,
[[viewer-flycam-stop-button-overlaps-chat]]'s gap,
[[viewer-r21]]'s
tiled textures, [[viewer-r23]]
(delegated to [[viewer-r17a]]), [[viewer-parcel-owners-terrain-overlay]]'s
tint, [[viewer-skin-image-backed-widgets]]' bevels at UI scale 1.5, the P31.8
`mTorso` idle sway ([context/viewer.md](../context/viewer.md)),
[[viewer-avatar-complexity-limit]]'s jellydolls and crowd frame time,
[[viewer-render-friends-only]]'s frame-time win, and the perf
re-measurements of [[viewer-perf-gpu-avatar-extract-skins-floor]],
[[viewer-perf-gpu-avatar-keystone-skinuniforms-spike]],
[[viewer-perf-slfaceext-material-reprep]],
[[viewer-perf-ui-layout-per-frame-relayout]] and
[[viewer-perf-async-asset-fetchers]] (the `dl` figure filling the gate on a
busy region).

## The first batch

`sl-client-bevy-viewer/tests/e2e_live_checks.rs`, on the fake grid and both
backends, retired these checks:

- **[[viewer-menu-accelerators-inert]]**: every window chord the menu bar
  draws opens and closes its window (Ctrl+P, Ctrl+I, Ctrl+T, Ctrl+M,
  Ctrl+B, Alt+P, Ctrl+Alt+Shift+S, Ctrl+F); Alt+Shift+F toggles the
  flycam; Ctrl+Z / Ctrl+Y send the selection's `Undo` / `Redo`; Ctrl+L
  links two prims and Ctrl+Shift+L unlinks them; Ctrl+Q logs out and quits.
- **[[viewer-about-floater]]**: the Help menu ticks the window, the tabs
  switch panes, a license row shows its text, Copy to Clipboard pastes the
  support block into the chat bar, and the Region line follows a teleport.
- **[[viewer-do-not-disturb-away]]**: while Unavailable an IM gets the busy
  reply once — not again for the next line of the conversation, which notes
  it — and a teleport offer is held until the mode ends.
- **[[viewer-avatar-radar]]**: a Range header click turns the order round,
  the filter leaves the matching row, and a selected row's Profile and IM
  buttons open the profile and the conversation.

### What the tests found

- **A link that landed was still two linksets.** The prim that became a
  child stayed in the selection, so Link stayed enabled and a second link
  named the child. `can_link` and `link_order` now count and name only the
  selected linkset **roots** (the reference's `getRootObjectCount` /
  `SEND_ONLY_ROOTS`); fixture test
  `a_selected_child_prim_is_not_a_linkset_to_link`.
- **Unlink lost the children.** The module promised that unlink leaves the
  selection in place so a wrong link can be redone the other way round, but
  any tool-state change (opening the Build menu is one) folds a selected
  child into its root, so after an unlink only the old root was selected.
  `PendingDelink` remembers the prims a delink takes out of a selected
  linkset and `reselect_delinked` adds each to the selection when it lands
  as a root, as the reference keeps every prim of a linkset selected;
  `ctrl_shift_l_unlinks_the_selected_linkset_and_keeps_both_prims_selected`.
- **The fake grid left child agents behind on a logout.** A viewer sends
  `LogoutRequest` to its root region only; the grid must close the child
  agents it opened next door (OpenSim's `CloseChildAgents`). A teleport
  followed quickly by a logout left the region it had just announced as a
  neighbour holding a session, which the stage's teardown reports as
  stranded. `retire_children_on_logout`;
  `a_logout_retires_the_neighbours_child_sessions`.

### What the batch added

- `WorldAction::ShiftSelect` / `WorldHandle::shift_select`: a Shift+click
  in build mode, which toggles the target in the selection and keeps the
  rest — how a test selects a second prim to link. Fixture test
  `a_shift_select_toggles_a_prim_and_keeps_the_rest`.
- `Stage::expect_quit`: the body announces that a viewer will quit by
  itself (a Quit chord), and the teardown holds it to a clean exit instead
  of asking it to log out.

### Left for the fake grid

Three checks assert the wire rather than the result, because the fake grid
does not do the result yet: the undo / redo test checks the grid hears the
`Undo` / `Redo` for the selected prim ([[server-fake-grid-object-undo]]),
the busy reply is checked at the grid rather than in a second viewer's
conversation ([[server-fake-grid-im-relay]]), and the radar is tested on the
catalogue NPCs because two stage viewers do not see each other's avatars
([[server-fake-grid-agent-avatars-shared]]).
