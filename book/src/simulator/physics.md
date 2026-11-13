# Physics

`sl-fake-grid` runs enough physics for the physics-shaped events and library
functions to tell a script the truth: a physical object falls, lands and comes
to rest; a scripted prim it lands on hears `collision_start`, `collision` and
`collision_end`; a prim under `llVolumeDetect` hears what passes through it;
the ground raises `land_collision_start`, `land_collision` and
`land_collision_end`; an object starting and stopping raises `moving_start`
and `moving_end`; and `llMoveToTarget` draws an object to its target. The code
is `sl_fake_grid::physics`.

## A kinematic solver, not a rigid-body engine

The decision (`server-world-collision-and-physics`): a purpose-built
kinematic solver, not `rapier3d`. The fake grid's job is to be reproducible
rather than plausible, it is started dozens of times in one `cargo test`, and
every event above needs only overlap tests, a ground clamp and velocity
integration.

- **Shapes.** Every prim is the axis-aligned box its scale and rotation
  bound; a physical object is the union of its prims' boxes. Meshes, sculpts
  and cut prims collide as their bounding boxes.
- **Motion.** Each tick moves every physical object, in local-id order, by
  its velocity: gravity (9.80665 m/s², less the share `llSetBuoyancy` takes
  away) or, while `llMoveToTarget` holds it, the pull of its target. It
  moves axis by axis, east, north, up, against the solid prims of other
  objects and the ground, and stops at the first surface in its way. An
  object that moved earlier in the tick is where it moved to for the ones
  after.
- **What it does not do.** No rotation, friction, sliding, stacking under
  load, forces, impulses or vehicles, and an avatar is never pushed.
- **Avatars.** An avatar is a 0.5 × 0.5 × 1.9 m box round its position, its
  turn ignored. The avatar controller (`sl_fake_grid::movement`) is handed
  the solid prims within 16 m: one too tall to step on (more than 0.35 m
  above the feet, a height not measured on either grid) stops a walk where
  the avatar meets it, a lower one is stepped on to, a prim's top is stood on
  (the collision plane level at its height), walking off one more than a step
  high is a fall, and a climb stops with the head at a prim's underside. A
  phantom or volume-detecting prim is walked through, and a prim the avatar
  already stands in does not hold it.
- **Determinism.** Everything is a function of the region: objects in
  local-id order, contacts in a sorted map, no clock but the tick's step.
  Two runs of one seed raise the same events in the same ticks
  (`sl-fake-grid/tests/scripts.rs`, `a_falling_box_collides`).

## Contacts and events

A prim touches what stands within a centimetre of it: each physical object's
prims touch the solid prims of other objects, an avatar touches the prims its
box meets, and a physical object whose box reaches the ground touches the
land. A volume-detecting prim instead touches the physical objects and
avatars whose boxes overlap it — it does not stop them.

Every tick, each prim's scripts hear what changed:

| contact | event |
| --- | --- |
| begun | `collision_start`, or `land_collision_start(position)` |
| going on | `collision` (never for a volume-detecting prim), or `land_collision(position)` |
| over | `collision_end`, or `land_collision_end(position)` |

Both sides of a contact between two prims hear of it, each detecting the
other prim; a physical object falling through a volume-detecting prim hears
nothing of it. The detected block of a prim is its key, name and owner, its
position, rotation and velocity, and `llDetectedType` `ACTIVE` for a physical
object or `PASSIVE` otherwise, with `SCRIPTED` when its object holds a script;
of an avatar, `AGENT` (with `ACTIVE` on Second Life). `llDetectedLinkNumber`
is the link of the prim that was touched. A collision of a child prim reaches
its own scripts when one of them handles the event and its root's when none
does, as `llPassCollisions` changes, by the rule the grid passes touches by.
The ground is told only to the object's root.

## Where the grids differ

From `events/collision` and `physics/targets` (`book/src/gridspec/lsl.md`):

| | Second Life | OpenSim |
| --- | --- | --- |
| a landing | stops dead | bounces at the product of the two materials' restitutions (wood 0.5, so wood on wood 0.25; the ground 0.001 of the object's) when faster than 1.5 m/s — ubODE's table and `bounce_vel` |
| an object at rest | sleeps a quarter of a second later and touches nothing, so its contacts end | keeps touching what it rests on |
| a script busy in a handler when a collision begins | hears nothing of it | hears it all once it is free |
| the position `land_collision_*` is handed | the ground under the object's root | the root |

A physical object made non-physical drops its own contacts unheard while what
it touched hears them end — OpenSim's measured behaviour, and consistent with
everything Second Life showed.

The sleep's quarter of a second is the measurement: aditi raised eleven
`collision` events between a landing's start and its end at 45 frames a
second, which is exactly what the fake grid raises ticking at the same frame.
The bounce reproduces OpenSim's double landing exactly: a two-metre drop lands
at 5.4 m/s, comes back up at 1.35 m/s, and lands again too slowly to bounce.

`llMoveToTarget` shrinks the distance left by `e` every tau, against gravity:
a target a metre up with a tau of 0.3 is within a tenth of a metre after
0.69 s, as Second Life measured (OpenSim 0.71 s). A tau of 0 or less moves
nothing on either grid, a target more than 65 m away nothing on Second Life,
and a prim that is not physical keeps its target without moving.
