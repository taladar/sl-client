---
id: viewer-vintage-ui-chrome-crosscheck
title: Measure skin fidelity instead of arguing about it
topic: viewer
status: done
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
pattern as the day-position pin.

## Done, 2026-09-27

**The size is the window's, not 1920×1080.** The claim that the compositor
honours the resize once it is actually asked did not survive a paired run.
niri opens the `firestorm-test` window **tiled** on the `sl-client` workspace,
which is on the 4K output (3840×2160 at scale 1.5). It keeps the window at
3840×2160 and ignores the 1920×1080 request. The fork's check caught it
(`WINDOW NOT 1920x1080 — the window is 3840x2160 …`). **On this machine a
chrome pair is captured at `--capture-size 3840x2160`**, the size the
compositor gives the window. The world-only default stays 1920×1080, because
nothing in a world frame depends on the window.

What the runner does now (`sl-crosscheck`):

- Reads Firestorm's `window_size` block. A refused window fails the run, and
  so does a Firestorm that says nothing about its window on a UI run
  (`WINDOW NOT REPORTED`). Ours lays its interface out in the off-screen
  capture target, so its silence is not a gap (`Viewer::ui_follows_window`).
- Reads every frame's pixel size back from the file and fails any frame not at
  the capture size (`FRAMES NOT AT …`). The file is the evidence; a status is
  only a claim.
- States on every UI run that the pair is **chrome only**, because floaters
  are closed and blocked.
- Keeps a pair per skin: a `--capture-ui` run defaults to
  `crosscheck-runs/<scenario>-chrome-<ours>-vs-<theirs>`.
- No longer ends a failed-but-two-sided run with "ready to be compared".

Found on the way: the default run directory was **relative**, and Firestorm's
launcher changes directory before it starts. Every default-directory run
therefore handed Firestorm a `--credentials` path it could not open, and a
`FIRESTORM_X64_USER_DIR` it resolved against `$HOME`, outside the run.
`RunDirs::new` now makes the root absolute. That refusal then sat on a modal
crash dialog until the deadline, which is filed as
[[test-firestorm-harness-refusal-hangs-on-crash-dialog]].

The recorded pair (local, git-ignored):

```sh
sl-crosscheck --scenario catalogue --capture-ui \
  --capture-size 3840x2160 --sl-client-skin vintage --firestorm-skin vintage \
  --run-dir crosscheck-runs/catalogue-chrome-vintage-vs-vintage-4k
```

Both halves: `ok — complete (3/3 frames)`, and Firestorm reported
`window 3840x2160 as asked`. The first thing the pair shows is **scale**. Our
menu bar is 38 px tall and the reference's is 19 px in the same frame, so our
interface is drawn at twice the reference's size. Ours follows the output's
1.5, which is intended, while Firestorm (Xwayland) sees 1.0. Pinning one scale
for a pair is [[test-crosscheck-pin-ui-scale]].
