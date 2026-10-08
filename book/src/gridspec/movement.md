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
- **Swimming, sitting on the ground, mouselook steering, pushes and
  collisions**: no leg of the case.
- Seen once and OpenSim's alone: an avatar that came down from a fall on a
  region border was left there by its simulator — walk animation playing,
  no update, child circuits closed, and the next login into that region
  refused until the grid was restarted.
