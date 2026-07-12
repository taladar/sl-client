---
id: test-environment
title: request environment settings
topic: test
status: done
origin: TEST_ROADMAP.md — Phase 11 — Region, estate & map `[both]`
---

Context: [context/test.md](../context/test.md).

`environment` — request environment settings. `1av`. **Green on OpenSim.**
    Drives the Extended Environment (EEP) `ExtEnvironment` capability with
    `Command::RequestEnvironment` (`parcel_id: None`, the whole region) and
    asserts a decodable `Event::Environment` reply arrives describing a
    non-degenerate day cycle: a positive `day_length` and at least one named
    sky/water frame (an empty frame set would mean the capability answered but
    decoded to nothing). Both grids serve a region default when no custom
    environment is set, so the invariant holds with no world setup — OpenSim's
    `EnvironmentModule.GetExtEnvironmentSettings` returns its built-in
    `WLDaycycle` (recorded here: `day_length=14400`, `day_offset=57600`, 8 sky
    frames across 4 altitude tracks + 1 water frame). Records the reply
    latency, day length/offset, reported `env_version`, and the sky/water
    frame and sky-track counts. **No new client code** — the
    `Command`/`Event`/`ExtEnvironment` CAPS surface and the
    `environment_from_llsd` parser already existed; only the runtime crates
    gained a re-export of `EnvironmentSettings` (present in both
    `sl-client-tokio` and `sl-client-bevy` for parity). `[both]`; the aditi
    run is deferred with the batch (SL serves its regional default over the
    same path).
