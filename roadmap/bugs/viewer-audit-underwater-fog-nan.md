---
id: viewer-audit-underwater-fog-nan
title: A negative underwater fog density NaNs the screen
topic: viewer
status: bugs
origin: static code audit (2026-08-26)
points: 1
---

Context: [context/viewer.md](../context/viewer.md).

`sl-viewer-world-scene/src/underwater_fog.rs:194` —
`base.powf(water.underwater_fog_mod.clamp(0.0, 10.0))` with a
**negative `base`** and a non-integral exponent is `NaN`, which reaches the
uniform and NaNs the whole screen.

The reference already fixed this: `llsettingswater.cpp:393` guards it, with the
comment *"BUG-233797/BUG-233798 -ve underwater fog density can cause
(unrecoverable) blackout"*.

Port the guard. `underwater_fog.rs` has no tests; `water_params` and this
expression are pure and need no GPU.
