---
id: viewer-snapshot-hide-balance
title: Snapshot option — hide the L$ balance
topic: viewer
status: done
origin: user request (2026-07) — follow-up to viewer-snapshot-floater
blocked_by: [viewer-snapshot-floater]
refs: [viewer-snapshot-quick-key]
---

Context: [context/viewer.md](../context/viewer.md).

Add a **hide-L$-balance** option to the snapshot floater
([[viewer-snapshot-floater]]): when a snapshot **includes the UI**
("show interface" on), the status bar's **L$ balance** is baked into the shot,
so a screenshot shared publicly leaks the shooter's balance. The reference
viewer offers a toggle to blank it for the capture — the same privacy default a
photographer expects.

Scope: a persisted checkbox beside the existing include-UI / include-HUD
toggles; when set (and the UI is included), hide the status-bar balance readout
(`status_bar`) for the shot frame only, using the same hide → shoot → restore
window the include-UI / include-HUD toggles already drive
(`snapshot_floater::start_capture` / `drive_capture`). It only matters when the
UI is in the frame, so it is inert with "show interface" off. Applies to the
quick key ([[viewer-snapshot-quick-key]]) too, since both share the capture
path.

Reference (Firestorm, read-only): the snapshot panel's
"Hide L$ balance in snapshots" option (`RenderHideBalanceInSnapshot`).

Builds on: [[viewer-snapshot-floater]] (the capture path and the toggle row),
and the status-bar balance display (`status_bar`).
