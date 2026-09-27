---
id: viewer-automation-executor
title: AutomationPlugin — the in-viewer request executor
topic: viewer
status: blocked
origin: viewer automation design (2026-09-28)
points: 8
blocked_by: [viewer-automation-locator-engine, viewer-automation-synthetic-input,
  viewer-automation-world-aim, viewer-automation-state-probes]
refs: [viewer-automation-remote-transport, viewer-automation-inprocess-transport]
---

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

Acceptance: an App-level test drives a login, opens a floater, clicks a
button and waits for its effect entirely through requests; each failure
kind produces its documented diagnostics.
