---
id: viewer-automation-world-model
title: World locators — find and read objects and avatars
topic: viewer
status: done
origin: viewer automation design (2026-09-28)
points: 8
blocked_by: [viewer-automation-protocol]
refs: [viewer-world-test-harness, viewer-automation-world-aim]
---

## Done (2026-09-28)

**Protocol** (`sl-automation-proto/src/world.rs`):

- `WorldLocator`: `kind` (`avatar` / `object` / `attachment`), `own` (the
  own avatar and what it wears), `name`, `full_id`, `local_id`, `owner`,
  `pcode`, `hover_text`, `near` (`Near { to: Anchor, radius }`, with
  `Anchor::Point` in region-local metres or `Anchor::OwnAvatar`) and `nth`.
  Builders `own_avatar()`, `named`, `owned_by`, `nearest_to`, and so on.
- `WorldNode`: kind, own, full id, local id, pcode, name, description,
  owner, position and rotation (region-local, Second Life axes, `w ≥ 0`),
  scale, parent, children, attachment point, worn by, sitting on,
  selected, hover text and name-tag text.
- Two error kinds: `WorldAmbiguous` (every candidate) and `WorldTimedOut`.
  The timeout carries the things whose name or owner never arrived.

**Viewer** (`sl-viewer-automation`):

- `WorldModel` is a `SystemParam`, with `world_snapshot(&mut World)` for
  exclusive callers.
  - It reads `ObjectState`, `AvatarState`, `SelectionSet`, `SlIdentity`,
    the transforms and the name tags' `TagContent`.
  - The order is fixed: avatars first (the own one leading, then by agent
    id), then objects and attachments by local id.
- `ObjectFacts` and `WorldModelPlugin` exist because the object stream does
  not carry an object's name, description or owner. The plugin folds every
  `ObjectProperties`, `ObjectPropertiesFamily`, and update-with-properties
  event into `ObjectFacts`, keyed by full id, whoever asked for it.
- `find_world` resolves a locator over a snapshot. `near` orders the
  matches nearest first and drops anything with no world position (a HUD
  attachment). Then `nth` picks one.
- `WorldQuery` (`WorldWant::All` / `One`) is polled once a frame.
  - Each poll finds the things that match every criterion except name and
    owner and still lack the one being compared.
  - For objects among them, it sends one `RequestObjectPropertiesFamily`
    per query. That is the request a hover makes, and it selects nothing.
    Avatar names are fetched by the avatar layer; the query only waits for
    them.
  - It answers only when nothing is unresolved. A `One` query waits while
    nothing matches and fails at once on several matches.

**Decisions worth knowing:**

- **"Nearest" is not a field.** It is `near` plus `nth(0)`
  (`nearest_to`), so a nearest-thing locator is also strict.
- **"Self" is `own_avatar()`.** That is kind `avatar` with `own = true`,
  and `own` also covers the attachments the own avatar wears.
- **Positions are relative to the agent's current region.** A thing in a
  neighbouring region reads outside `0..256`.
  - An avatar is placed by its tracked *object* entity, which carries the
    region basis like every object. The anchor (a sphere or a body root) is
    drawn in its own basis: its rotation read 90° off about X until this was
    fixed.
  - A coarse-only avatar has a position but no rotation.
- **`TagContent` moved down to `sl-viewer-world-api`**, with `TagLine`,
  `TagLineSize` and the two font-size constants. `sl-viewer-world-objects`
  re-exports them, so no user changed. Without the move, the automation
  crate would have needed the renderer crate, and with it
  `sl-viewer-platform` and the whole audio stack, just to read what a name
  tag says.
- **No request variants yet.** `find_world` / `wait_for_world` land in the
  proto with the executor, their first consumer. The executor also installs
  `WorldModelPlugin`.

**Tests:**

- Proto: criteria, JSON round-trips, refused misspellings, `Display`, and
  the world timeout message.
- Resolver units: order, `nth`, proximity order and radius, and a missing
  own avatar.
- Viewer `automation_world.rs`, in a `WorldTest` fixture with two named
  avatars (one turned 90°), a two-prim linkset, a turned prim with floating
  text, and a worn hat:
  - An avatar is found by name. Its id, position, rotation and name tag
    match the fixture, and no property request goes out.
  - The own avatar lists its hat as a child.
  - A prim queried by name *before* any properties arrive waits with three
    unresolved and asks exactly once for each of the three prims (not the
    hat). Once the replies land it answers with a readout that equals the
    fixture, selection included.
  - Every other readout is checked: child prim parent and position, hover
    text, rotation, attachment point and wearer, nearest to the own avatar,
    and by owner.
  - An ambiguous name lists both candidates.
  - A name that never arrives times out, naming the three unresolved
    prims.

Context: [context/automation.md](../context/automation.md).

A test about the world needs to say "the prim named *Door*", "Avatar Two",
"my own avatar" or "the nearest tree" and read its state.

## Wanted

- `WorldLocator` and `WorldNode` in `sl-automation-proto` (this task is
  their first consumer): object / avatar / self / attachment, by name, full
  id, local id, owner, pcode, proximity to a point or to the own avatar,
  hover text, with `nearest` / `nth`.
- Resolution over the viewer's own tracking: `ObjectState`
  (`sl-viewer-world-api/src/object_graph.rs`), `AvatarState` and
  `AvatarPickTarget` (`world_vocabulary.rs`), `SceneObject` /
  `ObjectDebugInfo`, attachment nodes, and the name-bearing
  `ObjectProperties` — requesting properties for candidates that have none
  yet, since an object's name arrives separately from its update.
- Readout: full id, local id, name, description, owner, pcode / kind,
  region-local position and rotation (from `GlobalTransform`, as the scene
  dump does), scale, parent / link set, attachment point, sit state,
  selected, hover text, name-tag text.

Acceptance: in a `WorldTest` fixture an avatar is found by name, a prim by
name once its properties arrive (and a query issued before they do waits
for them), and every readout matches the fixture.
