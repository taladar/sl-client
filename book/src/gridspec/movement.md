# Movement

A viewer does not move its own avatar. It states which controls are held in
its `AgentUpdate`s — forward, back, sideways, up, down, fly — with the way
the body faces, and the simulator decides where the avatar goes. What comes
back is the avatar's own object, updated like any other: a position, a
velocity, an acceleration, a rotation and, for an avatar, the plane it
stands on. The viewer draws its avatar from those and predicts between them.

Measured on 2026-10-08 by the `agent-movement` conformance case, which holds
one control after another for a few seconds each and samples every update
of its own avatar and every statement of its animation set:

- three runs on aditi, with two avatars, in two mainland regions (one run
  started in a region's corner and flew over the border, and is used only
  where a row says so);
- on the local OpenSim, on a hundred-metre square of ground flattened for
  the purpose (`terrain modify fill 25 -rec=20,130,100,100` in the
  north-eastern region). The rest of that block is hills: a first run on
  them read a walk as anything from 2.7 to 3.1 m/s, which is the slope and
  not the grid.

The case holds both live grids to very little — the avatar walks when told
to, and comes down when it stops flying — and records the rest. The fake
grid decodes an `AgentUpdate` and moves nothing, so the last column says
only that throughout; the avatar controller is the roadmap's
`server-world-agent-movement`, which is built to this chapter.

## Speeds

Speeds are the velocity the grid reported at a steady pace, on level
ground. The positions agree with them: the distance covered over the time
it took came to within two per cent of the reported speed on OpenSim and
within five on Second Life, whose ground was not level.

| control | Second Life | OpenSim | fake grid |
| --- | --- | --- | --- |
| forward (`AT_POS`) | 3.20 m/s (3.17 to 3.26 over three runs) | 3.145 m/s | does not move |
| backward (`AT_NEG`) | 3.2 m/s, the same as forward | 3.145 m/s | does not move |
| sideways (`LEFT_POS`, `LEFT_NEG`) | 3.0 to 3.2 m/s | 3.14 m/s | does not move |
| forward with always-run on (`SetAlwaysRun`) | 5.13 m/s; backward the same | 5.10 m/s; backward the same | does not move |
| forward with the `FAST_AT` bit | 3.2 m/s: the bit changes nothing | 3.14 m/s: the same | does not move |
| the `NUDGE_AT_POS` bit, held | 3.2 m/s for as long as it is held, with the walk animation | 3.14 m/s, the same | does not move |
| forward while crouching (`UP_NEG` on the ground) | 2.0 to 2.1 m/s | 3.14 m/s: a crouch changes the animation and not the speed | does not move |
| level flight (`FLY` and `AT_POS`) | 16.0 m/s | 12.58 m/s | does not move |
| level flight with `FAST_AT` | 15.9 m/s: nothing | 12.58 m/s: nothing | does not move |
| climbing (`FLY` and `UP_POS`) | 15.9 to 16.0 m/s | 16.36 m/s | does not move |
| descending (`FLY` and `UP_NEG`) | 22.8 m/s | 16.35 m/s | does not move |
| a ceiling | none in a thirty-second climb to 520 m: the same speed in every five-second window | none in the same climb | none |
| falling (flight let go at about 420 m) | 52.8 m/s fastest reported, 387 m in 10.6 to 11.1 s | 50.1 m/s, 425 m in 12.0 s | does not move |

## Starting, stopping and turning

| behaviour | Second Life | OpenSim | fake grid |
| --- | --- | --- | --- |
| a control stated once, with nothing after it but the client's once-a-second `AgentUpdate` | the avatar walks for as long as the control stands (six seconds measured) | the same | does not move |
| a control re-stated a hundred times a second | the same speed, the same rate of updates back, and no alert | the same | accepts it |
| letting go of a walk | at rest within half a second, 0.9 to 1.0 m further on | at rest within 0.3 s, 0.6 m further on | does not move |
| letting go of a run | 1.6 to 2.2 m further on | 0.55 to 0.65 m further on | does not move |
| letting go of level flight | 0.7 m further on | no further report: the next update is the avatar at rest | does not move |
| letting go of a climb | 0.1 to 0.7 m higher | 1.9 m higher | does not move |
| a turn on the spot (a new body rotation, no control) | reported back over several updates, and not exactly: four and five degrees short twice, and 78° short once | reported back in one update, to the hundredth of a degree | does not move |

