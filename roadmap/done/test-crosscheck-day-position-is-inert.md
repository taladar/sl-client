---
id: test-crosscheck-day-position-is-inert
title: --day-position asks for a keyframe the stock cycle does not have
topic: test
status: done
origin: pointing Firestorm at the fake grid ([[test-firestorm-fake-grid-crosscheck]], 2026-09-10)
points: 2
refs: [test-firestorm-fake-grid-crosscheck, test-fake-grid-sky-without-density-profiles]
---

Context: [context/testing.md](../context/testing.md).

`sl-crosscheck --day-position 0.5` does nothing against the stock region, and
the reference harness says so once, in its own log, where nobody was looking:

```text
day cycle has no sky at position 0.5; leaving environment alone
```

`FSTestHarness::applyEnvironment` pins the sun with
`LLSettingsDay::getSkyAtKeyframe(position, TRACK_GROUND_LEVEL)`, which finds
the frame **at exactly that keyframe** rather than evaluating the track there.
The fake grid's stock environment is a single-keyframe cycle at `0.0`
(deliberately — a cycle with one frame is a sky that does not move with the
region clock, which is what makes two captures minutes apart comparable), so
every position but `0.0` finds nothing and the sky is left alone. A run then
photographs whatever sky the region already had, and two runs at different
`--day-position` values come back identical, which reads as "the sun does not
matter here" rather than as "the knob is not connected".

Until 2026-09-10 this was invisible behind a larger fault — the day cycle was
being rejected outright ([[test-fake-grid-sky-without-density-profiles]]), so
there was no region cycle to sample at any position. With that fixed the
keyframe lookup is what is left.

Two ways out, and they are not exclusive:

- **Evaluate the track instead of indexing it.** Asking for "the sky at day
  position `p`" should blend the frames either side of `p`, which is what a
  running viewer does every frame anyway. A fork change
  (`~/devel/3rdparty/phoenix-firestorm`, branch `test-harness`), and it wants
  the same treatment in this workspace's own harness so the pair still agrees
  — a `--day-position` that moves one viewer's sun and not the other's would
  be worse than one that moves neither.
- **Give a scene a day cycle worth sampling.** `RegionConfig::environment`
  already takes a whole `EnvironmentSettings`, so a scenario that wants
  dawn / noon / dusk can carry frames at those keyframes and the exact lookup
  finds them. That is the cheaper half and does not touch either viewer.

Whichever is done, the flag must **fail loudly** when it cannot be honoured:
a capture whose lighting was not the lighting that was asked for is not a
capture of the requested scene, and a warning buried in a viewer log is not
a report. The runner reads `harness-status.json`, which is where an
unhonoured pin belongs.

## Done (2026-09-10)

All three, because the first two are not alternatives once the third is
taken seriously: a pin that fails loudly has to be able to succeed.

**Both harnesses evaluate the track.** The reference half now blends the
bounding pair with `LLTrackBlenderLoopingManual` — the same blender the
day-cycle editor and RLV's `@setenv_daytime` already drive — instead of
`getSkyAtKeyframe`, which looked the position up as a *key* in the track's
map and so declined every position but the ones a cycle happens to be keyed
at. This workspace's half already blended (`blended_sky_settings`), but it
blended the wrong cycle: `install_preset_day_cycle` fired on *any* pinned
position, so a run against a region with a real cycle rendered the legacy
presets and reported nothing. It is now gated on
`EnvironmentSettings::day_position_moves_the_sky` — nothing to interpolate —
which is what its own doc comment always said it was for.

**The grid serves a cycle worth sampling.** `RegionConfig::
ensure_day_cycle_can_be_sampled` installs the four legacy WindLight presets
(midnight / sunrise / midday / sunset at `0.0` / `0.25` / `0.5` / `0.75`)
over a region whose schedule holds one sky, and `sl-crosscheck` calls it for
every region of a run that passes `--day-position`. A scene carrying its own
multi-frame cycle keeps it. On the *grid* rather than in a viewer on purpose:
either viewer could synthesise a cycle when the region's cannot be sampled,
and then the two would photograph two different skies while both reporting
the request as honoured — the one failure a cross-check must not be able to
produce. Serving them needed the presets to stop being viewer-only content;
they moved from `sl-viewer-kit::sky_presets` into `sl_proto::sky_presets`,
beside `SkySettings::legacy_windlight_default`, which is where the rest of
the reference's ported sky content already lives. `FixedSky` — the World ▸
Environment menu's own vocabulary — stayed behind.

**And it fails loudly.** `harness-status.json` grew a `day_position`
(`requested` / `honoured` / `detail`), written by both viewers, present
whenever a sun was pinned and absent otherwise. An unhonoured pin fails that
viewer's status, fails `RunSummary::ran_as_asked` (so the runner's exit
status), and prints as `SUN NOT PINNED at 0.25 — …`. A viewer that reports
*nothing* about the pin is a third state, printed as `SUN NOT REPORTED`, and
also a failure: a build older than the field cannot say which sky it drew,
and silence must not read as success.

### Verified, and what the first run caught

Two `sl-crosscheck --scenario catalogue --look-at mesh-cube` runs, both
viewers, at `--day-position 0.5` and `0.0`. Mean RGB of the third frame:

| viewer      | 0.5 (midday)          | 0.0 (midnight)      |
| ----------- | --------------------- | ------------------- |
| `sl-client` | `(121.7 166.2 121.1)` | `(26.7 45.6 25.1)`  |
| firestorm   | `(113.1 157.1 116.0)` | `(17.1 31.2 22.7)`  |

Both move, and they move together — which is the whole claim. Before this,
the reference's frames were identical at every position.

**The first run failed, and it failed on this change rather than on the old
fault.** Firestorm reported the pin honoured against four keyframes;
`sl-client` reported "the region's day cycle schedules one sky" — while its
own log showed it ingesting `4 sky frame(s), cycle "Legacy WindLight
Presets"` a second after login and a full settle before the first frame. The
reading was real but taken at the wrong time: the capture schedule folded in
the pin's state from the first frame *of the process*, and a viewer starts on
its built-in single-frame default and only asks the region for an environment
once it is in world. Every run's first seconds report a substituted sky that
nothing is photographed under. The observation window now opens at the first
**captured frame** — before that a reading replaces the last, from there on an
unhonoured one sticks — with a test either side of the boundary. Worth
keeping: a status field about what the *frames* were like must be sampled when
frames are being taken, not when the process is.

The single-keyframe stock environment is unchanged and still the default,
because the property it buys is real: an unpinned run's captures are
comparable minutes apart. `EnvironmentSettings::default_region` is now that
one environment, public in `sl-proto` (it was a private copy in
`sim_session.rs`), so a fixture can start from what the region would have
served.
