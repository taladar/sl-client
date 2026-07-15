---
id: viewer-screen-space-reflections
title: Screen-space reflections (SSR)
topic: viewer
status: ready
origin: render-feature gap analysis vs Firestorm (2026-07)
---

Context: [context/viewer.md](../context/viewer.md).

Ray-marched reflections in screen space — the sharp, contact-accurate
reflections on wet floors, water and glossy PBR surfaces that the reflection
probes (P33) cannot give, because probes are low-resolution and static-ish while
SSR reflects the actual pixels on screen this frame.

Firestorm gates it on `RenderScreenSpaceReflections`; it is a deferred post pass
on the water / PBR reflection path, and it **layers with** the reflection probes
rather than replacing them (SSR where the screen has the data, probe fallback
where it does not — off-screen and grazing rays).

Scope: march the depth/normal buffers per reflective pixel, resolve the hit
colour, fade at screen edges and where the ray leaves the depth buffer, and
blend against the probe reflection as the fallback. Roughness-aware blur so
glossy (not just mirror) surfaces reflect correctly. This is a real GPU cost
centre — tie it to a quality tier.

Reference (Firestorm, read-only): the SSR deferred pass,
`RenderScreenSpaceReflections`.

Builds on: the deferred G-buffer (depth / normal / roughness) and the P33
reflection probes as the fallback.