## Jumping, landing and `FINISH_ANIM`

A Second Life simulator stops the avatar where it stands for some of its
animations and waits for the viewer that plays them to say they are over:
one `AgentUpdate` carrying the `FINISH_ANIM` control bit. OpenSim runs the
same animations on its own clock and never reads the bit.

| behaviour | Second Life | OpenSim | fake grid |
| --- | --- | --- | --- |
| the jump key (`UP_POS` on the ground), nothing else said | `pre_jump` joins the animation set and the avatar **never leaves the ground**: it was still there, and still in `pre_jump`, after 5 s on one avatar and 18 s on the other, with the key held throughout | off the ground 0.05 to 0.09 s after the key | does not move |
| the same, with `FINISH_ANIM` sent as `pre_jump` appears | off the ground 0.36 to 0.6 s after the key | the same 0.05 to 0.09 s: the bit changes nothing | does not move |
| the jump | 7.3 to 8.9 m/s upwards at the first report, 3.97 to 3.99 m high, down again 1.9 to 2.3 s after the key is let go | 9.86 m/s, 5.57 m high, down after 2.4 to 2.7 s | does not move |
| the animations of a jump | `pre_jump`, `jump`, then `land` for a moment (0.05 s) or not at all | `pre_jump` for 0.2 to 0.3 s, `jump`, `land` for 0.36 s, `stand` | none |
| the forward key as the avatar comes down from a jump, the landing not reported finished | moves it within 0.4 to 0.7 s: this landing is not held | moves it within 0.3 s | does not move |
| a fall from 390 m, then the forward key | the avatar **stays where it landed** for the eight seconds it was left there, and walks 0.45 s after a `FINISH_ANIM` | `standup` for 1.1 s, then `stand`; the forward key moves it 0.2 s later, with nothing sent | does not move |
| an arrival | one login of three — the one routed to its region's landing point — listed `land` and kept it: the avatar stood through four seconds of the forward key and walked at the first `FINISH_ANIM`, 24 s after arriving. The other two, at the spot the avatar had logged out at, were not held (one listed `land` for a second, the other never) | `falldown`, `land`, `stand` within 2.5 s of arriving on hilly ground; never held | does not move |

The landing point is the same thing that stopped the `region-crossing` case
on 2026-10-07, where ninety seconds of the forward key moved an avatar two
centimetres and a flight freed it: a flight ends the landing, and so would
the one bit.

## What the grid reports, and how often

Both grids report the avatar's own motion as `ImprovedTerseObjectUpdate`s.
Over a whole run OpenSim sent one full `ObjectUpdate` of the avatar among
439 updates, and Second Life eight among a thousand.

| behaviour | Second Life | OpenSim | fake grid |
| --- | --- | --- | --- |
| an avatar at rest | 1.4 to 1.6 updates a second | one update, then nothing: a single report in fifteen seconds | the arrival's update only |
| walking | eight to sixteen a second | on level ground one at the start and then **one every 1.9 to 2.3 s**; five to eleven a second on a slope, where the velocity changes | none |
| climbing or flying at a steady speed | **none**: two updates as a thirty-second climb began and the next when it ended, 450 m higher | about 2.8 a second | none |
| the velocity | the avatar's, and what its positions show | the same | zero |
| the acceleration | zero in every update, falling included | the same | zero |
| the collision plane, on the ground | the ground's normal and its offset | the same: `0 0 1 25` on level ground at 25 m | the stock plane |
| the collision plane, in the air | `0 0 0 1` | `0 0 0 1` | the stock plane |
| the animation set | the locomotion state and two to five assets beside it that are no built-in, changing from one statement to the next; on both avatars a fall, its landing, a hover and level flight were stated *only* as such assets, while `walk`, `run`, `stand`, `crouch`, `pre_jump`, `jump`, `land`, `hover_up` and `hover_down` came as the built-ins | exactly one built-in at a time: `stand`, `walk`, `run`, `crouch`, `crouchwalk`, `pre_jump`, `jump`, `land`, `hover`, `hover_up`, `hover_down`, `fly`, `falldown`, `standup` | none |

