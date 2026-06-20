# Nearby Avatars & Viewer Effects

Beyond the full scene graph (see [3D World Information](world.md)), the
simulator keeps a viewer aware of the avatars around it cheaply, and lets
viewers show each other small transient cues — where someone is looking, what
they are pointing at, the editing beam. This chapter covers three related
surfaces: **coarse locations** (the minimap feed), **viewer effects**, and
**agent tracking**.

## Coarse locations (the minimap feed)

`CoarseLocationUpdate` is the low-resolution position feed a viewer draws on its
minimap. The simulator pushes it periodically; it carries, for each nearby
avatar, a whole-metre position relative to the region's south-west corner and
the avatar's id. It is far cheaper than a full object update — one byte per axis
— so it covers avatars the viewer has not yet streamed as full objects.

`Event::CoarseLocationUpdate` surfaces it as a list of `CoarseLocation`
(`agent_id`, `x`, `y`, `z`), plus two indices into that list: `you` (the agent's
own entry) and `prey` (the tracked agent, if any). Both are `Option<usize>` — a
negative wire index means "absent". Heights arrive in units of four metres on
the wire, so `CoarseLocation::z` is a metre value that is always a multiple of
four, up to `1020`.

## Viewer effects

A **viewer effect** (`ViewerEffect`) is a short-lived visual cue one viewer asks
others to render. `ViewerEffectType` enumerates Linden Lab's HUD-effect codes;
the ones a normal viewer emits are the gaze and pointing hints and the beam
family:

- **Look-at** (`ViewerEffectType::LookAt`) — where an avatar is looking. The
  payload (`ViewerEffectData::LookAt`) names a source avatar, an optional target
  object, a global target position, and a `LookAtType` (idle, mouselook, focus,
  …).
- **Point-at** (`ViewerEffectType::PointAt`) — what an avatar is pointing at,
  with a `PointAtType` (select, grab, …).
- **Beam / glow / point / sphere / spiral / edit** — the
  `ViewerEffectData::Spiral` family (the viewer's `LLHUDEffectSpiral`): a source
  object, an optional target object, and a global position. This is the familiar
  selection/editing beam.

Each effect also carries a unique `id`, the source `agent_id`, a `duration` in
seconds, and an `RGBA` `color`. The effect-specific payload lives in a
`TypeData` blob on the wire; the well-known layouts above are decoded into the
typed `ViewerEffectData` variants, and anything else (or a payload whose length
does not match its type) is kept verbatim as `ViewerEffectData::Raw`.

A client sends effects with `Command::ViewerEffect` (a batch — one message may
carry several) and receives others' effects as `Event::ViewerEffect`.

## Agent tracking

Two estate/lookup messages locate a specific agent:

- `Command::TrackAgent` (`TrackAgent`) asks the simulator to track an agent; its
  coarse position then arrives in the `CoarseLocationUpdate` feed, with the
  `prey` index pointing at it.
- `Command::FindAgent` (`FindAgent`) asks for an agent's global position
  outright. The simulator answers with a `FindAgent` carrying the located global
  `(x, y)` positions, surfaced as `Event::FindAgentReply`. (There is no separate
  reply message — the request and the reply are the same `FindAgent` message,
  distinguished by whether its location block is filled in.)

The server side decodes the inbound client messages as
`ServerEvent::ViewerEffect`, `ServerEvent::TrackAgent` and
`ServerEvent::FindAgent`, and can push the sim-to-viewer side with
`SimSession::send_coarse_location_update`, `send_viewer_effect` and
`send_find_agent_reply`.

---

> **In this codebase**
>
> - Types are in `sl-proto/src/types/nearby.rs`: `CoarseLocation`,
>   `ViewerEffect`, `ViewerEffectType`, `ViewerEffectData` (with `from_wire` /
>   `to_wire`), `LookAtType`, `PointAtType`.
> - Commands `ViewerEffect`, `TrackAgent`, `FindAgent`; the `Session` methods
>   are `send_viewer_effect`, `track_agent`, `find_agent`; the wire encoders are
>   `send_viewer_effect` / `send_track_agent` / `send_find_agent` in
>   `sl-proto/src/session/circuit.rs`. The inbound `CoarseLocationUpdate`,
>   `ViewerEffect` and `FindAgent` (reply) decode into `Event` variants in
>   `sl-proto/src/session/methods.rs`.
> - Server events `ViewerEffect`, `TrackAgent`, `FindAgent` and the sim encoders
>   `send_coarse_location_update` / `send_viewer_effect` /
>   `send_find_agent_reply` are in `sl-proto/src/sim_session.rs`.
> - REPL commands `viewer_effect` (effect type and `LookAtType`/`PointAtType`
>   accept a name or a numeric code; the payload layout is chosen by `data=` or
>   inferred from the effect type), `track_agent`, `find_agent`.
