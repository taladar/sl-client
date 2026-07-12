---
id: viewer-p1-4
title: Quit + draw distance
topic: viewer
status: done
origin: VIEWER_ROADMAP.md — Phase 1 — Viewer shell (window, login, camera, quit)
---

Context: [context/viewer.md](../context/viewer.md).

**P1.4. Quit + draw distance.** `Esc` / `Q` sends
`Command::Logout` then `AppExit::Success`; also exit on `LoggedOut` /
`Disconnected`. On `RegionHandshakeComplete` send
`Command::SetDrawDistance(Distance::new(128.0))` so the sim streams content.
