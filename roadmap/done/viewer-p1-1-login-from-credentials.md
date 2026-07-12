---
id: viewer-p1-1
title: Login from credentials
topic: viewer
status: done
origin: VIEWER_ROADMAP.md — Phase 1 — Viewer shell (window, login, camera, quit)
---

Context: [context/viewer.md](../context/viewer.md).

**P1.1. Login from credentials.** `clap` args `--credentials <path>` /
`--avatar <name>`; load via `Credentials::load().select()`; resolve the grid
from `login_uri` / `grid` (default local `http://127.0.0.1:9000/`); acquire
MFA via `Avatar::acquire_mfa()` + `LoginRequest::with_mfa` when configured.
Build `LoginParams` and add `SlClientPlugin` (mirror `survey_probe.rs`).
