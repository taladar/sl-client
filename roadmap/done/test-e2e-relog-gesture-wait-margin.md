---
id: test-e2e-relog-gesture-wait-margin
title: e2e_relog's active-gesture test runs at the edge of its 60 s wait
topic: test
status: done
origin: full viewer test run while landing viewer-left-click-click-actions (2026-11-03)
refs: [test-e2e-sweep-relog]
---

Context: [context/automation.md](../context/automation.md).

## Observation

`sl-client-bevy-viewer/tests/e2e_relog.rs`
`an_active_gesture_is_active_after_a_relog` failed in a full release
`cargo nextest run` of the viewer and five library crates (1044 tests in
parallel): after 68.9 s, waiting for `menuitem[name_key=menu-inv-deactivate]`
to be enabled, `TimedOut … frames: 2500, millis: 60016` — the file's `WAIT`
of 60 s. Run alone straight after, it **passed in 58.1 s**.

So the test spends nearly all of its 60 s budget even unloaded, and any
contention pushes it over. Either something in the relog path is far slower
than it needs to be (a fixed delay, a poll interval, the relog itself), or the
wait is waiting on the wrong thing.

## To do

Find where the 58 s goes (the per-step timings of the scenario, against the
other `e2e_relog` cases, which finish well inside their waits) and fix that;
do not just raise `WAIT`.

## Finding (2026-11-20)

The 58 s was never one wait. The test runs four stages: two grid flavours,
each on both backends. Each stage took 10–14 s, and every step in it a second
or two. The failure was a right-click that missed.

`gesture_menu` types the gesture's name into the inventory search and
right-clicks its row. The search box debounces the query by 150 ms
(`QUERY_DEBOUNCE_SECS`), and the inventory probe the test waits on reads the
model, not the rows. So the click found the row where the **unfiltered**
tree drew it (y 411), stable and hit-testable, and aimed there. When the
press landed before the rebuild, the menu was the gesture's. When it landed
after, the row had moved up (y 257), the press hit the list's empty
background, and the root folder's menu opened. Deactivate was not in it, and
the 60 s wait ran out. It failed that way once alone, unloaded, at the start
of this task. The failure screenshot showed New Folder / Replace Current
Outfit over the filtered list.

## Fix

- While a typed query waits out the debounce, the inventory list
  (`rebuild_view`) carries `Busy`. It clears when the rebuild lands.
- The automation model now inherits `busy` from an ancestor, as it inherits
  `disabled` (ARIA's `aria-busy` covers an element's descendants). So a row
  of a busy list is busy, and a click on it waits through the `ready` check.
  The browser view's marker behaves as before.

## Verified

With the fix, all eight right-clicks of a run aimed at the filtered row
(y 257). Before it, every one aimed at y 411. The test passed four runs in
a row unloaded (56–57 s), and once throttled to 15% CPU (419 s, each
click 13–17 s into its step and still on the filtered row). Unit tests: a
row in a busy container holds a click until the container is ready
(`sl-viewer-automation`), and the list is busy for exactly the debounce and
shows the old rows meanwhile (`sl-viewer-inventory`). The book's automation
page says so.
