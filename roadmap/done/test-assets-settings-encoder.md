---
id: test-assets-settings-encoder
title: Write an EEP settings asset, not only read one
topic: test
status: done
origin: asset-class audit while doing viewer-static-asset-library (2026-09-01)
points: 3
refs: [test-shared-test-assets, viewer-environment-my-environments, viewer-environment-fixed-editor]
---

Done (2026-09-01), in three layers, because the writer needed an encoding
nothing in the workspace had.

**`sl-llsd`** — `Llsd::to_llsd_notation`, the inverse of
`parse_llsd_notation`. A settings asset is *notation* LLSD: the reference
serializes one with `LLSDSerialize::LLSD_NOTATION`
(`LLSettingsVOBase::createInventoryItem`, `indra/newview/llsettingsvo.cpp`), so
writing XML instead would have produced bytes no real grid serves. The writer
mirrors `LLSDNotationFormatter` at its default options — bare `1`/`0` booleans,
`i`/`r`/`u` prefixes, single-quoted strings escaped through the reference's own
`NOTATION_STRING_CHARACTERS` table, uppercase `b16"…"` binary — with one
deliberate improvement: reals are written at Rust's shortest round-tripping
precision rather than the ostream default of six significant digits, which
would have lost bits the decoder then read back wrong.

`settings_asset_llsd` now **dispatches on the header line** rather than
sniffing. It tried binary before notation, and a notation document opens with
`{`, which is also binary LLSD's map-begin marker — so a notation asset was
being handed to the binary parser first and only survived because that parse
failed. The `<? … ?>` line names the encoding (as `LLSDSerialize::deserialize`
reads it); it is now honoured, with the try-each-in-turn path kept for a
headerless payload.

**`sl-proto`** — `environment_asset_to_bytes`, the inverse of
`environment_asset_from_bytes`, emitting `<? llsd/notation ?>` plus the
notation body. The frame encoders (`sky_settings_to_llsd` /
`water_settings_to_llsd` / `day_cycle_to_llsd`) already existed for the
`ExtEnvironment` envelope, so the writer is the envelope's asset-shaped
sibling rather than a second serialiser.

`EnvironmentAsset` gained a **`DayCycle`** variant, and `day_cycle_from_asset`
decodes one. The enum modelled only the two single-frame kinds because the
World ▸ Environment presets are single frames — but a day cycle is what an
*inventory* environment item actually holds, so the fixture the task asked for
(a cycle running between the two skies) had nowhere to decode back into.

**`sl-test-assets::environment`** — `noon_sky` / `night_sky` / `water` /
`day_cycle` as typed values, each with an `_asset` function for its bytes.
The two skies sit at the ends of the brightness scale rather than in the
middle, because the only thing a capture can say about a sky is how bright it
came out: their `sunlight_color`s differ by 9x. They are *fixtures*, not
content — Linden's own four presets are ported in `sl_viewer_kit::sky_presets`
and remain the right frames for anything that wants to look like Second Life.

On the grid side the catalogue serves one settings asset of each kind
(`NOON_SKY_ASSET` … `DAY_CYCLE_ASSET`, ids `0xCA7_0009`–`0xCA7_000C`). No prim
names them — an environment is not a prim — so they are there for a viewer
pointed at the catalogue to fetch. The catalogue's *region* environment is
deliberately left unset: installing the fixture cycle would have made every
existing catalogue capture's brightness depend on the day position.

Tests: every kind round-trips through its bytes and every encoding decodes to
the same value (`sl-proto`), each fixture decodes back and the two skies are a
luminance apart (`sl-test-assets`), the catalogue serves and decodes all four
(`sl-fake-grid`), and a settings asset fetched over `ViewerAsset` under a
fixture's own id comes back as the day cycle that was written
(`client_end_to_end`). `sl-llsd` gained three writer tests, including the
escape table and an every-kind round trip.

This unblocks the settings-asset **save** path of
[[viewer-environment-fixed-editor]], which the environments library and the
day-cycle editor in turn build on.

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
