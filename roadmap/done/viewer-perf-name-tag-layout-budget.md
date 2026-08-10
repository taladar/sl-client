---
id: viewer-perf-name-tag-layout-budget
title: Per-frame budget for name-tag text layout
topic: viewer
status: done
origin: unbounded-frame-work survey (2026-08-09, performance branch)
refs: [viewer-name-tags-billboard-render]
---

Context: [context/viewer.md](../context/viewer.md).

`layout_tag_text` is change-gated, but arriving in a crowded place (login /
teleport — or, rarer, a crowd arriving around us) creates every tag dirty in
the same frame, and each costs parley shaping
(`TextPipeline::update_buffer`) plus glyph rasterisation into the shared
font atlases (`update_text_layout_info` — new atlas pages when a name brings
uncached glyphs), followed by a per-tag mesh build downstream.

Fix: `TagLayoutBudget` (default 4 blocks/frame, env
`SL_VIEWER_TAG_LAYOUT_BUDGET`). Over-budget dirty tags go into the system's
**existing** font-retry `reprocess_queue` `Local`, whose `remove` re-marks
the entity changed on a later frame — so deferred tags re-enter with no new
machinery. `build_tag_meshes` is `Changed<TextLayoutInfo>`-driven and is
bounded by the same cap automatically. `sync_tag_spans` (cheap string /
entity work) is untouched. The glyph atlas is shared frame-thread state, so
this is a budget, deliberately not an off-thread move.

Also promoted `textures::env_budget` to `pub(crate)` as the shared
env-override helper for budget resources.

Verify: Tracy max of `layout_tag_text` + `build_tag_meshes` after
teleporting into a crowded region; tags should populate over a few frames
instead of one hitch.
