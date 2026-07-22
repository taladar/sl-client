---
id: viewer-ambient-wind-sound
title: Ambient wind sound
topic: viewer
status: blocked
origin: Vintage-parity coverage audit (2026-07-22)
blocked_by: [viewer-audio-backend]
---

Context: [context/viewer.md](../context/viewer.md).

The procedural **wind** audio bed the reference synthesises client-side
(no asset): filtered noise whose gain and pitch follow the region wind
vector at the agent (`LayerData` wind — decoded), the agent's own velocity
(rushing air when flying / falling fast), and altitude, on the ambient
bus with its own volume slider. Quiet but load-bearing for the sense of
being outdoors; the classic reference implementation
(`llwindaudio`) is small and worth porting faithfully.

Reference (Firestorm, read-only): `llaudioengine` wind synthesis
(`llwindaudio`), the ambient-volume settings.

Deps: [[viewer-audio-backend]] (a procedural source into the ambient
bus).
