---
id: idiomatic-p7-04
title: ParticleSystem texture_id/target_id become typed Option keys
topic: idiomatic
status: done
origin: IDIOMATIC_ROADMAP.md — Phase 7 — second-pass audit (missed ids, in-band sentinels, non-masking)
---

Context: [context/idiomatic.md](../context/idiomatic.md).

`ParticleSystem.texture_id`/`target_id` → `Option<TextureKey>`/
`Option<ObjectKey>` (nil → `None` in the `PSYS` blob codec). (commit "Phase 7
B part 2")
