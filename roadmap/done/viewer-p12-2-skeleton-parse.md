---
id: viewer-p12-2
title: Skeleton parse
topic: viewer
status: done
origin: VIEWER_ROADMAP.md — Phase 12 — `sl-avatar`: skeleton & base body (pure crate)
---

Context: [context/viewer.md](../context/viewer.md).

**P12.2. Skeleton parse.** `skeleton.rs`: parse `avatar_skeleton.xml`
(from `&str`) → `Skeleton { joints }` with hierarchy, rest pos/rot/scale,
pivot, and collision volumes; plus the attachment-point→joint map and HUD-
point set from `avatar_lad.xml` `<attachment_point>`. Accessor helpers over
indices (restriction lints). Committed minimal fixture skeleton for tests.
