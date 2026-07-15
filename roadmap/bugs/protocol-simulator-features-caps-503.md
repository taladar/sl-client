---
id: protocol-simfeatures-503
title: SimulatorFeatures capability GET returns 503 on the local OpenSim (one-shot fetch never retries)
topic: protocol
status: bugs
origin: observed while live-testing protocol-lsl-syntax (2026-07-15)
---

Context: [context/protocol.md](../context/protocol.md).

While live-testing [[protocol-lsl-syntax]] against the local `opensim.service`
grid, the automatic **`SimulatorFeatures`** capability GET failed on every run,
in **both** runtimes (`sl-client-tokio` and `sl-client-bevy`), logging:

```text
WARN sl_client_tokio: CAPS request failed; no reply surfaced capability="SimulatorFeatures"
```

A throwaway diagnostic in `get_llsd` showed the GET returns **HTTP 503 Service
Unavailable with an empty body** — so it is an OpenSim-side cap-serving fault,
not a client parse error (the body never reaches `parse_simulator_features`).

Crucially this is **specific to the `SimulatorFeatures` cap**, not a general
outage: at the *same* caps-arrival instant the `LSLSyntax` cap GET returned 200
and decoded fully (653 functions / 770 constants / 35 events), and the region
handshake, EEP environment, and texture caps all worked. So the 503 is not a
startup race that a blanket retry would paper over — the `SimulatorFeatures`
handler itself is answering 503 on this OpenSim build/config.

Two things to pin down:

- **Why does OpenSim's `SimulatorFeatures` cap answer 503** here when its
  sibling caps answer 200? (Check `SimulatorFeaturesModule` handler
  registration / method, and whether a config toggle or a poll-service wrapper
  is involved. The `[SimulatorFeatures]` block in `OpenSim.ini` is all
  commented, i.e. defaults.)
- **Client robustness:** the runtimes fetch `SimulatorFeatures` **once** at caps
  arrival with **no retry** (`spawn_simulator_features`), so any transient
  failure is never recovered — unlike Firestorm, which defers/retries. Even if
  the OpenSim 503 is fixed, a one-shot fetch is fragile.

Impact: [[protocol-lsl-syntax]] is implemented and verified end-to-end (the
`LSLSyntax` fetch + decode works against the real OpenSim document), but its
**automatic trigger** keys off the `lsl_syntax_id` carried in the
`SimulatorFeatures` reply — so while this 503 stands, the local grid never fires
the LSLSyntax fetch on its own. The feature works the moment `SimulatorFeatures`
decodes (as on Second Life / aditi, or once this is fixed).
