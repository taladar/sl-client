---
id: viewer-automation-executor
title: AutomationPlugin — the in-viewer request executor
topic: viewer
status: done
origin: viewer automation design (2026-09-28)
points: 8
blocked_by: [viewer-automation-locator-engine, viewer-automation-synthetic-input,
  viewer-automation-world-aim, viewer-automation-state-probes]
refs: [viewer-automation-remote-transport, viewer-automation-inprocess-transport]
---

## Done (2026-09-29)

**Protocol** (`sl-automation-proto`): the requests their first consumer
needed — `hover`, `press` (a key or chord: `Ctrl+Shift+S`), `menu_path`,
`select_option`, `pie_slice`, `open_floater`, a `deadline` on every UI action,
`WaitCondition::Text`; the world ones `find_world`, `wait_for_world`
(`WorldWaitCondition`), `world_action` (`WorldAction`: click, right click,
hover, select, drop from a UI node; `reveal`), `drag_handle` (handle slug,
`DragAmount`, `SnapSide`, `DragModifiers`) and `sweep`; the state ones `read`
(a `Probe` → `ProbeReadout`), `read_log`, `read_diagnostics`,
`wait_for_state` (`StateCondition`: quiet, a probe's JSON at a pointer passing
a `ValueTest`, a log entry after a cursor → `StateObservation`) and
`screenshot`. New errors: `FillMismatch`, `StateTimedOut` (with the last
observation), `InventoryFolderNotFound`, `Unavailable`, `InvalidRequest`,
`ScreenshotFailed`. Every error `Response` carries a `FailureReport`.

**Executor** (`sl-viewer-automation/src/executor*`): `AutomationPlugin`
(installs `StateProbesPlugin` and `WorldModelPlugin` unless present) and the
`AutomationQueue` (`submit`, `take_response`, `drain_responses`); one
exclusive system in `Last` after the event log (`AutomationSystems`). UI
actions pursue, play through `SyntheticInput`, wait for the frames; a fill
confirms the field holds the text. World actions run `WorldAim` (a drop first
pursues its UI source), drags `ManipulatorDrag`, the band `WorldSweep` and
then waits for the selection to be exactly its targets. Requests that play
input take turns in submission order; reads and waits run alongside. A
duplicate id in flight is refused. The viewer installs it with
`ViewerAppOptions::automation` (off by default); the full-stack harness sets
it instead of adding the recorders by hand.

**Decisions:** a screenshot travels as a file at an absolute path the request
names, not as bytes (same machine either way; megabytes would clog the
channel). The failure report's tree is the scope's subtree (depth 4), else the
top of the tree (depth 3), capped at 300 nodes; 32 event-log entries; the
warnings logged since the request started (`LogTally::cursor`,
`diagnostics_cursor`). A covered node counts as visible for a wait.

**Tests:** proto round-trips and wire shapes for every new variant, the
`includes` subset, `ValueTest`, the log filter; key-chord parsing and handle
slugs; executor teeth over an `InteractionTest` app (click, fill, key press,
two clicks taking turns beside a pending wait, ambiguity with scope excerpt
and event tail, not-actionable, timeouts with last observation, the
diagnostics logged while a request ran, probe and log state waits, refusals);
fixture-world requests (find, wait, a covered click without reveal naming the
wall, with reveal touching the target; a handle drag; a sweep); and the
full-stack acceptance `a_session_is_driven_through_requests_alone` — login
waited for through the agent probe and quiet, the inventory floater opened by
its toolbar button, a UI action in the log, a screenshot with the window
outlined, the close button clicked and the window waited hidden.

Context: [context/automation.md](../context/automation.md).

The model, the locators, the input queue and the probes need one owner that
takes a `Request`, does it across however many frames it takes, and answers
with a `Response` — independent of how the request arrived.

## Wanted

- `AutomationPlugin` in `sl-viewer-automation`: a request queue resource
  with ids; a system that advances each request per frame (resolve →
  actionability → enqueue input → wait for its frames → confirm).
- In-viewer waits: a predicate over the model (locator matches / state /
  text / probe value / event seen after cursor / quiet) evaluated each
  frame, with a frame deadline and a wall deadline; the last observed value
  is returned on timeout.
- Diagnostics on every failure: the failing check, candidates, a semantic
  tree excerpt around the scope, and the event tail.
- Several requests may be in flight (a wait on one viewer while another
  acts), each answered independently.
- Installed only when automation is requested — a runtime switch, never a
  Cargo feature (a feature would double the `cargo hack` powerset); no cost
  otherwise.
- World requests land here, their first consumer
  ([[viewer-automation-world-model]] built the model without them): a
  `find_world` answered from `WorldQuery` with `WorldWant::All`, a
  `wait_for_world`, and `WorldModelPlugin` installed with the executor so
  object names are collected. World *actions* too
  ([[viewer-automation-world-aim]]): a `WorldAim` per request, polled each
  frame, whose `WorldTarget::input()` goes to the synthetic input; its
  `AimStage` is the progress to report and `WorldNotActionable` /
  `WorldTimedOut` (with `failed_check`) the failures. Likewise a
  `ManipulatorDrag` (handle, amount, snap regime, held keys — it plays its
  own input) and a `WorldSweep` (a rubber band that selects exactly its
  targets).
- The state probes land here as requests, their first consumer
  ([[viewer-automation-state-probes]] built the readers and the readouts
  without them): agent, status, conversations, notifications, selection,
  inventory by path, quiescence (a `quiet` wait condition), event-log and
  diagnostics reads by cursor (an "event seen after cursor" wait), and a
  screenshot with a locator's matches outlined (`request_screenshot` /
  `take_screenshot`; decide here whether the frame travels as PNG bytes or a
  path the viewer wrote). `StateProbesPlugin` is installed with the executor;
  `ProbeSources` and the global `LogTally` layer are already in every
  viewer.

Acceptance: an App-level test drives a login, opens a floater, clicks a
button and waits for its effect entirely through requests; each failure
kind produces its documented diagnostics.
