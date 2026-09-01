---
id: test-assets-settings-encoder
title: Write an EEP settings asset, not only read one
topic: test
status: ready
origin: asset-class audit while doing viewer-static-asset-library (2026-09-01)
points: 3
refs: [test-shared-test-assets, viewer-environment-my-environments]
---

Context: [context/testing.md](../context/testing.md).

`AssetType::Settings` — an EEP sky, water or day-cycle asset — is decoded
by `environment_asset_from_bytes`, driving
`sl-viewer-platform/src/environment_assets.rs`. Nothing writes one. The
region's environment reaches a fake grid through
`RegionFixture::environment` as a *typed* `EnvironmentSettings` over the
`ExtEnvironment` capability, which is a different path entirely: it never
produces asset bytes, so no fixture can put a settings asset in an
inventory, offer one, or serve one by id.

That blocks the whole inventory half of EEP: applying a settings item
from inventory, the environment editor's save/load,
[[viewer-environment-my-environments]], a day cycle offered in an IM, and
any fake-grid test that wants "this parcel's environment is *that* asset".

Add a writer — the inverse of the decoder, in whichever crate the decoder
lives closest to (`sl-proto`'s `types::environment` already owns
`SkySettings` / `WaterSettings` / the day-cycle model, and the round trip
belongs beside them; `sl-test-assets` then only needs a couple of named
fixtures on top).

Wanted:

- `SkySettings` / `WaterSettings` / day cycle → the LLSD document the
  decoder reads, round-tripped in a unit test;
- one recognisable fixture of each in `sl-test-assets` — a **night** sky
  and a **noon** sky at minimum, since the render oracle for "an
  environment change happened" is a luminance difference, and a day cycle
  that visibly moves between the two.

Note `legacy_windlight_default()` already builds the typed values, so the
fixtures are a serialisation away rather than a hand-written LLSD blob.

Acceptance: a settings asset written from a typed value decodes back to
it; a fake-grid fixture can serve a sky settings asset by id.