## Sitting

A sit is a request the simulator answers. The viewer sends an
`AgentRequestSit` naming the object and the point on it that was clicked;
the simulator answers with an `AvatarSitResponse` — the seat's position and
rotation relative to the object, a camera, an `AutoPilot` flag — and sends
the avatar's own object again, now a child of the seat with a position
relative to it. Standing up is one `AgentUpdate` with the `STAND_UP` bit.

Measured on 2026-10-08 by the `sit-stand` conformance case, four runs on
aditi and five on the local OpenSim, with two avatars: the first rezzes a
half-metre cube on the ground a metre and a half from itself and asks to sit
on it from there, from the ground, from six spots it walks to and, once a
script has given the cube a sit target
(`llSitTarget(<0, 0, 1>, llEuler2Rot(<0, 0, PI_BY_TWO>))`), from two it
flies to; the second asks for the seat while the first is on it. Distances
are along the ground, from the avatar to the cube's centre. The case holds
every grid it can put the question to — the live ones, and each fake
flavour for the legs that need no second resident, no script and no walk —
to the rows marked †.

### The answer

| behaviour | Second Life | OpenSim | fake grid |
| --- | --- | --- | --- |
| a sit on the cube from 1.5 m † | an `AvatarSitResponse` 0.17 to 0.18 s later — one round trip | the same, 0.01 to 0.1 s later | the same |
| the response's `AutoPilot` flag † | **set, in every response**: from 0.5 m and from 52 m, with a sit target and without | the same: `ScenePresence.SendSitResponse` passes `true` | set |
| the avatar on the seat | its object arrives as a child of the seat 22 ms after the response, a frame later and long before the client's `AgentSit` could have reached the simulator: **the request seats it** | in the same instant as the response (`SendSitResponse` calls its own `HandleAgentSit`) | on the client's `AgentSit` (`server-world-sit-and-attach`) |
| the object the response names | the cube asked for | the same | the same |
| the camera offsets and `ForceMouselook` | zero and unset: the script sets none | the same | zero and unset |
| the animation set once seated | no built-in: three to five assets that are none (`gridspec-animations`) | `sit` | none |
| what an observer is sent | the avatar as a child of the seat, at the same offset | the same | residents are not shown to each other |

### Where the avatar is put

| behaviour | Second Life | OpenSim | fake grid |
| --- | --- | --- | --- |
| a scriptless cube, asked for at its centre | at an edge, 0.34 m off the centre and 0.88 m above it, facing outwards | 0.42 m off the centre one way and up to 0.23 m the other, 0.90 m above it, facing outwards | every seat has a sit target |
| the same, asked for at a point on its top face 0.1 / 0.2 m off the centre | 0.34 / 0.20 m off: the point clicked chooses the edge and the place along it | 0.15 / 0.43 m off: the same, by a rule of its own | the point is ignored |
| the response's `SitPosition` for that cube † | **0.33 m below** where the avatar's own update then puts it (0.55 against 0.88 m) | where the update puts it | — |
| a seat with the sit target `<0, 0, 1>` | the avatar 1.35 m above the centre, turned as the target says | the same 1.35 m (`SIT_TARGET_ADJUSTMENT` less 0.05 m, "empirically determined to be what is used in SL") | 0.55 m above, on both |
| the response's `SitPosition` for that seat † | where the avatar is put: 1.35 m | **the target as the script set it**: 1.00 m, 0.35 m below the avatar | Second Life's states where the avatar is put, OpenSim's 0.35 m lower |
| a second avatar on a scriptless seat that is taken | seated on the same cube, 0.54 m higher than the first | seated on the same cube at the spot it would have had alone, the first avatar's or not | — |
| a second avatar on a sit target that is taken | seated as on a scriptless cube: no refusal | the same (`FindNextAvailableSitTarget` falls back to the prim) | — |

