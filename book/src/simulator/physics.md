# Physics

`sl-fake-grid` runs enough physics for the physics-shaped events and library
functions to tell a script the truth: a physical object falls, lands, slides
to a stop and comes to rest; it moves and turns under forces, torques,
impulses, pushes and set velocities; a hover holds it up and a look-at turns
it; a vehicle's motors drive it; a keyframed motion plays it; a pathfinding
character walks it. A scripted prim it lands on hears `collision_start`,
`collision` and `collision_end`; a prim under `llVolumeDetect` hears what
passes through it; the ground raises `land_collision_start`,
`land_collision` and `land_collision_end`; an object starting and stopping
raises `moving_start` and `moving_end`. The code is `sl_fake_grid::physics`,
with the vehicle model in `vehicles`, keyframed motion in `keyframes` and what
a script's call does to the region in `script_physics`.

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
  its velocity: last tick's, with the impulses and pushes given since added
  (or what `llSetVelocity` set, the impulses on top of it), then the tick's
  accelerations — gravity (9.80665 m/s², times the material's gravity
  multiplier, less the share `llSetBuoyancy` takes away) or a vehicle's whole
  model, and the steady force over the mass. A hover's pull, a character's
  steering, `llMoveToTarget`'s draw and a drag's pull replace the part of
  the velocity they hold. It moves axis by axis, east, north, up, against
  the solid prims of other objects and the ground, and stops at the first
  surface in its way. An object that moved earlier in the tick is where it
  moved to for the ones after.
- **Rotation.** Each physical object has a spin, kicked by rotational
  impulses, set by `llSetAngularVelocity`, driven by the steady torque (over
  the mass, a unit inertia) and a vehicle's angular motor, or replaced by a
  look-at's or a spin's turn. The axes its `STATUS_ROTATE_*` flags lock are
  taken out of it in the object's frame, and it turns by what is left.
- **Friction.** An object resting on something slows by Coulomb's friction —
  its material's coefficient times the gravity it feels, none for a
  weightless one — in its sliding and its spin. A vehicle's model has its
  own friction, and what steers an object sets its motion itself.
- **What it does not do.** No collision response in rotation (a box landing
  on its corner does not tip), no stacking under load, no inertia tensor,
  and a keyframed object moves through what is in its way.
- **Avatars.** An avatar is a 0.5 × 0.5 × 1.9 m box round its position, its
  turn ignored. The avatar controller (`sl_fake_grid::movement`) is handed
  the solid prims within 16 m: one too tall to step on (more than 0.35 m
  above the feet, a height not measured on either grid) stops a walk where
  the avatar meets it, a lower one is stepped on to, a prim's top is stood on
  (the collision plane level at its height), walking off one more than a step
  high is a fall, and a climb stops with the head at a prim's underside. A
  phantom or volume-detecting prim is walked through, and a prim the avatar
  already stands in does not hold it. `llPushObject` of an avatar adds the
  impulse over an avatar's mass (OpenSim's ubODE density, 60, times the box,
  unmeasured on either grid) to its velocity; a push with a lift takes it off
  the ground.
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
The ground is told only to the object's root. `llCollisionFilter` keeps a
prim's collision events to what has the name and key it gives (or, not
accepting, to what has not).

## Mass

`llGetMass` and its siblings read the whole linkset, in Linden's units
(kilograms over 100). Second Life's follows each prim's density and the
scale of its physics shape, which is rebuilt at once after a change of scale
unless one was rebuilt within the last second, and then a second after that
one (`physics/material`: three reads after a cube went back to 0.5 m read
its 1 m mass twice). OpenSim's is what the object weighed when the region
first saw it, its density and scale changes notwithstanding, or what
`osSetInertia*` set. The centre of mass is the mass-weighted middle of the
prims' boxes.

## Keyframed motion

