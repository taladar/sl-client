---
id: viewer-p35-1
title: Detect HUD
topic: viewer
status: ready
origin: VIEWER_ROADMAP.md — Phase 35 — HUD attachments
---

Context: [context/viewer.md](../context/viewer.md).

**P35.1. Detect HUD.** Classify an attachment whose `attachment_point()`
is a HUD slot (31–38, `HudCenter` / `HudTopLeft` / …); route it out of the
world scene to a dedicated screen-space HUD layer, and only for the **agent's
own** attachments.