Neither response says where a viewer should draw the avatar on both grids;
the avatar's own object update does.

### From a distance

| behaviour | Second Life | OpenSim | fake grid |
| --- | --- | --- | --- |
| a scriptless seat from further off † | answered, and the avatar seated at once, from 3.6, 4.2, 4.3, 6.6, 6.8, 7.3 and 7.5 m; **not answered at all** from 9.3, 10.0, 10.1, 12.6, 12.9, 13.1, 15.3, 15.7, 15.9, 20.5, 21.1, 21.3 and 24.8 m — no response, no alert, and the avatar stays where it is | answered, and the avatar seated at once, from every distance tried: 4.1 to 20.9 m | does not move an avatar |
| a seat with a sit target from further off † | answered and seated at once from 20.5, 21.1, 21.3, 24.8, 44.5, 45.0, 50.9 and 52.0 m | the same from 40.0 m | — |
| a sit on an object the region does not have † | the named alert `SitFailNotSameRegion` ("Try moving closer. Can't sit on object because it is not in the same region as you."), 0.17 s later | **nothing**: the simulator logs "Sit requested on unknown object" | each flavour's |
| a sit on an object in a neighbouring region | the same alert, from that region — measured earlier (see [World](../content/world.md)); no neighbour's object was in view of these runs | the same words as an alert **with no name**, 0.02 s later | not modelled |

The silence past eight or nine metres is what `logout-seated` ran into on
2026-10-06, when aditi "did not answer the sit": that case rezzed its seat a
metre and a half from where its flight to the build location was reckoned to
have ended, and the avatar had come down up to eleven metres from there.

### The ground, standing up and teleporting

| behaviour | Second Life | OpenSim | fake grid |
| --- | --- | --- | --- |
| `SIT_ON_GROUND` | `sit_ground_constrained` joins the animation set; the avatar stays its own root and is reported once or not at all | the animation set becomes `sit_ground_constrained`; no update of the avatar | nothing |
| a sit on the cube from there | answered as from standing | answered as from standing, a `stand` stated first | answered |
| `STAND_UP` from a seat † | the avatar is its own root again 0.2 s later, **0.34 m in front of where it sat and at the same height**, and drops to the ground from there; the same for both avatars, with a sit target and without | free 5 to 15 ms later, **0.65 m in front and 0.57 m above**, then drops; `sit` then `stand` | each flavour's placement, and no drop: nothing moves an avatar there |
| a teleport within the region, asked from a seat † | a `TeleportLocal` and the avatar free of the seat (in the sandbox, at the landing point every teleport there is sent to) | a `TeleportLocal` to the spot asked for, and the avatar free | — |
| a logout from a seat | [Session → Logout](session.md) | the same | — |

### What the client does with it

- **The session answers every `AvatarSitResponse` with an `AgentSit` at
  once**, as it always has. Neither grid waits for it, and neither says
  anything with its `AutoPilot` flag: the reference viewer walks towards
  the seat whenever the flag is set (`process_avatar_sit_response`) and is
  seated on the way.
- **The viewer draws a seated avatar from the avatar's own update**, never
  from the response, and reads the response for the scripted camera alone.
  The response is the wrong place to look on Second Life for a scriptless
  seat and on OpenSim for a scripted one.
- **A refusal ends the pending sit.** Second Life's are named alerts;
  OpenSim's two for a seat in the agent's own region — "There is no
  suitable surface to sit on, try another spot." and "Sit position on
  restricted land, try another spot" (`ScenePresence.PhysicsSit`,
  `PhysicsSitResponse`) — carry no name, so since this measurement the
  session knows them by their wording. Neither was provoked here.
- **A sit nothing answers** — OpenSim's unknown object, Second Life's seat
  too far away — ends on the session's own fifteen-second timeout as a
  `Diagnostic::ExpectedReplyMissing`. The viewer shows nothing for it, as
  the reference shows nothing.
