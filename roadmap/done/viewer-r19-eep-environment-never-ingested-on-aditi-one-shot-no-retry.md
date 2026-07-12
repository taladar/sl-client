---
id: viewer-r19
title: EEP environment never ingested on aditi (one-shot, no retry)
topic: viewer
status: done
origin: VIEWER_ROADMAP.md — Known rendering issues (to fix)
---

Context: [context/viewer.md](../context/viewer.md).

**R19. EEP environment never ingested on aditi (one-shot, no retry)**
(`sl-client-bevy-viewer` / `sl-client-bevy`, P22.1). **Fixed.**
Surfaced while debugging R18: on aditi the entire sky / sun / moon / cloud /
star / water stack silently ran on the **legacy WindLight defaults**
(`SkySettings::legacy_windlight_default`), never the region's real EEP. Root
cause was a cap-not-ready-yet **race** (the same class as the terrain fetch):
`request_environment` fired a **single** `RequestEnvironment` on
`RegionHandshakeComplete`, and the runtime **silently drops it** if the
`ExtEnvironment` capability is not in the caps map yet — which on a slower /
remote grid it usually is not at handshake time. Local OpenSim seeds caps fast
enough that the one-shot always won, so this went unseen until aditi. **Fix:**
`request_environment` now retries every 3 s (up to 12 attempts) until
`ingest_environment` folds the reply in and clears a pending flag (or it gives
up to the defaults); a `RegionHandshakeComplete` (login or border crossing)
starts a fresh cycle. The runtime also warns when `RequestEnvironment`
finds no `ExtEnvironment` cap. Verified: aditi now logs `environment ingested
(Region)` and the cloud params flip to `region_specified=true` with the
region's real values. This retroactively means **any P22/P23 behaviour
"verified on OpenSim only" was running on defaults on aditi** and should be
re-checked there now that the real EEP loads.
