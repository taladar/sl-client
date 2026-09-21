---
id: test-fake-grid-lsl-offline-cases
title: Move the script and chat conformance cases offline
topic: test
status: blocked
origin: LSL-on-the-fake-grid audit (2026-09-20)
points: 3
blocked_by: [server-fake-grid-scripted-scenario]
refs: [test-audit-fake-grid-conformance-grid, server-world-chat-routing]
---

Context: [context/lsl.md](../context/lsl.md).

The cleanest measure of whether the LSL programme worked. Nine
conformance cases exist today that cannot run against the fake grid, and
every one of them is live-grid-only for a reason this programme removes.

- `script-dialog` — waits for the local OpenSim's `SLClientScriptTester`
  prim to fire `llDialog` on a timer, and records `partial` on Second
  Life because no such fixture exists there;
- `script-permissions` — same fixture, same dependence;
- `script-running` — creates a script in a prim and toggles it; OpenSim
  only, because SL drops the task write;
- `script-upload` — the compile round trip;
- `object-touch-grab` — needs a touchable prim and a simulator that
  routes the grab;
- `chat-self-echo`, `chat-hear-other`, `chat-whisper-shout-range` — need
  [[server-world-chat-routing]];
- `money-transfer` — needs the payment path into a script.

Each of them currently reads its outcome the roundabout way, because a
reply-less command on a live grid has no observer: "the circuit staying
healthy, a keep-alive ping still round-tripping". Against a fake grid
with a real script engine the observation is direct — the case can
assert what the *script* did, not merely that the simulator did not
hang up.

Wanted: each case's `grids()` extended to the fake grid, its fixture
dependence redirected at the `scripts` scenario
([[server-fake-grid-scripted-scenario]]) rather than at a prim in the
local OpenSim's Default Region, its `partial` paths tightened to real
assertions where the fake grid can answer, and its name added to
`fake::OFFLINE_CASES` — which asserts, in both directions, that the list
and the registry agree, and which treats a `partial` run as a failure.

Note the rule the list documents: a case joins it "when the grid can
answer it, not when it stops erroring". So a case that would pass only
by recording `partial` stays out until the tranche it needs lands.

Acceptance: the nine cases run on every `cargo test` against the fake
grid and pass without `partial`; their live-grid variants still run and
still pass; and the OpenSim OAR fixture prim stops being a prerequisite
for anything that runs in CI.
