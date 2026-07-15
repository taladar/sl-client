---
id: viewer-avatar-impostors
title: Avatar impostors & complexity limiting
topic: viewer
status: ideas
origin: render-feature gap analysis vs Firestorm (2026-07)
---

Context: [context/viewer.md](../context/viewer.md).

The performance feature that makes a crowded region survivable: past a limit,
distant avatars are rendered as flat **billboard impostors** (a cached snapshot
re-rendered occasionally) instead of full geometry, and over-heavy avatars are
capped. In a busy club, the difference between this and its absence is tens of
frames per second.

Two linked mechanisms in Firestorm:

- **Impostors** — beyond `RenderAvatarMaxNonImpostors` (surfaced as
  `IndirectMaxNonImpostors`), the *N* nearest avatars render fully and the rest
  become impostors: render each to its own small target, draw that as a
  camera-facing billboard, and refresh it only when the avatar moves / animates
  or the view angle changes enough. This is the same render-to-texture idea as
  the P33 probes, applied per distant avatar.
- **Complexity limiting** (`RenderAvatarMaxComplexity` /
  `RenderAvatarComplexityMode`) — score each avatar's render cost (triangles,
  textures, attachments) and, past a budget, draw it as a flat "jellydoll"
  silhouette rather than its real (griefer-heavy) attachments. Needs a
  complexity metric per avatar and the fallback render.

Scope: the nearest-N selection (we already track avatar distances), the impostor
render target + billboard + refresh policy, the complexity score, the jellydoll
fallback, and the user controls (the limits, plus a per-avatar
"always render fully / never" override). Relates to the R22 avatar-render work.

Reference (Firestorm, read-only): `llvoavatar` impostor path,
`RenderAvatarMaxNonImpostors` / `RenderAvatarMaxComplexity` /
`RenderAvatarComplexityMode`.

Builds on: the avatar rendering (P12–P18) and the coarse/interest distance
tracking already in `avatars.rs`.
