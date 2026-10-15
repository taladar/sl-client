---
id: viewer-agent-hover-height-ingest
title: Ingest the account hover height (AgentPreferences) into the plant
topic: viewer
status: ready
origin: R23 aditi verification residual (2026-07-23)
refs: [viewer-r23]
---

Context: [context/viewer.md](../context/viewer.md).

After the R23 root-plant fix, the own avatar still sits **slightly in the
ground on aditi** (OpenSim plants cleanly). The remaining term the reference
applies and we do not is `getHoverOffset()` — the **account-level hover
height** preference (the viewer's Avatar Height slider / `llSetHoverHeight`),
distinct from the shape's transmitted `Hover` visual param (id 11001, which
R23 does apply). The reference adds it to the root position after the
body-size correction (`LLVOAvatar::updateCharacter`).

Scope:

- Fetch/ingest the hover: SL sends it via the `AgentPreferences` capability
  (POST, reply carries `hover_height`), and per-avatar updates arrive for
  other agents too (`AgentPreferencesUpdate`-style events); check the
  reference for the exact flows (`llagent.cpp` `setHoverHeight`,
  `LLVOAvatar::setHoverOffset`).
- Add the ingested value to the R23 root drop (`root_drop_from_metrics`'s
  `hover` argument is already there — the shape param and the account hover
  sum in the reference).
- Live-check on aditi against Firestorm with a non-zero hover setting.

Audit 2026-10-15: both inputs are decoded now and neither reaches the plant.
`sl-wire/src/agent_preferences.rs` reads and writes the capability's
`hover_height` (the own avatar's value), and every `AvatarAppearance` carries
the per-avatar `AppearanceHover` block as `hover_height: Option<Vector>`
(`sl-proto/src/types/appearance.rs`, filled in
`session/conversions.rs`) — that block is how other agents' hover arrives,
not a separate preferences event. `sl-viewer-world-avatar/src/avatars.rs`
still passes only the shape's `Hover` param to `root_drop_from_metrics`
(its doc says the region-side hover "is not ingested yet"). What remains:
add the appearance's `hover_height.z` (and, for the own avatar, the
`AgentPreferences` value) to that `hover` term, then the aditi live check.
