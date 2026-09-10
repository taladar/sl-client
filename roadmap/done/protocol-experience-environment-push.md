---
id: protocol-experience-environment-push
title: An experience can set the sky, and nothing in this stack can say so
topic: protocol
status: done
origin: reading the reference while scoping the environment leg of test-fake-grid-timeline (2026-09-09)
points: 3
refs:
  [
    viewer-environment-not-refetched-on-region-info,
    test-fake-grid-timeline,
    protocol-experience-parcel-recheck,
  ]
---

Context: [context/protocol.md](../context/protocol.md).

`llSetEnvironment` in an experience script does not change the region's
settings — it **pushes** an environment at the viewers in the experience, which
they layer over whatever the region says and drop again when the experience
releases them. That push exists in the protocol and this workspace implements
neither end of it.

## What the reference does

The envelope is an ordinary `GenericMessage` with the method
`PushExpEnvironment`, registered on the viewer's generic dispatcher at
`indra/newview/llenvironment.cpp:979`:

```cpp
if (!gGenericDispatcher.isHandlerPresent(MESSAGE_PUSHENVIRONMENT))
{
    gGenericDispatcher.addHandler(MESSAGE_PUSHENVIRONMENT, &environment_push_dispatch_handler);
}
```

The handler (`:300`–`:356`) unpacks the string parameters into an LLSD map
keyed by `experience`, `action`, `action_data`, `transition`, plus the object
and parcel names, and calls `LLEnvironment::handleEnvironmentPush` (`:2604`).
That dispatches on the action into three cases (`:2635`–`:2647`):

- `handleEnvironmentPushClear` — the experience releases the viewer,
- `handleEnvironmentPushFull` — a whole day cycle,
- `handleEnvironmentPushPartial` — a sky/water fragment merged over the current
  one,

each with a transition time. The result is an *injected* environment
(`LLSettingsInjected`, `DayInjection` at `:740`) that sits above the region's
`ENV_REGION` layer rather than replacing it — which is why releasing it restores
whatever the region says without a refetch.

## What is missing here

- **`sl-wire` / `sl-proto`**: a typed encoder and decoder for the
  `PushExpEnvironment` parameter list, beside the other `GenericMessage`
  features. The envelope already goes both ways
  (`SimSession::send_generic_message`, `Event::GenericMessage`), so nothing new
  is needed on the wire — only a codec, so a caller states an experience id, an
  action and a transition instead of packing strings.
- **The viewer**: an injected-environment layer in
  `sl-viewer-world-scene/src/environment.rs`. Today `EnvironmentState` has two
  levels (the shared grid environment and a locally pinned fixed sky); this
  needs a third that outranks the region and is cleared on release — and the
  clear must restore the region's settings without asking the grid again, as
  the reference's does.
- **`sl-fake-grid`**: a `timeline::Action` for it, so a scenario can push an
  experience environment and take it away again
  ([[test-fake-grid-timeline]] already carries every other scripted action).

## Done (2026-09-10)

All three, plus the LLSD groundwork the parameter needed.

- **`sl-llsd`**: `parse_llsd_serialized` / `to_llsd_serialized` /
  `LlsdEncoding` — the reference's `LLSDSerialize::deserialize` / `serialize`,
  header line and all. Parameter 0 of the push is exactly one of those
  self-describing payloads, and so is an EEP settings asset: `sl-proto`'s own
  private copy of that reader (`settings_asset_llsd`) is now a delegation.
- **`sl-wire`** (`environment_push.rs`): `ExperienceEnvironmentPush`,
  `EnvironmentPushAction` (`Clear` / `Full { asset_id }` / `Partial { sky,
  water }`), `build_environment_push_params` / `parse_environment_push`. The
  experience id rides in the message **invoice**, not the parameter list, which
  the round-trip test pins. An `action` naming none of the three is *rejected*,
  not tolerated.
- **`sl-proto`**: `Event::ExperienceEnvironmentPush`, decoded off both
  `GenericMessage` and `LargeGenericMessage` (the reference dispatches both
  through one handler); a push that will not decode is forwarded raw rather
  than dropped. Server side, `SimSession::send_experience_environment_push`.
  Also `sky_with_pushed_values` / `water_with_pushed_values` — the shallow
  per-key overlay `LLSettingsInjected::injectExperienceValues` does.
- **The viewer**: `PushedEnvironment` in
  `sl-viewer-world-scene/src/environment.rs` — the reference's `ENV_PUSH`,
  above the parcel's settings and below the local layer, with every injected
  value filed under the experience that pushed it so one release leaves
  another's standing. Per-key injections are folded into *every frame* of the
  cycle in force rather than pinning the sampled one, so the day keeps
  animating. `ingest_experience_environment_push` runs the events and holds a
  `Full` push until its settings asset resolves.
- **`sl-fake-grid`**: `Action::PushExperienceEnvironment`, with a timeline test
  driving the real client through a push and its release.

Two things deliberately left, both filed as
[[protocol-experience-parcel-recheck]]: the reference re-checks on every parcel
change whether each injecting experience is still allowed there
(`DayInjection::testExperiencesOnParcel`, over an `ExperienceQuery`
capability this workspace does not have), and it blends *per key* over the
transition rather than cross-fading the whole environment as this does.

One reference behaviour worth not "fixing": the overlay is shallow, so a push
naming a legacy-haze key (`ambient`, `blue_horizon`, …) at the top level is
shadowed by the `legacy_haze` sub-map every EEP sky carries and changes
nothing. `get_color` / `get_float` in `llsettingssky.cpp` read the sub-map
first, so the reference does the same; a script that means it pushes a whole
replacement `legacy_haze` map.

## Why it is worth doing

Two reasons beyond completeness. It is the *only* live environment change in
the protocol — an estate settings change reaches a viewer through a `RegionInfo`
and a refetch ([[viewer-environment-not-refetched-on-region-info]]), which is a
different mechanism with a different feel (no transition, whole-region). And an
experience that pushes a sky and never releases it is a real
grief/permissions surface: a viewer that ignores the push cannot show the user
what is happening to their sky, and cannot offer the reference's own way out.
