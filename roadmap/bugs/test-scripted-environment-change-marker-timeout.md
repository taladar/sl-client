---
id: test-scripted-environment-change-marker-timeout
title: The scripted environment change's `night` marker never arrived, once, in a full suite run
topic: test
status: bugs
origin: gridspec-neighbours-crossing (2026-10-07)
refs: [gridspec-neighbours-crossing, gridspec-terrain,
  test-e2e-ground-double-click-fails-under-suite-load]
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

## Again, in its sibling (2026-10-08)

`a_scripted_move_puts_the_object_where_the_script_said`, which cues its
timeline the same way, failed in two of the three full-suite runs of
[[gridspec-terrain]]'s commit with `timed out waiting for the moved marker`,
after 111 s and 96 s, and passed the third in 34.9 s. It takes about 7 s
alone (eight runs of eight), with the rest of its module, and alone under
the hook's own package list and target directory; its siblings took 10 to
16 s in the failing runs.

What those runs add:

- The test allows 60 s for the marker, so everything before the cue took 36
  to 51 s: longer than the 30 s `At::OnEvent` waits (`EVENT_WAIT_TIMEOUT`)
  before it abandons the script. That wait starts when the timeline does,
  at arrival, not at the cue — so a slow enough start loses the script
  before the test has said anything. This fits the 2026-10-07 failure too.
- The abandonment logs a warning, and the report's last warnings held none
  — nor one about lagged events. Whether the harness's log capture shows
  the fake grid's warnings at all was not checked, so this rules nothing
  out yet.
- A Second-Life-flavoured fake grid gives up an unacknowledged reliable
  packet after four sends a second apart, which would lose a marker for
  good while a viewer is stalled. That was silent; the driver now warns on
  every `ReliableGiveUp`. The passing run logged none.

Both failing runs had an earlier failure in another test, since fixed
([[test-fake-grid-tests-stop-reading-client-events]]).
