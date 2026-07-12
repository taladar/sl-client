---
id: protocol-34
title: Experience key-value store
topic: protocol
status: deferred
origin: ROADMAP.md — Tier C
---

Context: [context/protocol.md](../context/protocol.md).

**34. Experience key-value store · ⛔ out of scope (not a client protocol
feature).** Item #27 implemented the experience metadata and permission CAPS and
deferred the experience's server-side **key-value store**, which a later pass
promoted here on the assumption it was a small client CAPS API. On
investigation it is **not** a client-facing protocol feature at all, so it is
reclassified out of scope:

- The reference viewer (Firestorm) requests all 13 experience capabilities in
  `indra/newview/llviewerregion.cpp` (`GetExperiences`, `AgentExperiences`,
  `FindExperienceByName`, `GetExperienceInfo`, `GetAdminExperiences`,
  `GetCreatorExperiences`, `ExperiencePreferences`, `GroupExperiences`,
  `UpdateExperience`, `IsExperienceAdmin`, `IsExperienceContributor`,
  `RegionExperiences`, `ExperienceQuery`) — **all implemented by #27** — and
  **no** key-value capability.
- The SL wiki's authoritative *Current Sim Capabilities* list contains no
  `ExperienceKeyValue` (nor any `KeyValue`) capability.
- `llReadKeyValue` / `llCreateKeyValue` / `llUpdateKeyValue` /
  `llDeleteKeyValue` / `llKeysKeyValue` / `llDataSizeKeyValue` appear in the
  viewer tree only inside `app_settings/keywords_lsl_default.xml` — the
  script-editor keyword list — never as cap requests or HTTP calls.

The key-value store is an **in-world LSL surface only**: scripts call those
functions and the simulator services them against an internal Linden datastore
over a service-to-service path that is never surfaced to a viewer/client. A
client cannot read or write it (not even its own experience's store — that too
is script-only). There is consequently no client wire protocol to implement, so
this joins the other out-of-scope items below (the asset-byte *decode* /
rendering / voice-transport family). With this reclassified, **the roadmap's
client protocol *feature* surface is complete: #1–#33 are done.** The only
remaining open work is the **Tier E decode-fidelity fixes (#35–#51)** — not new
features, but information-loss gaps where an already-shipped item decodes a wire
field and then drops it before the caller sees it. (#35–#49 are now
done; #50–#51 remain.)
