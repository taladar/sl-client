---
id: idiomatic-p7-03
title: EEP textures (SkySettings sun/moon/cloud/bloom/halo/rainbow, WaterSett
topic: idiomatic
status: done
origin: IDIOMATIC_ROADMAP.md — Phase 7 — second-pass audit (missed ids, in-band sentinels, non-masking)
---

Context: [context/idiomatic.md](../context/idiomatic.md).

EEP textures (`SkySettings` sun/moon/cloud/bloom/halo/rainbow,
    `WaterSettings` normal_map/transparent_texture) → `Option<TextureKey>`;
    new `optional_texture_member`/`optional_texture_to_llsd` LLSD boundary
    helpers. Viewer effects (`ViewerEffectData::{LookAt,PointAt}`
    source/target, `Spiral` source/target) →
    `Option<AgentKey>`/`Option<ObjectKey>`; module-local
    `optional_agent`/`optional_object` decode helpers +
    `map_or_else(Uuid::nil,..)` encode. The REPL gained reusable
    `opt_agent`/`opt_object` arg helpers (absent/nil → `None`) for the rest of
    the sweep. +1 unit test. (commit "Phase 7 B part 1")
