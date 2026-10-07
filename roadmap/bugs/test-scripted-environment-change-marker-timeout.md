---
id: test-scripted-environment-change-marker-timeout
title: The scripted environment change's `night` marker never arrived, once, in a full suite run
topic: test
status: bugs
origin: gridspec-neighbours-crossing (2026-10-07)
refs: [gridspec-neighbours-crossing]
---

Context: [context/test.md](../context/test.md).

## Observation

`full_stack_test::tests::a_scripted_environment_change_darkens_the_sky_unasked`
failed once inside the commit hook's full `nextest` run (2026-10-07, 5258
tests, the machine busy): `timed out waiting for the 'night' marker`, after
92 s. For the whole wait the session saw only pings, `SimStats` and
`SimulatorTime` — the timeline's three steps (set the environment, save the
region, send the marker) never ran, so its `At::OnEvent` trigger never saw
the chat line `change the sky`. Run alone straight afterwards it passed in
6 s.

Nothing in the change being committed touches a timeline or the chat path
(it adds a draw-distance branch to the per-session neighbour announcer), but
"passes alone" is what a race looks like, and the two candidates were not
told apart:

- the viewer's chat line never reached the grid (the test says the cue
  before the session is ready to send it), or
- the timeline's subscriber missed the `ServerEvent::Chat` — every session
  task reads one `broadcast` channel, and a reader that lags is told how
  many events it lost, not which.

## Wanted

Find which. The failure output held no `the … missed N events` warning and
no record of whether the grid received the chat, so first make the harness
say both on a timeout; then run the test under load until it fails again.
