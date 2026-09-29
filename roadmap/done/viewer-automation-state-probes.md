---
id: viewer-automation-state-probes
title: State probes — chat, notifications, agent, logs and screenshots
topic: viewer
status: done
origin: viewer automation design (2026-09-28)
points: 8
blocked_by: [viewer-automation-protocol, viewer-automation-windowless-mode]
refs: [viewer-fake-grid-render-harness, viewer-floaters-decoupled-from-the-session]
---

## Done (2026-09-29)

**Protocol** (`sl-automation-proto/src/probe.rs`), the readouts:
`ConversationReadout` (by `ConversationRef`: nearby, direct, group,
conference; unread, pending invite, `TranscriptLine`s with speaker, speaker
id and kind, `ChatKind`, text, own), `NotificationReadout` (id, template,
text, `OfferedButton`s, live, response), `StatusReadout` (region, parcel,
balance, SLT `ClockTime`), `AgentReadout` (agent id, `RegionReadout`,
position, seat, `TeleportReadout`, `CameraView`), `SelectedObject`,
`InventoryFolderReadout` / `InventoryEntry` under an `InventoryRoot`,
`LogPage<T>` of `LogEntry` (seq, `LogStream`, kind, detail) with `next` and
`dropped`, `DiagnosticsReadout` (warning and error totals, `DiagnosticLine`s)
and `QuiescenceReadout::is_quiet`.

**Viewer** (`sl-viewer-automation`):

- Readers `read_agent`, `read_status`, `read_conversations`,
  `read_notifications`, `read_selection`, `read_inventory` (exact names per
  segment; `ProbeError::NoSuchFolder` names the first miss),
  `read_quiescence`, `read_diagnostics`. Computed when asked.
- `ProbeSources`: `fn(&mut World)` readers for the models in heavy crates,
  registered by `sl-client-bevy-viewer/src/automation_sources.rs` in the
  assembly — `conversation_readouts` (people), `live_notifications`
  (notices), `teleport_readout` (places), the status bar's balance and
  `SceneQuiescence`'s outstanding work. The automation crate gained only
  `sl-viewer-inventory`, `sl-viewer-notifications`, `tracing(-subscriber)`
  and `image`.
- `EventLog` (+ `EventLogPlugin`, in `Last`): every `SlEvent`, `SlCommand`
  and `UiAction` under one counter, 4096 kept as clones, printed on read
  (kind = variant name, detail cut at 2048 chars); `read(cursor, streams,
  limit)`.
- `LogTally` / `LogTallyLayer`: warning and error counts plus the last 256
  lines, read by cursor. `init_tracing` installs `LogTally::global()`; a test
  names its own with `DiagnosticsSource`.
- `PipelineStatus` / `PipelineStatusPlugin` moved here from the test-only
  readback rig (which now uses them).
- `request_screenshot` / `take_screenshot`: the primary window as a
  `CapturedFrame` (RGBA, `to_png`), a locator's boxes outlined in magenta at
  the window's scale factor.
- `StateProbesPlugin` installs the recorders; the full-stack harness now
  uses it. Installing it in a running viewer is the executor's.

**Models made readable:** the conversation model's lines keep their
`ChatType`; every toast card declares its buttons (`ToastButtons` in
`sl-viewer-notifications`, via `ToastSpec::buttons` for the bespoke cards —
script dialogs, script and experience permissions, load URL, offers, group
notices); `NotificationId::get`; `AgentBalance::linden_dollars`.

**Bug found by the full-stack test, fixed:** after a *distant* teleport the
destination region was never named — the status bar stayed on
"Connecting…". The destination's `RegionHandshake` arrives on its circuit
while it is still a child, before the handover commits, and the session had
not recorded that circuit's region, so the identity carried handle 0 and
`maintain_world` pinned it on the region being left (then cleared it with the
world reset). `Session::begin_handover` now notes the destination region on
the handover circuit, and `maintain_world` keeps a handshake for a region
with no entity until one is spawned. Tests:
`handover_destination_handshake_names_the_destination` (sl-proto, fails
without the fix) and `a_teleport_destination_is_named_by_its_early_handshake`
(sl-client-bevy).

**Decisions:**

- **No request variants yet**, as with the world model: the probe requests
  land with the executor, their first consumer (noted there), the driver
  methods with the driver.
- **A heard chat line has no channel** on the wire (`ChatFromSimulator`
  carries none); the readout names how it was said (`ChatKind`), and the
  channel of what the own agent said is its `Chat` command in the event log.
- The event log orders a frame's messages events, then commands, then UI
  actions — recording order, not causal order within a frame.

**Tests:** proto round-trips and `is_quiet`; unit tests for the event log
(burst order, limit + filter continuation, dropped marker, truncation), the
tally, the screenshot outline and PNG, the conversation readout (recall band
excluded, echoed own chat is own) and the teleport readout; a notices test
that a live card reports the buttons it declared; fixture tests
(`sl-client-bevy-viewer/src/automation_probes.rs`) that the agent, status
balance, selection, inventory path, notifications, quiescence and event log
change with their state; and one full-stack test on the fake grid: login,
quiet, agent and status, an object's shout / an IM / a modal alert reaching
the transcripts, a live notification whose default button is one semantic
node, the event log in order, a 640×360 window screenshot with its outline,
and a distant teleport walking Idle → InProgress → Arriving → Succeeded into
the named far region.

Context: [context/automation.md](../context/automation.md).

Much of what a test asserts is not one widget: "the other viewer heard
this", "a notification offered these buttons", "I am now in region East",
"the viewer sent exactly one ObjectUpdate", "nothing logged an ERROR".
These are read from models the viewer already keeps.

## Wanted

- Transcripts: local chat (sender, type, channel, text) and IM sessions,
  from the conversation model rather than scraped from text widgets.
- Notifications and toasts: text and the buttons offered (the buttons are
  also semantic nodes, so a test can answer one with a locator).
- Status bar values: region, parcel, balance, time.
- Own agent: region, region-local position, teleport phase, sit state,
  camera mode.
- Current selection; inventory by folder path.
- **Sequence-numbered logs** of `SlEvent` summaries, outbound `SlCommand`s
  and `UiAction`s, read by cursor (like `ViewerHarness`'s `Recorded`), so a
  reader never misses an entry; bounded with an explicit "dropped" marker.
- A WARN / ERROR log counter with the recent lines.
- Quiescence: asset in-flight counts (`scene_is_quiet`) and render settle.
- Screenshot of the capture target with UI, optionally overlaid with the
  semantic boxes of a locator's matches.

Acceptance: each probe has a test that it changes when the state does; a
cursor read after a burst of events returns all of them in order.
