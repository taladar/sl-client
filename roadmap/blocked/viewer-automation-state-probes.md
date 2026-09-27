---
id: viewer-automation-state-probes
title: State probes — chat, notifications, agent, logs and screenshots
topic: viewer
status: blocked
origin: viewer automation design (2026-09-28)
points: 8
blocked_by: [viewer-automation-protocol, viewer-automation-windowless-mode]
refs: [viewer-fake-grid-render-harness, viewer-floaters-decoupled-from-the-session]
---

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