The region plays an object's frames a tick at a time, each frame moving and
turning it by its share relative to where the frame found it: a motion
OpenSim lets `llSetPos` move aside goes on from there, a stop leaves the
object where it is, reverse undoes each frame backwards, a loop starts again
and a ping-pong turns round. A frame shorter than a tick is done in one. A
start raises `moving_start` and an end, a stop of a playing motion or a
pause `moving_end`. Turning an OpenSim object physical deletes its motion
(with a `moving_end`); Second Life refuses the physics instead.

## Vehicles

OpenSim's vehicle model (`ubOde/ODEDynamics.cs`) on the kinematic solver:
the type presets and each parameter's clamps; the linear and angular motors
with their decay; friction timescales; hover and buoyancy; linear and angular
deflection; vertical attraction and banking. Both grids clamp the linear
motor at 100 m/s and the angular at 4π rad/s (`physics/vehicles`). The model
has no camera to steer a mouselook vehicle by, and turns the object as a unit
inertia.

## Pathfinding characters

The fake grid keeps no navmesh: all its ground is walkable, and a point lies
on it within a metre of the ground. A character is physical, walks in a
straight line over the ground at its speed towards its goal — a point, a
patrol's points in turn, a wander's points (the same for the same object in
every run), what it pursues, or a point away from what it flees —, raises
`path_update` `PU_SLOWDOWN_DISTANCE_REACHED` within twice its radius and
`PU_GOAL_REACHED` within its radius, and stops (`physics/character`).

## Where the grids differ

From `events/collision` and the `physics/*` probes
(`book/src/gridspec/lsl.md`):

| | Second Life | OpenSim |
| --- | --- | --- |
| a landing | stops dead | bounces at the product of the two materials' restitutions (wood 0.5, so wood on wood 0.25; the ground 0.001 of the object's) when faster than 1.5 m/s — ubODE's table and `bounce_vel` |
| an object at rest | sleeps a quarter of a second later and touches nothing, so its contacts end | keeps touching what it rests on |
| a script busy in a handler when a collision begins | hears nothing of it | hears it all once it is free |
| the position `land_collision_*` is handed | the ground under the object's root | the root |
| `moving_start` / `moving_end` | for any object starting and stopping | for keyframed motion alone (`KeyframeMotion` is OpenSim's only caller) |
| an impulse or push given to a prim that is not physical | lost | applied when it turns physical |
| a force set while not physical | pushes once physical (unmeasured) | kept for `llGetForce` only |
| mass | follows density and, late, scale | as first seen |
| `VEHICLE_TYPE_NONE` on a moving vehicle | stops it | it coasts |
| a keyframed motion soon after a change of the physics shape | waits until 0.4 s after the change | starts at once |
| `KFM_CMD_PLAY` with nothing playing | plays the last motion given | nothing |
| a drag of a physical prim | pulls it towards the grab | leaves it |

A spin of a physical prim turns it towards the rotation asked on both
(`object-touch-grab`, leg 10). A physical object made non-physical drops its
own contacts unheard while what it touched hears them end — OpenSim's
measured behaviour, and consistent with everything Second Life showed.

The sleep's quarter of a second is the measurement: aditi raised eleven
`collision` events between a landing's start and its end at 45 frames a
second, which is exactly what the fake grid raises ticking at the same frame.
The bounce reproduces OpenSim's double landing exactly: a two-metre drop lands
at 5.4 m/s, comes back up at 1.35 m/s, and lands again too slowly to bounce.

`llMoveToTarget` shrinks the distance left by `e` every tau, against gravity:
a target a metre up with a tau of 0.3 is within a tenth of a metre after
0.69 s, as Second Life measured (OpenSim 0.71 s). A tau of 0 or less moves
nothing on either grid, a target more than 65 m away nothing on Second Life,
and a prim that is not physical keeps its target without moving. A look-at
shrinks the angle left by `e` every damping on Second Life, and spins the
object at the angle left times π × min(1, 1/strength) on OpenSim
(`SceneObjectPart.UpdateLookAt`): a quarter turn with a strength of 1 and a
damping of 0.5 is within a tenth of a radian after 1.36 s and 0.87 s, as each
grid measured.
