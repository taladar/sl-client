# Environment

What each grid answers on the `ExtEnvironment` capability: a region's day, a
parcel's, and what a set or a reset gets back. The settings themselves are
described in [Region & Estate Information](../content/region.md).

Measured on 2026-10-05 on Second Life's beta grid (aditi) and the local
OpenSim standalone by the conformance case `environment`, which reads the
region's and a parcel's environment and — as OpenSim's estate owner — sets and
resets a parcel's. The raw replies were read from the trace log
(`RUST_LOG=sl_client_tokio::wire=trace`). The aditi test accounts may change no
land, so on Second Life an accepted change is not measured.

## A region's environment

| behaviour | Second Life | OpenSim | fake grid |
| --- | --- | --- | --- |
| reply envelope | `environment`, `success` | `environment`, `parcel_id`, `success` | OpenSim's (the client reads either) |
| keys of `environment` | `day_cycle`, `day_hash`, `day_length`, `day_offset`, `env_version`, `is_default` (false), `parcel_id` (-1), `region_id`, `track_altitudes` | `day_cycle`, `day_length`, `day_offset`, `env_version`, `flags`, `parcel_id` (-1), `region_id`, `track_altitudes` | OpenSim's |
| day length / offset of a default day | 14400 s / 57600 s | 14400 s / 57600 s | the same |
| `env_version` | 1 on the region reached | 0 for the default | `FakeSl` 1, `FakeOpensim` 0 |
| day cycle name | a name somebody chose (`3 hr day / 1 hr night #4`) | `Default` | `FakeSl` `Default Daycycle`, `FakeOpensim` `Default` |
| tracks | 5: one water keyframe, 8 sky keyframes on the ground track, 3 empty altitude tracks | the same | 5: one water and **one** sky keyframe |
| frame names | decimal hashes (`2221907172`) | decimal hashes (`14663353929151905476`) | `Default`, `Default Water` |
| a sky frame's keys | 38, with `asset_id` | the same 37 without `asset_id` | the client's own encoding |
| a water frame's keys | 13 | 14: also `transparent_texture` | the client's own encoding |
| `track_altitudes` | 1000, 2000, 3000 | 1000, 2000, 3000 | the same |
| `reflection_probe_ambiance` on the default skies | absent | absent | absent |
| the capability at `RegionHandshake` time | already granted (one viewer login) | already granted | granted with the seed |

The fake grid's default day is a single keyframe on purpose: a day with more
renders differently by the region clock, and two captures minutes apart would
not compare (`EnvironmentSettings::default_region`). A scene that wants a real
day installs one.

## A parcel's environment

| behaviour | Second Life | OpenSim | fake grid |
| --- | --- | --- | --- |
| a parcel with no environment of its own | `environment` holds `is_default` (true), `parcel_id`, `region_id`, `track_altitudes` — **no day cycle** | the same | the same, both flavours |
| a set by someone without rights | `success: false`, message `You do not have permission to modify this parcel.Failed to change the environment on this parcel.` | `success: false`, message `No permission to change parcel environment` | taken: the fake grid enforces no land rights |
| a set by the owner, the reply | not measured | **`{success: true}` and nothing else** | `FakeOpensim` the same; `FakeSl` the stored environment |
| a set by the owner while the estate does not allow parcel environments | not measured | accepted and stored, but a read still answers `is_default` | `FakeOpensim` serves it regardless (`server-fake-grid-parcel-on-movement`) |
| a set by the owner once the estate allows them | not measured | served: `parcel_id` the parcel's, the day sent, the length set | served |
| a reset, the reply | not measured | `{messageID, regionID, success: true}` | `FakeOpensim` the same; `FakeSl` the inheriting answer |
| a single-track set (`trackno`) | supported by the reference viewer | refused: `Environment Track not supported` (from the source, `EnvironmentModule.SetExtEnvironmentSettings`) | applied to the whole day |

Neither grid sends the region's day again for a parcel that only inherits. A
viewer shows such a parcel the region's environment itself, and an editor
opened on it starts from a default day.

## What the client does with it

A headless viewer logged into each grid (`sl-viewer-ctl launch`, then
`environment`) ingested the region's day at its first request and drew from
it: on aditi the cycle `3 hr day / 1 hr night #4`, sky frame `949491502`; on
OpenSim `Default`, sky frame `15123771676403276959`. With no
`reflection_probe_ambiance` in either default, both skies take the classic
path (`SkySettings::sky_hdr_scale` of 1). The request is retried for as long
as the capability is missing, which neither login needed.

- A reply that takes a change without saying what the land has now (OpenSim's
  two) is followed by a GET of the same land, in both runtimes, so a caller of
  `Command::SetEnvironment` or `ResetEnvironment` gets `Event::Environment` on
  either grid (`sl_proto::environment_reply_needs_refetch`). The reference
  viewer applies nothing and waits for the grid's next push.
- A refusal is `Event::EnvironmentChangeRefused` with the grid's reason, which
  the viewer shows in the reference's `WLRegionApplyFail` alert. Until
  2026-10-05 both it and OpenSim's bare acceptance were logged as replies that
  did not parse, and nothing reached the caller.
- The viewer refetches the region's environment on every `RegionInfo`, and a
  parcel's when `ParcelProperties` reports a new `ParcelEnvironmentVersion`
  with the estate allowing parcel environments
  (`RegionAllowEnvironmentOverride`). Both grids reported version -1 for a
  parcel without one; aditi's region allowed the override, the local OpenSim
  estate did not.
