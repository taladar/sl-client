---
id: viewer-p0-1
title: Create the crate skeletons
topic: viewer
status: done
origin: VIEWER_ROADMAP.md — Phase 0 — Scaffold the three new crates
---

Context: [context/viewer.md](../context/viewer.md).

**P0.1. Create the crate skeletons.** Add `sl-prim/`, `sl-sculpt/`,
`sl-client-bevy-viewer/`, each with a `Cargo.toml` (`edition = "2024"`,
`rust-version = "1.94.0"`, `publish = false`, `[lints] workspace = true`), a
`CHANGELOG.md` (`# Changelog` / `## 0.1.0` / `Initial Release`), and a
`cliff.toml` copied from `sl-mesh/cliff.toml` with the crate's own
`tag_pattern` (`^sl_prim_[0-9.]*$`, `^sl_sculpt_[0-9.]*$`,
`^sl_client_bevy_viewer_[0-9.]*$`) and matching version trim.
