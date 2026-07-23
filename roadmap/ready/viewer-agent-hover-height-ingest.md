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
