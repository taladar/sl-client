---
id: viewer-refused-capability-silent-drops
title: Commands for a capability the grid refuses are dropped without a word
topic: viewer
status: bugs
origin: gridspec-seed-capabilities audit (2026-10-04)
refs: [gridspec-seed-capabilities]
---

Context: [context/viewer.md](../context/viewer.md).

The `gridspec-seed-capabilities` audit checked, for every capability a stock
OpenSim refuses (`book/src/gridspec/capabilities.md`), what both runtimes do
when a command needs it. Three gaps with a real fallback were fixed then (user
reports, offline messages, material edits). What remains are commands with
**no** fallback on such a grid that are dropped silently — `if let Some(url) =
caps.get(..) { … }` with no `else` — so a floater waits for a reply that never
comes:

- **All experience commands** (`GetExperienceInfo`, `FindExperienceByName`,
  `GetExperiences`, `ExperiencePreferences`, `AgentExperiences`,
  `GetAdminExperiences`, `GetCreatorExperiences`, `GroupExperiences`,
  `IsExperienceAdmin`, `IsExperienceContributor`, `UpdateExperience`,
  `RegionExperiences`, `ExperienceQuery`) — tokio `lib.rs` around 2354–2437,
  bevy `lib.rs` around 3930–4099. The experience floaters get nothing on
  OpenSim.
- **Voice** (`RequestVoiceAccount`, `RequestParcelVoiceInfo`,
  `SendVoiceSignaling`) — tokio `lib.rs` around 2280, bevy around 3785.
- **The explicit `Ais3*` commands** — only reachable from `sl-repl`.
- **bevy only:** before the region's capabilities arrive, the upload-failure
  and marketplace-failure reports are skipped (`upload.rs` around 74,
  `lib.rs` around 4595), where tokio reports them.

Fix: report each as a failure the caller can see (the
`report_caps_failure` path the material edit now takes), and have the viewer
say "not available on this grid" where it would otherwise wait — or disable
the floater up front from the granted capability set, as the reference
viewer disables its material editor.
