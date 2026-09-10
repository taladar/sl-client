---
id: protocol-experience-environment-push
title: An experience can set the sky, and nothing in this stack can say so
topic: protocol
status: ready
origin: reading the reference while scoping the environment leg of test-fake-grid-timeline (2026-09-09)
points: 3
refs:
  [
    viewer-environment-not-refetched-on-region-info,
    test-fake-grid-timeline,
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

## Why it is worth doing

Two reasons beyond completeness. It is the *only* live environment change in
the protocol — an estate settings change reaches a viewer through a `RegionInfo`
and a refetch ([[viewer-environment-not-refetched-on-region-info]]), which is a
different mechanism with a different feel (no transition, whole-region). And an
experience that pushes a sky and never releases it is a real
grief/permissions surface: a viewer that ignores the push cannot show the user
what is happening to their sky, and cannot offer the reference's own way out.
