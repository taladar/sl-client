# LSL scripts

What a script's library calls do on each grid, measured by the LSL probe
corpus ([The LSL probe corpus](../tools/lsl-probes.md)): each probe is
planted, driven and read by our own client through the `lsl-probes`
conformance cases, on aditi (Second Life's Mono) and on the local OpenSim
under YEngine and XEngine. A probe's transcript is the committed result
file, `sl-lsl-runtime/probes/results/<grid>/<probe>.txt`; the tables here
are the reading of them. The runner's `#` notes in a transcript are what
the grid sent the viewers meanwhile — the permission question, the
`ScriptControlChange`, the follow camera, the animation list — beside what
the script printed.

The fake grid runs no scripts yet, so its column says what it does with the
wire half where it has one; the library itself is
`server-lsl-lib-avatar-control` and its siblings, built to these tables.

## Animations, sitting, controls and camera

Measured on 2026-10-24 by the probes `avatar/permissions`, `avatar/sit`,
`avatar/controls`, `avatar/camera` and `avatar/animation`, with two
avatars: the owner, which owns the probe's prim, and another, which owns
nothing of it and touches it so the script learns its key. Second Life is
aditi's Mauve (`sim_channel` `Luau`, `2026-07-24.30110090421`); OpenSim is
0.9.2.1, where **YEngine and XEngine agree on every row**, so they share a
column. The permission bits are LSL's: `TAKE_CONTROLS` 4,
`TRIGGER_ANIMATION` 16, `TRACK_CAMERA` 1024, `CONTROL_CAMERA` 2048,
`OVERRIDE_ANIMATIONS` 32768.

### Asking for permissions

| behaviour | Second Life | OpenSim | fake grid |
| --- | --- | --- | --- |
| `llTakeControls`, `llStartAnimation`, `llSetCameraParams` before any grant | a `DEBUG_CHANNEL` complaint each — `Script trying to take controls from owner but PERMISSION_TAKE_CONTROLS permission not set!`, `Script trying to trigger animations but PERMISSION_TRIGGER_ANIMATION permission not set`, `Script trying to set camera parameters but PERMISSION_CONTROL_CAMERA permission not set!` — and the script runs on | nothing at all | no scripts |
| `llGetCameraPos` before any grant | `ZERO_VECTOR`, no complaint | the same | no scripts |
| the question | a `ScriptQuestion` with the mask as asked, the object's name, its owner's legacy name (`First Last`) and no experience | the same | the same, as a test sends it (`e2e_script_permissions`) |
| the answer granting all that was asked | `run_time_permissions` with the mask; `llGetPermissions` the mask, `llGetPermissionsKey` the owner | the same | the viewer's card answers with the mask asked, on both flavours |
| the answer granting part of it (16 of 1044) | the part: 16 | the same | — |
| the answer denying (0) | `run_time_permissions(0)`; the permissions key stays the owner | the same | the card's *No* answers 0, on both flavours |
| the answer granting **more** than was asked (1044 for a question of 16) | **1044**: no grid masks the answer by the question | the same (`handleScriptAnswer` stores the answer as it came) | — |
| `llRequestPermissions(owner, 0)` | **nothing**: no question, no `run_time_permissions`, the permissions held unchanged | **releases**: `run_time_permissions(0)` at once, nothing held, the key `NULL_KEY` | no scripts |
| `CONTROL_CAMERA` asked by an object the avatar neither wears nor sits on | **refused unasked**: `Camera control currently only supported for attachments and objects on which you are sitting.` on `DEBUG_CHANNEL`, no question, no `run_time_permissions` — and the same for any mask that holds `CONTROL_CAMERA` | asked like any other; granted | no scripts |
| a question to the other avatar | the same question, put to that avatar's viewer; granted, the key is that avatar's | the same | no scripts |

### Sitting

The probe's prim carries `llSitTarget(<0, 0, 0.5>, a quarter turn)`,
`llSetCameraEyeOffset(<-2, 0, 1>)` and `llSetCameraAtOffset(<0, 0, 0.5>)`.
How each grid places a sitter is [Movement](movement.md#sitting); these are
the script's view of it.

| behaviour | Second Life | OpenSim | fake grid |
| --- | --- | --- | --- |
| the sit response's `SitPosition` for the target `<0, 0, 0.5>` | `<0, 0, 0.85>` — the target raised by 0.35 m | `<0, 0, 0.5>` — the target as set | `SitPolicy::response_states_seated_position`, per flavour |
| its rotation and camera offsets | the target's rotation; the eye and focus offsets as the script set them | the same | the camera offsets are zero (no scripts) |
| `changed` once seated | `CHANGED_LINK`; `llAvatarOnSitTarget` the sitter, `llGetNumberOfPrims` 2, the last link the sitter, `llGetObjectPrimCount` 1, `llGetAgentSize` of the sitter not zero | the same | no scripts |
| `llGetLinkKey(llGetNumberOfPrims())` on the lone prim, nobody seated | `NULL_KEY` | the prim's own key | no scripts |
| the avatar-control permissions asked of the seated avatar (`TAKE_CONTROLS`, `TRIGGER_ANIMATION`, `TRACK_CAMERA`, `CONTROL_CAMERA`) | granted at once, no question — to the owner and to another avatar alike | the same (`implicitPerms` for a sitting agent) | no scripts |
| what is held after `llUnSit` or standing up | 1040: `TAKE_CONTROLS` and `CONTROL_CAMERA` are dropped, `TRIGGER_ANIMATION` and `TRACK_CAMERA` kept, the key unchanged | the same | no scripts |
| a follow camera when the sitter stands | nothing sent | a `ClearFollowCamProperties` for the seat, whether a camera was set or not | — |
| a sit on the prim with its target cleared (`llSitTarget(ZERO_VECTOR, …)`) | seated (2 prims, the avatar the last link) but `llAvatarOnSitTarget` `NULL_KEY`; the response puts the seat at `<-0.34, 0, 0.55>`, turned half round | the same to the script; the response says `<0.415, 0, 0.9>`, unturned | — |
| the sitter's animation | the built-in `sit`, sourced from an object that is not the seat | `sit`, with no source | none |

### Controls

The script takes forward and turn-left, accepted and not passed on, and the
runner holds each down for a second (`Command::SetControls`).

| behaviour | Second Life | OpenSim | fake grid |
| --- | --- | --- | --- |
| the `ScriptControlChange` for `llTakeControls(CONTROL_FWD \| CONTROL_ROT_LEFT, TRUE, FALSE)` | a release of **every** control (`0xFFFFFFFF`; a later take releases what the last took), then a take of `0x80101` — the two keys and the nudge-forward bit `0x80000` — not passed to the agent | a release of the two keys, passed and not, then a take of `0x101` not passed | no scripts (`send_script_control_change` exists) |
| the `control` events of one press | one with `level` and `edge` the key, one with `level` 0 and `edge` the key on release | the same | no scripts |
| events between, while the key is held a second | 21–23 (a measurement: a frame's) | 1 | no scripts |
| a key the script did not take (back) | no event; the avatar walks | the same | — |
| `llTakeControls(CONTROL_ROT_LEFT, FALSE, TRUE)` (accept off) | a take of every control **but** turn-left (`0xFFFFFEFF`), passed to the agent; no event for turn-left | a take of turn-left (`0x100`), passed; no event | no scripts |
| `llReleaseControls` | a release of what was taken; `TAKE_CONTROLS` is dropped from the permissions | no `ScriptControlChange`; `TAKE_CONTROLS` dropped likewise | no scripts |
| `llTakeControls` after that | a `DEBUG_CHANNEL` complaint (two for the one call), nothing taken | nothing, silently | no scripts |

### Camera

| behaviour | Second Life | OpenSim | fake grid |
| --- | --- | --- | --- |
| `CONTROL_CAMERA` and `TRACK_CAMERA` asked of the owner standing | refused unasked (above); nothing held | asked; granted | no scripts |
| the same, seated on the prim | granted, no question | the same | no scripts |
| `llSetCameraParams` with a flag, floats and vectors | one `SetFollowCamProperties` naming the prim, the rules **in the script's order**, each vector rule split into its three codes (`CAMERA_FOCUS_OFFSET` as 2, 3, 4; `CAMERA_FOCUS` as 18, 19, 20) | the same parameters, **sorted by code** | no scripts (`send_set_follow_cam_properties` exists) |
| values out of their documented ranges (distance 100, pitch −200, behindness angle 400, focus lag 9, position threshold −1) | sent as given: **no clamping** | the same | — |
| `llClearCameraParams` | a `ClearFollowCamProperties` naming the prim | the same | — |
| `llGetCameraPos` with `TRACK_CAMERA` | not `ZERO_VECTOR` | the same | no scripts |
| what is held after the owner stands | 1024: only `TRACK_CAMERA` | the same | no scripts |

### Animations

| behaviour | Second Life | OpenSim | fake grid |
| --- | --- | --- | --- |
| `TRIGGER_ANIMATION` and `OVERRIDE_ANIMATIONS` asked of the owner by an unattached prim | asked (32784); granted | the same | no scripts |
| `llStartAnimation("dance1")` | an `AvatarAnimation` adding `dance1`, sourced from the prim; `llGetAnimationList` has it, `llGetAnimation` still `Standing` | the same | `Action::AnimateAvatar` plays a list |
| who is told the source | the animated avatar's own viewer; another viewer has `dance1` with no source | the same | — |
| `llStopAnimation("dance1")` | gone from both | the same | — |
| `llStartAnimation` of a name the prim does not hold and no built-in has | `Could not find animation 'no such animation'` on `DEBUG_CHANNEL` | nothing | no scripts |
| `llSetAnimationOverride("Standing", "dance1")` — a built-in's name, the prim holding no animation | accepted: the standing animation becomes `dance1`, `llGetAnimationOverride("Standing")` says `dance1` | the same (`DefaultAvatarAnimations` names it) | no scripts |
| `llGetAnimationOverride("Standing")` after `llResetAnimationOverride("ALL")` | `stand` | `Standing` | no scripts |
| the other avatar, after it granted `TRIGGER_ANIMATION` | danced and stopped like the owner | the same | no scripts |

### What the viewer does with it

The permission card (`viewer-permission-request-dialog`) shows a question as
both grids put it, and answers *Yes* with the mask asked and *No* with 0;
`e2e_script_permissions` holds it to that on both fake flavours. Neither
grid's differences reach it: where they differ, Second Life asks nothing.
Taking the movement keys (`viewer-input-script-control-capture`) and
applying a scripted follow camera
(`viewer-scripted-followcam-llsetcameraparams`) are viewer tasks of their
own, which carry the rows above that bear on them.

## Chat, listens, dialogs and link messages

Measured on 2026-10-24 by the probes `comms/listen-limit`, `comms/dialog`,
`comms/textbox`, `comms/instant-message`, `comms/owner-away`,
`comms/object-chat` and `comms/link-messages`, on the same grids, with the
owner and the other avatar standing by the probe's prims. What an
*avatar* is sent of each say function is [Chat](chat.md#a-scripts-lines);
these are what a *script* hears and what a dialog becomes. YEngine and
XEngine agree on every row (one instant message to `NULL_KEY` took 3 s
under XEngine), so OpenSim is one column again.

### Listens

| behaviour | Second Life | OpenSim | fake grid |
| --- | --- | --- | --- |
| two `llListen` with the same filter | one listen: the same handle | the same | no scripts |
| how many one script may hold | 65 | 64 | no scripts |
| the one too many | a run-time error, `Object [script:<name>] Script run-time error` then `Too Many Listens` on `DEBUG_CHANNEL`; the script is **stopped** (`llGetScriptState` 0, answers nothing) | **silently refused**: the handle is not positive, every later one too, and the script runs on | no scripts |
| `llListenRemove` of a handle the script never had | nothing | the same | no scripts |
| a listen filtered to a message (`"match"`) | hears that message alone | the same | no scripts |

### What one object hears of another

Listeners stand just inside and just outside each documented range: 9 m and
11 m, 19 m and 21 m east of the speaker, and 99 m and 101 m straight above
it (a range is a distance in three dimensions, and no parcel of the build
location is 100 m wide).

| behaviour | Second Life | OpenSim | fake grid |
| --- | --- | --- | --- |
| `llWhisper`, `llSay`, `llShout` | heard within 10 m, 20 m and 100 m: at 9, 19 and 99 m, not at 11, 21 and 101 m | the same | no scripts |
| `llRegionSay` on 5 | every listener, 101 m included | the same | no scripts |
| a script's own prim (another script in it, listening on the channel) | hears nothing the prim says, by any function | the same | no scripts |
| `llRegionSayTo` another prim, on 5 and on 0 | that prim alone, at any distance | the same | no scripts |
| `llRegionSayTo` its own prim, or `NULL_KEY` | nothing, and no complaint | the same | no scripts |
| `llRegionSay` on channel 0 | refused: `Cannot use llRegionSay() on channel 0.` on `DEBUG_CHANNEL` | refused: `llRegionSay: Cannot use on channel 0` | no scripts |
| a say of 1100 characters | heard cut to **1024** | heard cut to **1023** | no scripts |

### Dialogs

What the viewer is sent (the runner's `# dialog` note) is the `ScriptDialog`:
the object's name, its owner's first and last name, the message, the
channel and the buttons in the script's order. A reply is said on the
dialog's channel as the avatar; the listener hears it with that avatar's
key and its `llKey2Name` as the name.

| behaviour | Second Life | OpenSim | fake grid |
| --- | --- | --- | --- |
| three buttons, the second pressed | the buttons in order; the script hears `Two` from the owner | the same | `send_script_dialog` puts it as both grids do; the viewer's card replies with the index and label (`e2e_script_dialog`, both flavours) |
| an empty button list | one button, **`OK`** | one button, **`Ok`** | each flavour's, as the test sends it |
| a reply with a label the dialog does not have | heard as sent: the grid does not check a reply against the buttons | the same | — |
| twelve buttons | shown | the same | — |
| thirteen | refused, no dialog: `llDialog: button list too long, must be 12 or fewer entries` | refused, no dialog: `llDialog: No more than 12 buttons can be shown` | no scripts |
| a label of 24 characters | shown as it is | shown **cut to 23**; the reply is still whatever the viewer sends | no scripts |
| a label of 25 | refused, no dialog: `llDialog: button labels must be 24 or fewer characters long` | shown cut to 23, no complaint | no scripts |
| an empty label | refused, no dialog: `llDialog: all buttons must have label strings` | refused, no dialog: `llDialog: Button label cannot be blank` | no scripts |
| a button that is an integer (`["One", 2]`) | refused, no dialog: `llDialog: button list must contain only strings` | shown, the integer as `2` | no scripts |
| an empty message | refused, no dialog: `llDialog: must supply a message` | `llDialog: Empty message` **and** the dialog shown, empty | no scripts |
| a message of 511 characters | shown | the same | — |
| of 512 | refused, no dialog: `llDialog: message too long, must be less than 512 characters` | shown cut to 511, no complaint | no scripts |
| of 513 | refused likewise | `llDialog: Message longer than 512 bytes` and shown cut to 511 | no scripts |
| a dialog to `NULL_KEY` | nothing, no complaint | the same | no scripts |
| a dialog to the other avatar | put to that avatar's viewer, the owner's name on it; its reply heard from it | the same | — |

### Text boxes

An `llTextBox` is the same `ScriptDialog` with the one button
`!!llTextBox!!`; the reply's label is the text typed.

| behaviour | Second Life | OpenSim | fake grid |
| --- | --- | --- | --- |
| a few words typed | heard as typed | the same | the card's Submit replies with the text on index 0 (`e2e_script_dialog`, both flavours) |
| nothing typed | an empty line heard | the same | — |
| 300 characters typed | 254 heard: the reply's field holds 254 bytes, and our client cuts it there, as the reference's message system does (its text box takes 250 characters) | the same | — |
| an empty message | refused, nothing shown: `llTextBox: must supply a message` | refused, nothing shown: `llTextBox: Empty message` | no scripts |
| a message of 513 characters | refused: `llTextBox: message too long, must be less than 512 characters` | refused: `llTextBox: Message longer than 512 bytes` | no scripts |
| a text box to the other avatar | put to it; its text heard from it | the same | — |

### Instant messages and the owner elsewhere

| behaviour | Second Life | OpenSim | fake grid |
| --- | --- | --- | --- |
| what `llInstantMessage` costs the script | a 2 s sleep (2.00–2.09 s measured) | 2 s | no scripts |
| the message | an `ImprovedInstantMessage` of dialog 19 (`IM_FROM_TASK`), `FromAgentID` the **owner**, the message id the **prim's key**, the name the prim's, not offline | the same; its binary bucket 26 bytes against Second Life's 15 | no scripts |
| to the other avatar | delivered to it the same way | the same | no scripts |
| to `NULL_KEY` | `Invalid Key passed to instant message.` on `DEBUG_CHANNEL`, still 2 s | `llInstantMessage: An invalid key  was passed to llInstantMessage` | no scripts |
| to a key that is nobody's | nothing, no complaint, 2 s | the same | no scripts |
| 1100 characters | cut to 1023 | the same | no scripts |
| a line break | kept | the same | no scripts |
| `llOwnerSay`, `llRegionSayTo` the owner | no delay | the same | no scripts |
| while the owner is in another region (on OpenSim a neighbour, where it keeps a child agent; on Second Life a region away) | only the **instant message** reaches the owner; `llOwnerSay` and `llRegionSayTo` are lost | the instant message **and `llRegionSayTo`** reach it; `llOwnerSay` is lost | no scripts |
| what the script sees of the owner away | `llGetAgentSize` zero, `OBJECT_POS` an empty list | `llGetAgentSize` zero, but `OBJECT_POS` still answered (the child agent) | no scripts |

### Link messages

The probe's prim is linked to nothing (`llGetLinkNumber` 0, one prim) and
holds two scripts; `LINK_ROOT` is the link number 1.

| behaviour | Second Life | OpenSim | fake grid |
| --- | --- | --- | --- |
| `LINK_THIS`, `LINK_SET`, link 0 | both scripts, **the sender included**, with sender 0 | the same | no scripts |
| `LINK_ROOT` (1) | nobody: an unlinked prim is not link 1 | **both scripts**: OpenSim takes link 1 for the lone prim | no scripts |
| `LINK_ALL_OTHERS`, `LINK_ALL_CHILDREN`, link 2 | nobody | the same | no scripts |
| a 3000-character string and a key that is no key (`"not a key"`) | both arrive whole | the same | no scripts |

### What the viewer does with it

The dialog card shows a dialog as both grids put it, the empty list's one
button included under either spelling, and replies with the pressed
button's index and label; a text box replies with the text on index 0.
`e2e_script_dialog` holds it to that on both fake flavours, and since
2026-10-24 a reply longer than its field is cut to it in `sl-proto`
rather than failing to encode. The refusals reach the viewer only as
`DEBUG_CHANNEL` chat, which the roadmap task
`viewer-script-warning-window` is to show.

## Sensors, raycasts and touches

Measured on 2026-10-25 by the probes `detection/sensor`,
`detection/cast-ray` and `detection/touch-surface`, on the same grids, with
both avatars standing by the probe's prims. The probe's prim A faces east,
unrotated; its targets are 0.5 m cubes at known distances east of it and
above it, some scripted (each names its prim `Target <letter>`), some bare
(no script, the default name). OpenSim runs its default settings —
`SensorMaxRange` 96, `SensorMaxResults` 16, `UseLlCastRayV3` false, so
`llCastRay` is the older implementation — and **YEngine and XEngine agree on
every row** (500 rays took YEngine 33 s and XEngine 21 s), so OpenSim is one
column again. Both regions held other residents' objects, so a sweep's
answer has strangers in it; the probe prints whether the owner owns each.
Second Life was run three times: rows that changed between runs say so.

### Sensors: what a sweep returns

| behaviour | Second Life | OpenSim | fake grid |
| --- | --- | --- | --- |
| the most one sweep returns | **32** | 16 (`SensorMaxResults`) | no scripts |
| the order | nearest first, by the distance between the prims' centres, strangers and the probe's own mixed | the same | no scripts |
| the order the targets were rezzed in (far end first) | irrelevant: distance alone | the same | no scripts |
| the scanning prim itself | never returned | the same | no scripts |
| a target exactly at the range (3 m, range 3.0) | **left out**: in range means nearer than it | **returned**: within it means up to it | no scripts |
| a target whose centre is 3.5 m away and whose near face is 3.25 m (range 3.4) | left out: the centre counts, not the bounds | the same | no scripts |
| a target 100 m straight up, asked for with a range of 200 | not found: the range is held to 96 | the same (`SensorMaxRange`) | no scripts |
| a range of 0, or −1 | **no event at all**: neither `sensor` nor `no_sensor` within five seconds | `no_sensor` | no scripts |
| an arc of `PI / 4` | what lies within 45° of the prim's forward (east) axis: the target 3.5 m straight up is left out | the same | no scripts |
| an arc of 0 | **no arc**: everything in range, the target straight up included | only what lies exactly on the axis | no scripts |
| an arc of −1 | everything in range, as for 0 | **nothing**: `no_sensor` | no scripts |
| an arc of 4.0 (more than `PI`) | everything in range, as `PI` | the same | no scripts |
| a name filter (`"Target B"`), a key filter, both | the prims that match; a key with another name matches nothing (`no_sensor`) | the same | no scripts |
| nothing matches | `no_sensor` | the same | no scripts |
| `llDetectedType` of an idle scripted prim, of a bare prim | 12 (`PASSIVE \| SCRIPTED`), 4 (`PASSIVE`) | the same | no scripts |
| `llDetectedPos`, `llDetectedRot`, `llDetectedVel`, `llDetectedOwner`, `llDetectedGroup` of a prim | its centre, rotation and velocity; its owner; 1 (the same group as the scanner, neither having one) | the same | no scripts |
| `llDetectedLinkNumber`, `llDetectedGrab`, the `llDetectedTouch*` in `sensor` | 0, `ZERO_VECTOR`, the `TOUCH_INVALID_*` values | the same | no scripts |
| `llDetectedKey` one past the last result | `NULL_KEY` | not `NULL_KEY` (an empty key) | no scripts |

### Sensors: the type mask

| behaviour | Second Life | OpenSim | fake grid |
| --- | --- | --- | --- |
| `ACTIVE \| PASSIVE \| SCRIPTED` | **scripted objects only**: `SCRIPTED` narrows the mask, so not one bare prim of eighteen is returned, though a ray hits them | **the union**: bare and scripted prims alike | no scripts |
| `PASSIVE` alone | the bare prims and other unscripted objects — and some idle scripted ones (below) | the bare prims, never a scripted one | no scripts |
| `SCRIPTED` alone, `ACTIVE` alone | scripted objects — but **not every idle scripted prim**: of the five targets, whose scripts set the name and do nothing more, one or two were missing each run, and a different one each time (Target B in two runs, Target W in one); the one `SCRIPTED` missed, `PASSIVE` found. `llDetectedType` says 12 for all of them in every sweep | every scripted prim, either way: OpenSim counts a scripted prim as active | no scripts |
| a repeating sweep for Target B with `SCRIPTED`, later in the run | `no_sensor` every time, in two runs of two: by then Target B was not among the scripted | found every time | no scripts |
| `AGENT` | every avatar in range, `llDetectedType` 3 (`AGENT \| ACTIVE`), 35 with a scripted attachment worn; `llDetectedOwner` the avatar itself; `llDetectedName` its `llKey2Name` | the two avatars, type 1 (`AGENT`) | no scripts |
| `AGENT_BY_LEGACY_NAME` with the owner's legacy name | the owner alone | the same | no scripts |
| `AGENT_BY_USERNAME` with `llGetUsername(owner)` | the owner alone | **nobody**: `no_sensor` | no scripts |

### Repeating sensors

| behaviour | Second Life | OpenSim | fake grid |
| --- | --- | --- | --- |
| `llSensorRepeat` every 0.5 s, for 2.6 s | 5 sweeps, the first after 0.51 s, then every 0.51 s | 4, the first after **0.6 s**, then every 0.5 s | no scripts |
| one that finds nothing | `no_sensor` every interval | the same | no scripts |
| after `llSensorRemove` | nothing | one sweep already queued arrives at once, then nothing | no scripts |
| every 0.01 s | 17–22 sweeps a second (a measurement) | 9.5 a second: about one every 0.1 s | no scripts |
| every 0.0 s | **one** sweep, at once, then none | **none**: an interval of 0 is no repeat | no scripts |
| `llSensor` while a 1 s repeat runs | the `llSensor`'s answer, then **nothing more**: the one sweep replaced the repeat | the `llSensor`'s answer, and the repeat runs on every second | no scripts |

### Raycasts

`llCastRay` along the row from just outside prim A's east face: a scripted
`Wall` at 5 m, a phantom `Ghost` at 7.5 m and a bare prim at 10 m, so the
near faces are 4.75 m, 7.25 m and 9.75 m away. Each answer is a list of
hits, nearest the start first, and a status last.

| behaviour | Second Life | OpenSim | fake grid |
| --- | --- | --- | --- |
| no options | `[wall's key, <4.75, 0, 0> (as a vector), 1]` — one hit, the face it entered | the same hit, but the position is a **string** (`"<124.75, 128, 27.5>"`, region coordinates) | no scripts |
| `RC_MAX_HITS 4` | the wall and the bare prim; status 2 | the wall, the bare prim and **two hits on land 51.7 m out** — past the ray's 15 m end, at the ray's height, the same point twice; status 4 | no scripts |
| `RC_DETECT_PHANTOM TRUE` | the ghost as well, between them | the same, and land | no scripts |
| `RC_DATA_FLAGS` with `RC_GET_NORMAL \| RC_GET_ROOT_KEY \| RC_GET_LINK_NUM` | each hit `[key, link, position, normal]`: the link 0 (an unlinked prim), the key the prim's own, the normal `<-1, 0, 0>` | each hit `[key, position (a string), link, normal]` — the link **after** the position; land's normal the terrain's slope | no scripts |
| `RC_REJECT_TYPES RC_REJECT_NONPHYSICAL` | nothing: status 0 | the three land hits alone | no scripts |
| `RC_REJECT_TYPES RC_REJECT_PHYSICAL` | the wall and the bare prim | the same, and land | no scripts |
| a ray from prim A's centre | **not** prim A: the wall and the bare prim | prim A first, at its east face, then the rest | no scripts |
| `RC_REJECT_TYPES 0x20000000` (`RC_REJECT_HOST`) | refused: `Invalid filter settings for llCastRay.` on `DEBUG_CHANNEL`, and an **empty list** | accepted; prim A left out | no scripts |
| a ray from inside the wall | not the wall: the bare prim alone | the wall (where it leaves it), then the rest | no scripts |
| a ray from the far end back | the bare prim's and the wall's far faces, nearest the start first | the same, and prim A | no scripts |
| a ray straight down to height 0 | land, where the ground is; `RC_REJECT_LAND` leaves nothing | the same | no scripts |
| a ray from prim A's top down to the owner | prim A (entered from outside), the owner, land; `RC_REJECT_AGENTS` drops the owner | the owner, the other avatar standing by it, land twice | no scripts |
| a ray of length 0 | `[0]` | the same | no scripts |
| a ray whose end lies outside the region | cast as far as the region: the wall, the bare prim, land twice further out | the same, with OpenSim's land hits | no scripts |
| `RC_MAX_HITS 0` | `You must request at least one result from llCastRay.`, an empty list | treated as 1 | no scripts |
| `RC_MAX_HITS -1`, `RC_MAX_HITS 300` | `Error. No more than 256 hits can be return by llCastRay.`, an empty list | −1: `[0]`; 300: every hit | no scripts |
| an option that does not exist (`99, 1`) | `Invalid filter settings.`, an empty list | ignored: the plain answer | no scripts |
| `RC_MAX_HITS` with no value after it | `Syntax error in llCastRay call. Incorrect number of elements in param list.`, an empty list | an `IndexOutOfRangeException` in the engine that **stops the script** (YEngine: `Script must be Reset to re-enable.`) | no scripts |
| 500 rays in a row | all answered, about 640 a second; no `RCERR_*` | all answered, 15–24 a second; no `RCERR_*` | no scripts |

### Touches: where the touch landed

A viewer that picks a point sends it with the grab and with the release
(`ObjectGrab`'s and `ObjectDeGrab`'s `SurfaceInfo`), and the runner sent
chosen values (`drive=touch face=… uv=… st=… at=… normal=… binormal=…`).

| behaviour | Second Life | OpenSim | fake grid |
| --- | --- | --- | --- |
| a touch with no surface block | `TOUCH_INVALID_FACE` (−1), `TOUCH_INVALID_TEXCOORD` (`<-1, -1, 0>`) for UV and ST, `ZERO_VECTOR` for the point, normal and binormal, in `touch_start` and `touch_end` | the same | no scripts |
| a touch a viewer could have picked (face 1, UV, ST, a point on the prim, a unit normal) | every value **as the viewer sent it**, in both events | the same | no scripts |
| one no viewer could pick: face 9 of a six-faced cube, UV and ST outside 0–1, a point 8.7 m off the prim, a normal of length 2, a zero binormal | **as sent**: the grid checks nothing | the same | no scripts |
| face −1 with the rest given | face −1 and the rest as sent | the same | no scripts |
| the other avatar's touch | its values as sent | the same | no scripts |
| a `touch` event for a grab and a release with no drag update between | **one** per touch | **none** | no scripts |
| `llDetectedGrab`, `llDetectedLinkNumber` | `ZERO_VECTOR`, 0 | the same | no scripts |

### What the viewer does with it

Nothing of a sweep or a ray reaches the viewer; the refusals are
`DEBUG_CHANNEL` chat, for `viewer-script-warning-window`. A touch's
surface block is the viewer's to fill: since neither grid checks it, what
a script reads is whatever the viewer picked, so the picking is the whole
of it: our viewer fills `Command::GrabObject`'s `surface` from its own
pick, in region metres (`world_test`'s
`a_world_touch_states_its_position_in_the_region`).

## Compiling

Measured on 2026-10-24 and 25 by the conformance case `script-compile`
(`sl-conformance run --grid aditi|opensim script-compile`): it uploads a
valid script to a prim, then sixteen broken sources over it, each with its
fault at a known place, sources of growing size, and a broken and a valid
one to a script in the agent's inventory; it records every answer verbatim
and reads the stored source back over the UDP transfer. Second Life is
aditi's Mauve under the `mono`, `lsl2` and `luau` targets; OpenSim is
0.9.2.1 under YEngine and XEngine, picked by the source's first line
(`//YEngine:`, `//XEngine:`), so OpenSim's positions count that line too: a
fault on line 5 of the script is on line 6 of what was uploaded. The census
holds both grids to every answer below.

The fake grid compiles nothing yet: it answers every script upload
`compiled` with no errors. Compiling on the way through, with these answers,
is `server-fake-grid-script-compile-on-upload`.

### The answer

There is no dry run on any grid: the upload *is* the compile. The answer to
the uploader's POST carries `compiled` and an `errors` array of strings.

| behaviour | Second Life | OpenSim YEngine | OpenSim XEngine |
| --- | --- | --- | --- |
| a position | `(line, column)` **from zero**, a space after the comma: `(4, 20) : ERROR : Syntax error` for line 5, column 21 | from zero, no space: `(5,20) Error: invalid operand token` | from zero, no space: `(5,20) syntax error`; a C# compiler error `(5,8): Error CS0103: …`; an end of file at column **−1**: `(9,-1) Unexpected EOF` |
| where it points | the token **after** the fault, where the parser stood: `x = ;` at the `;`, `y = 1` at the `=`, `llNope()` at the `)` | the fault | the fault |
| what a column counts | **bytes**: three two-byte characters before the fault move it by six | characters | characters |
| a tab | two tabs before the fault counted four columns | one each | one each |
| CR LF line ends | read as LF | the same | the same |
| how many errors | **one**, the first found | every one; a syntax error drags follow-on errors (three for one missing expression) | one entry; for C# errors, every error of the pass on its own line of that one string |
| three faults (a type mismatch, then an undeclared name, then an unknown function) | only the undeclared name, the second fault: names are resolved before types | all three | all three, in one string |
| a missing expression | `Syntax error` | `invalid operand token` | `syntax error` |
| a type mismatch | `Type mismatch` | `must explicitly cast from string to integer` | `` Cannot implicitly convert type `string' to `integer'. An explicit conversion exists (are you missing a cast?) `` (CS0266) |
| an undeclared name, an unknown function, label or state | `Name not defined within scope` | `undefined constant/function/variable y`, `… llNope()`, `undefined label nowhere`, `undefined state other` | `` The name `y' does not exist in the current context `` (CS0103), the same for `llNope`; `` The label `nowhere:' could not be found within the scope of the goto statement `` (CS0159); an unknown **state compiles** |
| a global declared twice | `Name previously declared within scope` | `duplicate variable x` | `` The type `SecondLife.XEngineScript' already contains a definition for `x' `` (CS0102) |
| a path without a return | `Not all code paths return a value`, at the function's closing brace | **compiles** | `` `SecondLife.XEngineScript.f()': not all code paths return a value `` (CS0161), at the function's first line |
| no `default` state | `(0, 0) : ERROR : Syntax error` | `no default state defined` | **compiles** |
| an empty source | `(0, 0) : ERROR : Syntax error` | `fetched source empty asset://<id>`, no position | `Unexpected EOF` |
| a string that never ends | `Name not defined within scope`, inside the string | `invalid operator` at the end of the source | `Unexpected EOF` at the end |

### What a failed compile does

| behaviour | Second Life | OpenSim YEngine | OpenSim XEngine |
| --- | --- | --- | --- |
| the stored source | **replaced** by the broken one (read back) | replaced | replaced |
| the script that ran before | **stopped**: `GetScriptRunning` answers not running, it answers no chat | gone: `GetScriptRunning` is not answered, no chat answered | gone, the same |
| the answer to the upload | `compiled` false and the error | the same | **none**: XEngine answers no upload to a script that holds a running instance (below) |

So a viewer's Save always stores the text; a failed compile leaves a
stopped script with the broken source, which the next open shows.

### `new_asset` and the item

| behaviour | Second Life | OpenSim |
| --- | --- | --- |
| `new_asset` of a task upload | an id, fresh each time | **the item's own id** (`UpdateItemAsset.cs` writes `m_inventoryItemID`), not the asset's |
| `new_asset` of an agent upload | an id | the asset the item is listed with afterwards |
| a script's asset id in a task listing and in an inventory fetch, to its owner with full permissions | **hidden** (nil) | listed |
| the item id after any upload | unchanged | unchanged |
| `is_script_running` | echoed | echoed |

### Running, re-uploading, sizes, targets

| behaviour | Second Life | OpenSim YEngine | OpenSim XEngine |
| --- | --- | --- | --- |
| a valid upload with `is_script_running` false | compiled, stays stopped, `state_entry` does not run | the same | the same |
| with true | starts: `state_entry` runs | the same | the same |
| the same source again over the running script | answered; the script restarts (`state_entry` again) | the same | stored and restarted, but the upload is **never answered**, and each such upload keeps one of OpenSim's HTTP handlers — after a few, every later upload is refused (`error sending request`) |
| sources of 65 000, 66 000, 262 200 bytes and 1 MiB (a comment) | all compile; no limit up to 1 MiB (the reference editor stops at 262 144 characters) | the same | the same |
| 600 functions returning 100-character strings | Mono compiles, then `Stack-Heap Collision` at start (one run of two sent both `DEBUG_CHANNEL` lines empty); **LSL2 refuses** it with 1203 errors of no position (`Function Args: (null)`, `Local List: (null)`) | compiles | compiles |
| an agent-inventory upload (`UpdateScriptAgent`) | compiled like a task's: the same errors | **not compiled**: `compiled` true for a broken source | the same as YEngine |
| the `lsl2` target | the LSL2 (LSO) compiler, the same messages | ignored | ignored |
| the `luau` target | the Luau compiler: `:2: Expected identifier when parsing expression, got '='`, a **one-based** line, no column; the item's subtype becomes Luau; LSL source under it compiles, since `default { state_entry() … }` happens to parse as Lua, and fails as it starts: `lua_script:3: attempt to call a nil value` | ignored: Luau source is read as LSL and refused, the subtype stays LSL | — |

### What the viewer does with it

Since 2026-10-25 `ScriptCompileError::parse` moves every `(line, column)`
to one-based (it took Second Life's zero-based numbers as they came, and
the editor named the line above the fault), reads Luau's `:line:` form
and XEngine's `-1`, and keeps what the column counts — bytes for Second
Life's spaced form, characters for OpenSim's — so the editor's error list
and the rendered caret name the character the grid meant
(`ScriptCompileError::character_column`). The census records where our
parse puts each first error, so the parse is held to both grids. A Luau
error is listed with its line alone. A save XEngine leaves unanswered
fails after a minute as a save failure, in both runtimes (the reference
gives up after 30 s), though the grid stored the source; reopening the
script shows it.

## Events

Measured by the corpus's first event probes on 2026-10-23
(`events/queue-limit`, `events/timer-no-stack`, `events/state-change`,
`events/state-self`, `events/detected-cleared`) and on 2026-10-25 by
`events/order`, `events/merging`, `events/collision`, `events/attach` and
`events/state-drops`, on the same grids. **YEngine and XEngine agree on
every row but two** — how much a queue holds, and what a take and a rez do
to the script — which say so in the cell. A probe that wants to see what
waited in a queue sleeps while its stimuli are raised and prints the
handlers that then run, in order; a round ends at a fence, a link message
to itself sent after the sleep, whose handler waits a second more and sends
a second one, so that what was still on its way lands in front of it.

### The queue, the timer and a state change

| behaviour | Second Life | OpenSim | fake grid |
| --- | --- | --- | --- |
| how many events a queue holds (100 link messages to itself) | 64; the rest are dropped | YEngine 64; **XEngine all 100** | no scripts |
| a timer that fell due while the queue was full | dropped with the rest | YEngine the same | no scripts |
| a 0.1 s timer over a 2 s sleep | runs once on waking: a timer never stacks | the same | no scripts |
| a state change: the events still queued | dropped | the same | no scripts |
| — the timer | kept, at its interval | the same | no scripts |
| — the listens | dropped | the same | no scripts |
| — a repeating sensor | dropped: not one sweep in the new state | the same | no scripts |
| — controls taken with `TAKE_CONTROLS` | kept: the new state's `control` runs on a press, and the grid tells the viewer nothing at the change | the same | no scripts |
| — the permissions and their key | kept | the same | no scripts |
| — a listen opened in `state_exit` | dropped with the rest | the same | no scripts |
| — an `llTarget` and an `llRotTarget` set in the handler that changes state | **kept**: `at_target` and `at_rot_target` run in the new state | **dropped**: neither runs | no scripts |
| a sensor and targets set afresh in the new state | they work | the same | no scripts |
| `state default` in `default` | runs neither `state_exit` nor `state_entry` | the same | no scripts |
| `state` in a function | the caller carries on; the change comes when the handler ends | the handler ends at the `state` | no scripts |
| `llDetectedKey`, `llDetectedName`, `llDetectedTouchFace` outside the event | `NULL_KEY`, `NULL_KEY`'s text, 0 | an empty key and name, −1 | no scripts |

### The order of events of different types

`events/order` raises its stimuli while the probe sleeps: a helper script in
the same prim sends link messages and changes the prim's colour, a talker in
another prim says a line on the probe's channel, the probe's own timer and
sensor answer, and the runner touches the prim.

| what was raised while the script slept, in that order | Second Life | OpenSim | fake grid |
| --- | --- | --- | --- |
| a link message, then a colour change, in one handler | link, `changed` | the same | no scripts |
| a colour change, then a link message | **link, `changed`**: the `changed` comes after | `changed`, link: in the order raised | no scripts |
| link, colour, link, colour, 0.3 s apart | link, link, **one** `changed` | link, `changed`, link, `changed` | no scripts |
| link and colour, the sensor's answer, the talker's line, a 0.3 s timer, a touch at about a second | the line, link, timer, sensor, `touch_start`, `touch_end`, `changed` | sensor, link, `changed`, the line, timer, `touch_start`, `touch_end` | no scripts |
| a touch at about a second, link and colour at 2.5 s, the line at 3 s, a timer at 3.5 s | link, the line, timer, `touch_start`, `touch_end`, `changed` | `touch_start`, `touch_end`, link, `changed`, the line, timer | no scripts |

OpenSim's queue is first in, first out across every type, on both engines.
Second Life's is not: a touch and a `changed` raised while the script slept
come after the link messages, chat, timers and sensor answers raised in the
same sleep, whenever they were raised, and `changed` comes last of all.
Among the others the order is the order they reached the script, which is
frame timing (the sensor's answer came after a timer set to fire at 0.3 s,
and the helper's link message after the talker's line).

### What merges

`events/merging`, the same way: each round raises one kind of event several
times while the probe sleeps three seconds.

| raised while the script slept | Second Life | OpenSim | fake grid |
| --- | --- | --- | --- |
| five colour changes in one handler | **one** `changed` (2) | five | no scripts |
| three changes of the colour to the one it has | **one** `changed` (2): setting a colour raises it, changed or not | **none** | no scripts |
| a colour, a scale and a texture change (plywood to blank) in one handler | **one** `changed`, 10: colour and scale, no texture bit (`llSetTexture` raised none on aditi in 2026-09 either) | three: 2, 8, 16 | no scripts |
| three colour changes half a second apart | **one** `changed`: queued behind the probe's own cue, so each later one joined it — the `Coalescing::Reference` rule | three | no scripts |
| five identical link messages | five | the same | no scripts |
| one line said five times, then five different lines | five each, in order | the same | no scripts |
| `llSensor` three times in one handler | **one** `sensor` | three | no scripts |
| three touches a second apart | **one** `touch_start` and one `touch_end`, each of one avatar, and one `touch` | three of each, no `touch` | no scripts |

### Collisions

`events/collision`: prim B, two metres above the probe's prim, turns
physical, falls onto it and comes to rest; five seconds later it is put back.

| behaviour | Second Life | OpenSim | fake grid |
| --- | --- | --- | --- |
| a physical prim falling onto the scripted one | `collision_start` and `collision_end` once each, with 11 `collision` events between (a measurement) | each **twice** — it bounced — with 66–76 `collision` events | no scripts |
| `llDetectedType` of the falling prim | 10 (`ACTIVE \| SCRIPTED`) | the same | no scripts |
| the scripted prim under `llVolumeDetect(TRUE)` | `collision_start` and `collision_end` as the prim falls through, no `collision` | the same | no scripts |
| the falling prim's own events, landing on the solid prim | `collision_start`, `collision_end` and `collision` of its own | the same | no scripts |
| the scripted prim asleep three seconds while it is hit | **nothing at all**: no `collision_start`, `collision` or `collision_end` on waking, though the falling prim's own events show it hit | all of it queued: each start and end twice, 82–132 `collision` events | no scripts |

Where the falling prim lands after a volume-detect pass is the region's: on
aditi it fell on to land (`land_collision_start`), on the local OpenSim on
to the region's reference prim beneath (another `collision_start`).

### Attached, dropped, taken and rezzed again

`events/attach`: the runner attaches the probe's prim to the owner's left
hand from the ground (`ObjectAttach`) and drops it (`ObjectDrop`); the script
attaches itself with `llAttachToAvatar` once granted `PERMISSION_ATTACH`;
then the runner takes the prim into the owner's inventory and rezzes the
item back where it stood. The script's one-second timer and its listen on 7
run all the while.

| behaviour | Second Life | OpenSim | fake grid |
| --- | --- | --- | --- |
| attached from the ground | `attach` with the owner's key, alone; `llGetAttached` 5 | two `changed` of **32768** first — OpenSim's own `POSITION` bit, which LSL has no constant for — then `attach` | no scripts |
| dropped | `attach(NULL_KEY)`, alone | one `changed` of 32768, then `attach(NULL_KEY)` | no scripts |
| `llAttachToAvatar` | `run_time_permissions(32)`, then `attach` | `run_time_permissions(32)`, two `changed` of 32768, then `attach` | no scripts |
| the prim's key, attached and dropped | kept | the same | no scripts |
| worn | the timer runs and the listen hears | the same | no scripts |
| taken and rezzed again | a new key; `on_rez(0)` alone; the timer and the listen carry on — the script's state is kept | YEngine the same; **XEngine resets the script**: `state_entry` runs, the globals are gone, and `on_rez(0)` comes after it | no scripts |

The restart of a region (`CHANGED_REGION_START`, and what runs first) is
measured with the script persistence it belongs to, in
`gridspec-lsl-persistence`: aditi grants no restart without estate rights,
and the runner does not survive one.

### What the viewer does with it

Nothing of a script's queue reaches the viewer: a state change sends it
nothing, not even about the controls it keeps (what `llTakeControls`
sends is in [Controls](#controls)). The viewer's half is the wire the
stimuli travel, the same on both grids — the touch, the attach from the
ground and the drop, the take and the rez — which the runner sends with our
own client.
