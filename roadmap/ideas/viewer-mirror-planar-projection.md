---
id: viewer-mirror-planar-projection
title: Realtime mirrors as a planar projection, not a cube from the mirror's centre
topic: viewer
status: ideas
origin: viewer-realtime-mirrors (2026-11-10)
refs: [viewer-realtime-mirrors]
---

Context: [context/viewer.md](../context/viewer.md).

Our realtime mirror is a cube captured at the mirror probe's origin and sampled
by reflected direction, like any reflection probe. A flat mirror reflects from
its *plane*, so the cube puts things where a cube map does, not where a mirror
does: in the fake-grid mirror scene
(`a_mirror_shows_the_avatar_on_the_fake_grid`, `mirror.png`) the reflected
horizon sits well below the real one and the avatar comes back small, seen
from the pane's centre rather than from the eye's mirror image.

The reference's hero probe is "a realtime planar projected probe" (its Probe
Update tooltip): `LLHeroProbeManager` renders from the camera mirrored in the
mirror plane and the surface samples that projection. Doing the same — one
camera mirrored about the plane, an oblique near plane on the mirror, the
surface sampling by screen position — would also cost one render per mirror
instead of six.

Bevy's parallax-corrected light probes are a cheaper middle ground for a box
volume, and may be worth trying first.
