---
id: protocol-experience-parcel-recheck
title: An experience's sky follows you off its land, and nothing takes it back
topic: protocol
status: done
origin:
  split out of protocol-experience-environment-push while implementing it
  (2026-09-10)
points: 2
refs: [protocol-experience-environment-push]
---

Context: [context/protocol.md](../context/protocol.md).

[[protocol-experience-environment-push]] landed the push itself: an experience
can inject a sky over the region's, and releasing it puts the region's back.
Two pieces of the reference's behaviour around that injection were left out,
and both are about the injection *outliving* what entitles it to exist.

## The parcel re-check

An experience is admitted per land, not per session. The reference hooks the
parcel-change signal from inside the injection itself
(`DayInjection::onParcelChange` → `testExperiencesOnParcel`,
`indra/newview/llenvironment.cpp`) and asks the region, over an
**`ExperienceQuery`** capability, which of the currently-injecting experiences
are allowed on the parcel the agent has just walked onto. The ones that are not
are cleared, each with its own transition.

Without it an experience's sky follows the agent off the land that admitted it
and stays until the script thinks to release it — which a script that has lost
the agent will never do. That is the grief surface the push task named, arriving
by the back door.

This needs:

- **`sl-wire`**: the `ExperienceQuery` capability — a GET carrying the parcel id
  and the experience ids to test, replying with the ones that pass. It is not in
  this workspace's experience module, which has the other ten
  (`GetExperienceInfo`, `RegionExperiences`, …) but not this one.
- **The viewer**: hang the query off `EnvironmentState::set_agent_parcel`, which
  already knows when the agent steps over a line, and clear the experiences it
  answers "no" for.
- **`sl-fake-grid`**: a `Scenario` knob for which experiences a parcel admits,
  so a timeline can walk an avatar off the land and assert the sky comes back.

## The per-key blend

The reference blends each injected *key* toward its new value over the push's
transition time (`LLSettingsInjected::injectSetting` schedules an `Injection`
whenever the transition is over `0.1` seconds; `applyInjections` interpolates
per key each tick). The viewer here cross-fades the **whole** environment over
that time instead, reusing the manual-transition machinery.

The end states are identical and the intermediate frames are close, so this is a
fidelity item rather than a bug — but a cross-check that photographs a sky
mid-transition would see the difference, and it is the kind of divergence that
is much easier to fix while the layer is fresh than to explain later.

## Done (2026-09-11)

Both, plus a third way an injection outlived its entitlement that reading the
reference for this turned up.

- **`sl-wire`** (`experience/`): the `ExperienceQuery` capability — the twelfth
  and last of the family. `experience_query(parcel_id, &ids)` writes
  `?parcelid=N&experiences=a,b` (the parameter name only before the *first* id,
  as the reference's own string building does, so an empty list omits it),
  `parse_experience_query` reads it back, and `parse_experience_query_reply` /
  `build_experience_query_response` carry the
  `{ experiences: { "<id>": bool } }` answer. An entry the viewer cannot read is
  **skipped**, never read as a refusal: the reference acts only on the entries
  that say *no*.
- **`sl-proto`**: `CAP_EXPERIENCE_QUERY` (requested and served),
  `Command::QueryParcelExperiences`, `Event::ParcelExperiences`, and the
  `EXPERIENCE_QUERY_TAG` stamping convention — the reply names only experiences,
  so the runtimes stamp the queried parcel into it the way
  `AVATAR_PICKER_SEARCH_TAG` stamps a query id. Server side,
  `SimExperiences::{set_parcel_experiences, parcel_admits, parcel_experiences}`
  behind a `CapHandler::ExperienceQuery`.
  **A parcel nothing was declared for admits everything**: declaring one is what
  makes it land-scoped, so a fixture that does not care answers the way a grid
  with one region-wide experience does.
- **The viewer**: `EnvironmentState::standing_parcel` — the parcel the agent is
  *standing* on, which is **not** `parcel_id` (that one is `None` wherever a
  region has per-parcel environment overrides switched off, and where an
  experience may keep its sky is a question about land, not about environment
  versions). `set_standing_parcel` arms the re-check, `query_parcel_experiences`
  asks, `ingest_parcel_experiences` releases each refused experience over the
  reference's `TRANSITION_FAST`, and an answer for a parcel the agent has since
  left is discarded rather than acted on.
- **`sl-fake-grid`**: no new crate code — the knob is
  `SimExperiences::set_parcel_experiences` driven through `with_sim`, the way
  every other grid-side change is. `tests/client_end_to_end.rs` drives the real
  client through the capability against a declared and an undeclared parcel.

**The per-key blend**, in full: `PushedValue` / `ValueBlend` in
`sl-viewer-world-scene/src/environment.rs` are the reference's pairing of
`mOverrideValues` with an `Injection`, and
`sl_proto::{sky,water}_with_blended_values` apply them — a key listed in the mix
map lands only that far from the frame's own value toward the pushed one. A
**partial** push and its release no longer cross-fade the whole environment at
all; a whole settings asset going in, or a whole track coming out, still does,
which is the reference's own split (`injectSetting` / `removeInjection` against
`animateSkyChange` / `animateWaterChange`). Releasing fades each key back
**out** rather than snapping it, and the experience leaves `experiences()` the
moment it releases — the fade outlives the experience, not the other way round,
so a key on its way off screen is not re-asked about on the next parcel line.
`blend_llsd_value` is `interpolateSDValue`: numbers lerp (integers rounding),
maps and arrays recurse, `sun_rotation` / `moon_rotation` slerp (lerping a
quaternion's components walks off the unit sphere), and anything with no halfway
— strings, uuids, a mismatched pair — switches at the reference's `BREAK_POINT`
of `0.5`.

**A third one, not in the task as written.** `LLEnvironment::onRegionChange`
drops every injected environment *unconditionally* — the capability test beside
it is commented out, under "for now environmental experiences do not survive
region crossings". This viewer's pushed layer used to cross a region line and
stay, which is the same grief surface by a longer route: the script holding the
sky is back where the agent left it, and the destination has its own idea of
which experiences it admits. `clear_pushed` now takes a transition and is called
on `RegionHandshakeComplete` over `TRANSITION_DEFAULT`; it had no caller at all
before this.
