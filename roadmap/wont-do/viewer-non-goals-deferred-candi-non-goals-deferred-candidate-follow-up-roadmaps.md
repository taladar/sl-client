---
id: viewer-non-goals-deferred-candi
title: Non-goals (deferred; candidate follow-up roadmaps)
topic: viewer
status: wont-do
origin: VIEWER_ROADMAP.md — Non-goals (deferred; candidate follow-up roadmaps)
---

Context: [context/viewer.md](../context/viewer.md).

Most former non-goals are now planned phases (see Phases 19–34): PBR / GLTF and
legacy normal / specular materials + bump / shiny / glow / fullbright
(Phase 27), animated textures (28), water surface (23), sky / atmosphere (22),
shadows (24), distance-based LOD switching (21), local lights (25), Linden
trees / grass (26), animesh (29), particles (30), flexi prims (32), reflection
probes (33), and avatar cloth / breast-butt physics (34), on a shared `avian3d`
physics foundation (31). Still deferred: facial-morph lip-sync, object
selection / interaction, any chat *input* or non-quit UI, and sound.

## Obsolete (audit 2026-10-15)

This was a section of the retired monolithic `VIEWER_ROADMAP.md`, not a
task, and every claim in it has since been overtaken: the "still deferred"
items all shipped (object selection / interaction —
[[viewer-object-selection-core]]; chat input — [[viewer-chat-history-panel]];
sound — [[viewer-streaming-audio]] and the `sl-audio` / `sl-viewer-audio`
crates), and the shared `avian3d` physics foundation it names was removed in
favour of a custom `parry3d` raycast index. Remaining work lives in the
per-task items, so there is nothing left to track here.
