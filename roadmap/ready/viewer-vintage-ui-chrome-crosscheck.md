---
id: viewer-vintage-ui-chrome-crosscheck
title: Measure skin fidelity instead of arguing about it
topic: viewer
status: ready
origin: Vintage skin fidelity audit (2026-09-20)
points: 3
refs: [viewer-vintage-skin, test-firestorm-crosscheck-runner,
       test-firestorm-crosscheck-report, test-firestorm-harness-skin-selection]
---

Context: [context/viewer.md](../context/viewer.md),
[context/vintage-skin.md](../context/vintage-skin.md).

Every other task in this group is a claim about how two viewers look side by
side, and right now the only way to check one is to run both and squint.
`sl-crosscheck` already runs both viewers against an in-process fake grid with
one capture block, and `SL_VIEWER_CAPTURE_UI=1` already makes both capture
their chrome. Two things stand between that and a usable skin measurement.

**The reference run wears the wrong skin** — it comes up in Firestorm's
default skin every time, so a comparison against "the reference's Vintage"
measures nothing. Done, in [[test-firestorm-harness-skin-selection]]: the
fork's harness takes a skin, and `sl-crosscheck` grows **per-viewer** options
(`--sl-client-skin` / `--firestorm-skin` and their themes) rather than one
shared flag, because the two viewers' skin namespaces are unrelated.

**A UI capture resizes the window, and only on that side.** The reference's
snapshot path cannot draw the UI at any size but the window's, so
`SL_VIEWER_CAPTURE_UI=1` makes its harness reshape the window to the capture
size and warn if the compositor declines; ours lays the UI out at the capture
size directly. A UI cross-check is therefore the one run where a tiling
compositor can silently make the two frames incomparable. The run should read
back what it got and say so, rather than producing a pair that differs by a
scale factor nobody notices.

Also worth stating plainly in the task's output, because it bounds what this
can measure: **the harness closes every floater and blocks them for the run**,
so a UI capture is a comparison of *chrome* — menu bar, toolbars, chat bar,
status row — and not of floater rendering. A notification popping between two
frames would otherwise make a sequence incomparable, which is why the block
exists and why it should stay.

## Found and fixed, 2026-09-20

A real run — `--only firestorm --firestorm-skin vintage --capture-ui` — put
this beyond theory, and it was worse than the description above. The frames
held 1024×738 of content in the bottom-left of a 1920×1080 image, and by the
second frame the same grab stitched across it twice. `harness-status.json`
said `{"ok":true,"reason":"complete"}` and the log said `window resized to
1920x1080`. **Every UI capture ever taken on this machine was like that.**

The window had never been resized at all. `LLViewerWindow::reshape()` is the
*inbound* notification — what the window system calls when a window has
already changed — and it only updates `mWindowRectRaw` and re-lays the UI. It
never touches `mWindow`, so the harness had been telling the viewer's
internals a size the real window did not have: the UI laid itself out for a
window that did not exist while the snapshot grabbed the one that did.
`LLWindow::setSize` is the request.

The guard written to catch a refusing window manager could not fire either. It
compared the request against `LLViewerWindow`'s own rect — which the request
had just set — so it always took the success branch and the `LL_WARNS` below
it was unreachable.

Both fixed on the fork's `test-harness` branch: the harness asks
`LLWindow::setSize`, and `verifyWindowSize()` reads `LLWindow::getSize` at the
settle-to-capture transition (a resize is a round trip, so asking and checking
in one breath can only confirm what was asked). A mismatch now fails the run
through a `window_size` block of requested / honoured / detail, on the same
pattern as the day-position pin. The same run now produces two full
1920×1080 frames with the whole interface in them.

The compositor honours the resize once it is actually asked, so no
window-rule workaround is needed here — the `detail` field is for the
machine where it is not.

## What to do

Both of the original items are done (see above). What is left:

- A recorded chrome-capture pair per skin under the cross-check's run
  directory, the way the scene dumps are recorded.
- The pair itself, which waits on there being a skin of ours worth comparing —
  [[viewer-vintage-skin]].

## Done when

`sl-crosscheck --sl-client-skin <ours> --firestorm-skin vintage --capture-ui`
produces a chrome pair from both viewers at the same size, or fails saying
why, and the pair is what the Vintage-alike skin's fidelity is judged
against.
