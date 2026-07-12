---
id: viewer-r16
title: Linden system hair shows on mesh-hair avatars
topic: viewer
status: done
origin: VIEWER_ROADMAP.md — Known rendering issues (to fix)
---

Context: [context/viewer.md](../context/viewer.md).

**R16. Linden system hair shows on mesh-hair avatars**
(`sl-texture`). Surfaced during the P20.2 aditi session: the default Linden
**system hair** base-mesh part (`avatar_hair.llm`, the helmet-shaped scalp
mesh) kept rendering as a solid **dome** even on avatars that wear a **rigged
mesh hair** attachment (or are bald), where the reference viewer hides it.
**Root cause** (the third candidate — the hair bake's own alpha not being
applied): a Second Life server "Sunshine" bake is a **5-component** J2C, whose
channels are `R G B alpha mask` (the reference's `RGBHM`: colour,
heightfield/**alpha**, clothing mask — `llviewertexlayer.cpp`). Our
`decode_multicomponent` took only RGB and reported `components: 3`, so **every
modern-SL bake was classified fully opaque** and the composited alpha (which
makes a hair bake soft and a bald/mesh-hair bake transparent) was thrown
away — the scalp mesh then read as a solid helmet and the P14.3
transparent-region hide never fired. **Fix:** `decode_multicomponent` now
keeps the first four
channels — RGB **plus the composited alpha (channel 3)** — as the RGBA8 pixels
(matching the reference viewer's `decodeChannels(.., 0, 4)`), so the existing
P14.3 pipeline classifies a bald/mesh-hair hair bake `Transparent` (region
hidden) or `Masked` (soft hair) with no rendering-code change. The 5th channel
(the clothing/bump mask) is preserved in a new `DecodedImage::aux` field,
mirroring the reference's separate `decodeChannels(.., 4, 4)` pass, for later
material use; `downsample` carries it in lockstep. Confirmed live on aditi
(own + nearby avatars): the hair dome is gone. Reference:
`LLViewerTexLayerSetBuffer::readBackAndUpload` (`baked_image_components = 5`),
`LLImageJ2C::decodeChannels`, `LLVOAvatar::updateMeshVisibility`.