- `e2e_sit` drives the viewer against each fake flavour: *Sit Here* seats
  the avatar, *Stand Up* leaves it where that grid puts a standing avatar,
  and a sit on an object the grid has dropped raises `SitFailNotSameRegion`
  on the Second Life flavour and nothing on the OpenSim one.

### Not measured

- **Where exactly Second Life stops answering**: between 7.5 and 9.3 m for
  this cube. Whether the seat's size or the height between the two moves
  that is not known.
- **The other refusals**: `SitFailCantMove`, `SitFailNotAllowedOnLand`,
  `CantSitNoRoom` and `CantSitNoSuitableSurface` on Second Life, and
  OpenSim's two sentences. Nothing here was refused except the unknown
  object.
- **OpenSim without a physics engine that seats avatars**: its fallback
  ignores a scriptless seat more than 10 m away
  (`ScenePresence.SendSitResponse`). The local grid's ubODE never reaches
  it.
- **A seat that sets a camera or forces mouselook**, a linkset with several
  sit targets, a phantom or physical seat, a seat that moves, and an
  unsit by script.
- **A sit while Second Life holds the avatar for an animation**: every sit
  here was answered whatever the avatar's animations were, a landing among
  them.
- **A region crossing on a seat**: `gridspec-seated-crossing`.

## What the client does with it

- **The viewer reports its own holding animations finished**, as the
  reference does (`LLAgent::onAnimStop`): when its copy of `standup`,
  `pre_jump`, `land` or `medium_land` runs out it sends the `FINISH_ANIM`
  bit, except for a pre-jump or a landing while the jump key is still held
  and a landing within a second of a jump input. The measurement is why
  each half of that matters — without the bit a Second Life avatar does not
  jump and does not get up from a fall — and it was written before this was
  measured; nothing in it changed.
- **A client that plays no animations has to send the bit itself**:
  `Command::FinishAnimation` (`Session::finish_animation`). The conformance
  cases that walk an avatar on Second Life are such clients.
- **The viewer states a control once**, when the keys change, and leaves the
  rest to the session's once-a-second `AgentUpdate`. Both grids keep the
  avatar moving on that.
- **Prediction has to carry the avatar through a silence.** Second Life
  says nothing for the whole of a steady climb and OpenSim nothing for two
  seconds of a level walk, both with a velocity that is exactly the motion
  and a zero acceleration. The viewer's dead-reckoner extrapolates from the
  last velocity for as long as the circuit is alive and tapers only when
  the circuit itself has gone quiet, as the reference's does.
- `e2e_movement` drives the viewer on a live grid
  (`SL_E2E_GRID=opensim|aditi`): the forward key walks the avatar from where
  the login put it, a tap of the jump key takes it off the ground, and the
  forward key walks it again afterwards. It reads the position the grid
  reports, so on Second Life it passes only if the viewer's `FINISH_ANIM`s
  went out. On OpenSim it needs a start clear of the test content
  (`SL_E2E_START='uri:Northeast Region&70&180&26'`): the default spot let
  the avatar walk 1.2 m.

## Not measured

- **Whether a hard landing on OpenSim holds the avatar during its
  `standup`.** The case takes the avatar to have landed when it has been at
  rest for half a second, which is most of the 1.1 s the animation lasts.
- **How long Second Life's holds last unanswered.** Eight seconds after a
  fall, eighteen in a pre-jump and twenty-four after an arrival all ended
  with the bit rather than by themselves.
- **What makes an arrival one that is held.** One of three was; the
  landing point is the difference seen, not a cause shown.
- **Second Life's animation assets beside the built-ins**, and why some
  states come only as those: the roadmap's `gridspec-animations`.
- **A turn on the spot on Second Life**: the three readings do not agree.
- **Movement with no `AgentUpdate` at all** after the control is stated:
  the session always sends one a second.
- **Swimming, mouselook steering, pushes and collisions**: no leg of the
  case.
- Seen once and OpenSim's alone: an avatar that came down from a fall on a
  region border was left there by its simulator — walk animation playing,
  no update, child circuits closed, and the next login into that region
  refused until the grid was restarted.
