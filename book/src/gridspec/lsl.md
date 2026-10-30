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
`DEBUG_CHANNEL` chat, which its Script Errors window shows
([Run-time errors](#run-time-errors)).

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
`DEBUG_CHANNEL` chat, for the Script Errors window
([Run-time errors](#run-time-errors)). A touch's
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

What else a take and a rez keep — globals, permissions, the queue, a sleep —
and what a restart of the region does are in [Persistence](#persistence).

### What the viewer does with it

Nothing of a script's queue reaches the viewer: a state change sends it
nothing, not even about the controls it keeps (what `llTakeControls`
sends is in [Controls](#controls)). The viewer's half is the wire the
stimuli travel, the same on both grids — the touch, the attach from the
ground and the drop, the take and the rez — which the runner sends with our
own client.

## Experiences and the key-value store

Measured on 2026-10-25 by the probes `experience/no-experience` and
`experience/powers` (and on 2026-10-27 by `experience/sit-on-link`), with
the same two avatars, on the same grids. Every
script here was saved into **no experience**: none of our avatars owns,
administers or contributes to one, and saving a script into an experience
needs one of the three (creating an experience needs a Premium account —
[Experiences](experiences.md)). So what follows is what each grid answers a
script outside every experience, and what the powers an experience widens
do without one. Granted experience permissions, a key-value store that
holds anything, what `llSitOnLink` does in an experience and the
experience-only reach of `llTeleportAgent` are unmeasured.

### The experience functions

| behaviour | Second Life | OpenSim | fake grid |
| --- | --- | --- | --- |
| a script that calls any experience function or has an `experience_permissions*` handler | compiles | **does not compile**: YEngine refuses each of the eleven calls, `undefined constant/function/variable llAgentInExperience(key)` and so on (`undefined name` for `llGetExperienceErrorMessage`), and the handler, `unknown event handler experience_permissions_denied(key,integer)` — it names no error for `experience_permissions`; XEngine's parser stops at the first handler, `syntax error`, before it would reach a call | no scripts |
| `llAgentInExperience`, the owner or the other avatar | 0 | — | no scripts |
| `llGetExperienceDetails(NULL_KEY)` (the script's own experience) | an empty list, still after five seconds | — | no scripts |
| `llGetExperienceDetails` of a public experience (Bugsly Testing) | six fields at once: the name, the owner's key, the experience's key, the state `"0"`, its message `"no error"`, the group's key — in that order, which is not the library table's (it puts the group third) | — | no scripts |
| `llGetExperienceDetails` of an id no experience has | an empty list at first; a second later six fields: an empty name, `NULL_KEY` owner and group, the id asked, state `"7"`, `"invalid experience"` | — | no scripts |
| `llGetExperienceErrorMessage(0 … 18)` | `no error`, `exceeded throttle`, `experiences are disabled`, `invalid parameters`, `operation not permitted`, `script not associated with an experience`, `not found`, `invalid experience`, `experience is disabled`, `experience is suspended`, `unknown error`, `experience data quota exceeded`, `key-value store is disabled`, `key-value store communication failed`, `key doesn't exist`, `retry update`, `experience content rating too high`, `not allowed to run in current location`, `experience permissions request timed out` | — | no scripts |
| `llGetExperienceErrorMessage` of −1, 19, 20 | `unknown error id` (not code 10's `unknown error`) | — | no scripts |
| `llRequestExperiencePermissions`, the owner or the other avatar | `experience_permissions_denied` at once, reason **5**, `XP_ERROR_NO_EXPERIENCE` (`script not associated with an experience`); no question to either viewer, no `DEBUG_CHANNEL` complaint, nothing held | — | no scripts |
| `llSitOnLink`, the owner on every link number a single prim answers to (0, `LINK_THIS`, 1, `LINK_ROOT`, `LINK_SET`, `LINK_ALL_OTHERS`, `LINK_ALL_CHILDREN`) and on link 2, which is no prim; with no sit target and with one | **−1**, `SIT_NOT_EXPERIENCE`, every time; nobody sits, nothing reaches either viewer, no `DEBUG_CHANNEL` complaint | does not compile: YEngine `undefined constant/function/variable llSitOnLink(key,integer)`, XEngine ``CS0103: The name `llSitOnLink' does not exist in the current context`` | no scripts |
| `llSitOnLink` of `NULL_KEY`, a key no avatar has, the prim's own key, or the other avatar | −1 as well: the missing experience is checked **before** the agent and the link, so `SIT_INVALID_AGENT` and `SIT_INVALID_LINK` are never reached | — | no scripts |
| the seven key-value calls (`llCreateKeyValue`, `llReadKeyValue`, `llUpdateKeyValue`, `llDeleteKeyValue`, `llKeyCountKeyValue`, `llKeysKeyValue`, `llDataSizeKeyValue`) | each returns a request key, and the `dataserver` answers each `"0,5"` within a second, **not in the order called** | — | no scripts |

### Teleporting an agent and a temporary attachment

`experience/powers` aims each teleport three metres east of where the
agent stands, in the region, and reads where it is five seconds later; the
runner notes the teleport messages each viewer is sent. Bit 16 is
`TeleportFlags::VIA_LOCATION`, `0x20000` `WITHIN_REGION`, `0x10000`
`VIA_REGION_ID`.

| behaviour | Second Life | OpenSim | fake grid |
| --- | --- | --- | --- |
| `llTeleportAgent(owner)` with no permission | `Script trying to teleport agent without current permission!` on `DEBUG_CHANNEL`; nothing moves | nothing at all | no scripts |
| asking the owner for `PERMISSION_TELEPORT` (4096) | an ordinary question, no experience; granted | the same | no scripts |
| `llTeleportAgent(owner, "", …)` granted | a `TeleportStart` and a `TeleportLocal`, flags `0x20010`, and an alert, `You have been teleported by the object 'Object' on the parcel '…' owned by <the object's owner>.`; the avatar lands **where the region routes it** — by Mauve's telehub, 66–80 m from the aim in three runs — not at the aim | `TeleportStart` and `TeleportLocal`, flags `0x10`, no alert; the avatar lands at the aim | no scripts |
| — teleported back where it stood | the same routing, no alert; still as far off | lands where it stood | no scripts |
| `llTeleportAgent(other)` with the owner's grant | nothing moves, no complaint | the same | no scripts |
| asking the other avatar for `PERMISSION_TELEPORT` | the question is put to it; granted, the key is that avatar's | the same | no scripts |
| `llTeleportAgent(other)` granted by that avatar | **nothing moves**, no complaint, nothing sent to either viewer: outside an experience only the owner is teleported | **teleported**, as the owner is | no scripts |
| `llTeleportAgentGlobalCoords(owner, llGetRegionCorner(), …)` granted (with `PERMISSION_ATTACH`) | flags `0x30000`, the alert; lands 18.5 m from the aim | flags `0x10`, lands at the aim | no scripts |
| — `llTeleportAgent` back where it stood | **refused**: `TeleportFailed`, `Could not teleport closer to destination` | lands where it stood | no scripts |
| `llAttachToAvatarTemp(ATTACH_LHAND)` by the owner's prim, granted `PERMISSION_ATTACH` | `attach` with the owner's key; `llGetAttached` 5; the owner and the permissions unchanged | the same | no scripts |

A landing that is not the aim is the region's routing, not the function's:
Mauve has a telehub, and a teleport inside a region with one is sent to it
(the conformance support's walk to the build location exists for the same
reason). Where a parcel or a region routes nothing, as on the local OpenSim,
the avatar lands at the aim. The runner walks an avatar a probe's script
teleported back to the build location before the next probe.

### What the viewer does with it

Nothing an experience asks reaches the viewer from a script in no
experience: Second Life denies `llRequestExperiencePermissions` without a
question, and OpenSim does not compile it. The question a script *in* an
experience puts — a `ScriptQuestion` naming the experience — is unmeasured
for the reason above; the viewer's experience card is held to it as the
reference viewer reads it by `e2e_script_permissions` on the fake Second
Life flavour (*Yes* grants the mask and admits the experience). A script's
teleport reaches the viewer as an ordinary in-region teleport, the
`TeleportLocal` it handles for any other.

## Outbound HTTP and in-world URLs

Measured on 2026-10-25 by the probes `http/outbound`, `http/throttle` and
`http/url` (and on 2026-10-27 by `http/options`), on the same grids;
YEngine and XEngine answered every one of them alike, so the OpenSim
column is both. `http/outbound` sends each
request to the script's **own** in-world URL, so one script is both the
client and the server and nothing leaves the grid; `http/url` has the
runner send requests to a script's URL from outside the grid (the `fetch`
drive). OpenSim refuses scripts any address on the simulator's own network
by default (`OutboundDisallowForUserScripts`), its own included, so the
local grid lets them reach `127.0.0.1:9000` alone
(`OutboundDisallowForUserScriptsExcept`, stated in each result's
`settings`); every other llHTTPRequest limit is at its default.

### What arrives at the URL

What `llGetHTTPHeader` gives the receiving script of a plain
`llHTTPRequest(url, [], "")` and of the runner's request.

| behaviour | Second Life | OpenSim | fake grid |
| --- | --- | --- | --- |
| the URL `llRequestURL` grants | `http://<simhost>:12046/cap/<key>`, no slash at the end | `http://127.0.0.1:9000/lslhttp/<key>/`, **with** one — so `<url>/path` arrives as `//path` | no scripts |
| `x-path-info`, `x-query-string` | the path; the query **as sent** (`b=two%20words`) | the path; the query **decoded** (`b=two words`) | no scripts |
| `x-script-url` | the URL granted | the URL without its closing slash | no scripts |
| `x-remote-ip` | set | set (`127.…` for the script's own request) | no scripts |
| `x-secondlife-shard` | `Testing` (aditi) | `OpenSim` (the `[Network] shard` setting) | no scripts |
| `x-secondlife-object-name`, `-object-key`, `-owner-key` | the object's name and key, the owner's key | the same | no scripts |
| `x-secondlife-owner-name` | the owner's legacy name (`First Last`) | the same | no scripts |
| `x-secondlife-region` | `Mauve (255744, 257792)`: the name and the region's corner in metres | `Default Region (256000, 256000)`, the same form | no scripts |
| `x-secondlife-local-position`, `-local-rotation`, `-local-velocity` | the object's, in parentheses — `(52.000000, 62.000000, 34.594490)` — which a cast to `vector` or `rotation` does not read | the same | no scripts |
| `user-agent` of a script's request | `Second-Life-LSL/<sim version> (https://secondlife.com)` | **none** (`user_agent` is unset by default) | no scripts |
| `content-type` of a script's request | not given to the script | `text/plain;charset=utf-8`, or the `HTTP_MIMETYPE` | no scripts |
| `pragma` of a script's request | not given to the script | **inverted**: none by default, `no-cache` with `HTTP_PRAGMA_NO_CACHE, FALSE` (`ScriptsHttpRequests.cs` adds the header when the flag is off) | no scripts |
| a custom header (`HTTP_CUSTOM_HEADER`, or the runner's `X-Probe`) | **not given to the script**: `llGetHTTPHeader` answers the `x-*` headers above and `user-agent` only | given | no scripts |
| `host`, `accept`, `via`, `x-forwarded-for`, `content-length` | not given | not given | no scripts |
| a request body over 2048 bytes (3000 sent, from a script or from outside) | **cut to 2048** | all 3000 | no scripts |

### What `llHTTPRequest` accepts

A refusal is `NULL_KEY` from `llHTTPRequest` and a line on
`DEBUG_CHANNEL`; on OpenSim a refused request still returns a **key**, the
`DEBUG_CHANNEL` line comes, and no `http_response` ever follows.

| request | Second Life | OpenSim | fake grid |
| --- | --- | --- | --- |
| `GET`, `POST`, `PUT`, `DELETE`, `PATCH` | sent; the method arrives as given | the same | no scripts |
| `HEAD` | sent; the URL answers **400** with the raw `HTTP/1.1 400 Bad Request…` as the body, and the script never hears it | sent; the script hears it, the answer is 200 with no body | no scripts |
| `OPTIONS`, `get` (lower case), `FOO` | refused: `HTTP method <name> is unsupported` | sent as given; the script hears it | no scripts |
| `GET` with a body | sent; the body is dropped (the script reads 0 bytes) | **499**, `Cannot send a content-body with this verb-type.` | no scripts |
| `HTTP_CUSTOM_HEADER` `X-Probe` | accepted (a key, a 200); whether it went out no script can see, since `llGetHTTPHeader` does not give it | accepted; arrives | no scripts |
| `HTTP_CUSTOM_HEADER` `Host`, `User-Agent`, `Accept` | refused: `HTTP_CUSTOM_HEADER value is an invalid header` | refused: `llHTTPRequest: Name is invalid as a custom header at parameter 1` | no scripts |
| `HTTP_CUSTOM_HEADER` `X-SecondLife-Shard` | refused, the same | **sent**, and the shard arrives as the grid's own, not the script's | no scripts |
| `HTTP_BODY_MAXLENGTH` 16385 | refused: `HTTP_BODY_MAXLENGTH value is invalid` | accepted | no scripts |
| an unknown option (99) | refused: `HTTP parameter key 1 is not recognized` | refused: `llHTTPRequest: Parameter 0 is an invalid flag` | no scripts |
| `not a url`, `ftp://…` | refused: `URL passed to llHTTPRequest is not valid` | refused: `llHTTPRequest: Invalid url`, `… Invalid url schema` | no scripts |
| `http://127.0.0.1:1/` | refused, the same `not valid` | refused: `llHttpRequest: Request to http://127.0.0.1:1/ disallowed by filter` | no scripts |
| a host that does not resolve (`.invalid`) | **502**, an HTML `Unknown Host` page from the grid's proxy | **499**, `Error: NameResolutionFailure` | no scripts |

### What comes back

| behaviour | Second Life | OpenSim | fake grid |
| --- | --- | --- | --- |
| the status `llHTTPResponse` gives (201, 404, 500, 418) | arrives as given, to a script and outside | the same | no scripts |
| an answer of 3000 bytes, no `HTTP_BODY_MAXLENGTH` | 2048 bytes, metadata `[0, 2048]` (`HTTP_BODY_TRUNCATED` at 2048) | 2048 bytes, **empty metadata** | no scripts |
| — `HTTP_BODY_MAXLENGTH` 16384 | all 3000, empty metadata | the same | no scripts |
| — `HTTP_BODY_MAXLENGTH` 100 | 100 bytes, metadata `[0, 100]` | 100 bytes, empty metadata | no scripts |
| an answer of 5000 bytes, fetched from outside | all of it | the same | no scripts |
| `Content-Type` outside, by default | `text/plain; charset=utf-8` | `text/plain` | no scripts |
| — after `llSetContentType(CONTENT_TYPE_JSON)` | `application/json; charset=utf-8` | **`text/plain`**: the call has no effect | no scripts |
| — after `llSetContentType(CONTENT_TYPE_HTML)` | `text/plain; charset=utf-8`: HTML is served only to the owner's own viewer | `text/plain` | no scripts |
| `HEAD` from outside | **400**, the script never hears it | 200, the script hears it | no scripts |
| `OPTIONS` from outside | 200, `text/plain`, `OK` — **answered by the simulator**, the script never hears it | the script hears it and answers | no scripts |
| a request the script never answers | **504**, `Timed out waiting for response from internal server.`, after 26 s | **500**, `Script timeout`, after 25 s | no scripts |
| a request to a released URL | 404, `cap not found: '<key>'` | 404, an HTML `Page not found` | no scripts |

### The user agent, `Accept` and the extended error

Measured on 2026-10-27 by `http/options`, which sends to its own URL as
`http/outbound` does. OpenSim knows none of the three options:
`HTTP_USER_AGENT` is an `undefined constant` to YEngine and `CS0103 … does
not exist in the current context` to XEngine, so the script does not
compile. The refusals below are each a `NULL_KEY` and a line on
`DEBUG_CHANNEL`, as in
[What `llHTTPRequest` accepts](#what-llhttprequest-accepts); none stops
the script, a user agent with a space included, which the library table
calls a script error.

| request | Second Life | OpenSim | fake grid |
| --- | --- | --- | --- |
| `HTTP_USER_AGENT, "probe-agent/1.0"` | sent; the `user-agent` arrives as the default with the value **appended** after a space: `Second-Life-LSL/<sim version> (https://secondlife.com) probe-agent/1.0` | does not compile | no scripts |
| — given twice | sent; only the **last** is appended | — | no scripts |
| — with a space (`probe agent`) | refused: `HTTP_USER_AGENT 'probe agent' is invalid` | — | no scripts |
| — empty | refused: `HTTP_USER_AGENT '' is invalid` | — | no scripts |
| — 306 characters | refused, the same, the whole value quoted | — | no scripts |
| `HTTP_ACCEPT, "application/json"`, answered `text/plain` | sent; the script hears the request and answers, but what comes back is **415**, `Unsupported or unknown Content-Type.` — the simulator replaces the answer | — | no scripts |
| — answered after `llSetContentType(CONTENT_TYPE_JSON)` | 200, the script's answer | — | no scripts |
| `HTTP_ACCEPT, "text/plain"` | 200 | — | no scripts |
| `HTTP_ACCEPT` of two types (`application/json, text/plain`) | refused: `HTTP_ACCEPT: value '…' is invalid.` — one type only | — | no scripts |
| — `*/*` | refused: `HTTP_ACCEPT: Unsupported mime type '*/*'` | — | no scripts |
| — empty | refused: `HTTP_ACCEPT: value '' is invalid.` | — | no scripts |
| `accept` as the receiving script reads it | empty whatever was asked: `llGetHTTPHeader` does not give it | — | no scripts |
| `HTTP_EXTENDED_ERROR, TRUE`, a request that succeeds or one answered 404 | unchanged: 200 or 404 with the script's body | — | no scripts |
| — with a request `llHTTPRequest` refuses | **a key**, the same line on `DEBUG_CHANNEL`, and an `http_response` at once whose body is an RFC 7807 JSON object: `detail`, `status`, `title`, `type` (`http://wiki.secondlife.com/wiki/llHTTPRequest`), the status the answer's own | — | no scripts |
| — not a URL, `ftp://…`, `http://127.0.0.1:1/` | **471**, `llHTTPRequest URL is invalid.`, detail `URL passed to llHTTPRequest is not valid` | — | no scripts |
| — an unknown option (99) after the flag | **472**, `llHTTPRequest parameter error.`, `Parameter key in position 2 is not recognized.` (the line on `DEBUG_CHANNEL` says `key 2` too, where without the flag it said `key 1`) | — | no scripts |
| — the unknown option **before** the flag | refused as without it, `NULL_KEY`: the list is read in order and stops at the first bad option | — | no scripts |
| — a user agent with a space | 472, `Invalid user agent probe agent in position 2` | — | no scripts |
| — `HTTP_CUSTOM_HEADER` `Host` | **473**, `Invalid custom header in position 2` | — | no scripts |
| — `HTTP_BODY_MAXLENGTH` 16385 | 472, `Maximum body length must be in the rage (0, 16384] in position 2` (sic) | — | no scripts |
| — method `FOO` | **405**, `Unrecognized HTTP Method.`, `HTTP method FOO is unsupported at position 2.` | — | no scripts |
| — a host that does not resolve | 502 with the proxy's HTML `Unknown Host` page, as without the flag: the request was sent, so there is no problem report | — | no scripts |
| — an `HTTP_ACCEPT` the answer does not meet | 415 with the plain `Unsupported or unknown Content-Type.`, as without the flag | — | no scripts |
| `HTTP_EXTENDED_ERROR, FALSE` with a URL that is not one | `NULL_KEY`, as with no flag | — | no scripts |

A prim's complaints on `DEBUG_CHANNEL` are cut off when they come fast:
eleven refusals in about thirty seconds, most 1.2 s apart, ended in `Too many
errors... dropping further messages until the flood stops`, and the
complaints after it never came. `http/options` waits four seconds after
each refusal for that reason.

### URLs: what a script may hold

| behaviour | Second Life | OpenSim | fake grid |
| --- | --- | --- | --- |
| `llGetFreeURLs` before any | 7851 in Mauve's sandbox parcel (the parcel's and region's allowance, shared with everyone there) | 100 | no scripts |
| `llRequestURL` | `URL_REQUEST_GRANTED`; the `http_request` id is the key the call returned; one URL fewer | the same | no scripts |
| `llRequestSecureURL` | granted: `https://<simhost>:12043/cap/<key>` | **`URL_REQUEST_DENIED`**, no reason: the local simulator runs no HTTPS listener | no scripts |
| `llReleaseURL` | the URL is free again at once | the same | no scripts |

### The throttle

`http/throttle` sends 30 requests in one event, then five more with
`HTTP_VERBOSE_THROTTLE` off, all to its own URL.

| behaviour | Second Life | OpenSim | fake grid |
| --- | --- | --- | --- |
| the first 30 | **25** accepted, the next 5 refused (`NULL_KEY`), each with `Too many HTTP requests too fast.` on `DEBUG_CHANNEL` | **4** accepted (a burst of 3 and one more — `PrimRequestsBurst` 3, `PrimRequestsPerSec` 1), 26 refused, **no message** | no scripts |
| five more with `HTTP_VERBOSE_THROTTLE, FALSE` | all refused, silently | all refused, silently | no scripts |
| what came back for the accepted ones | 200 each; nothing for a refused one (no 499) | the same | no scripts |
| one more request 40 s later | accepted, 200 | the same | no scripts |

How the throttle lets go again, and why a script that retries at once stays
shut out on Second Life, is in
[HTTP requests: how the throttle lets go](#http-requests-how-the-throttle-lets-go).

### What the viewer does with it

Nothing: none of this reaches a viewer. A refused request's complaint is an
ordinary `DEBUG_CHANNEL` line, and the HTML a script serves is only drawn as
HTML by the owner's own viewer's browser, which `http/url` did not measure.

## llJson* and the linkset data store

Measured on 2026-10-26 by the probes `data/json` and `data/linkset-data`, on
the same grids; YEngine and XEngine answered `data/json` alike, so the
OpenSim column is both. OpenSim has no linkset data at all: `data/linkset-data`
does not compile under either engine (YEngine: `undefined
constant/function/variable llLinksetDataAvailable()`; XEngine: `syntax error`
at the `linkset_data` event), so its tables are Second Life's alone.

### The sentinels

`JSON_INVALID` … `JSON_DELETE` are one character each, `U+FDD0` … `U+FDD8`
(`llEscapeURL` gives `%EF%B7%90` for `JSON_INVALID`), on both grids.

| behaviour | Second Life | OpenSim | fake grid |
| --- | --- | --- | --- |
| `llSubStringIndex("abc", JSON_INVALID)` | -1 | **0**: OpenSim's string search treats the sentinels as characters to ignore, so it finds one at the start of any string that is not empty | no scripts |
| `llSubStringIndex("a" + JSON_INVALID, JSON_INVALID)` | 1 | **0** | no scripts |
| `llParseStringKeepNulls("abc", [JSON_INVALID], [])` | `["abc"]` | **`["", "", "", ""]`**: a separator between every character, and the characters lost | no scripts |
| `==`, `llListFindList` with a sentinel | not equal, -1 | the same | no scripts |
| a list variable of no specifiers given to `llJsonGetValue`, `llJsonValueType` or `llJsonSetValue` | unchanged | **gains an element**, `0`: OpenSim adds to the list it is given (an empty `[]` written in the call is not affected) | no scripts |

### `llList2Json`

| input | Second Life | OpenSim | fake grid |
| --- | --- | --- | --- |
| an integer, a float | `1`, `2.500000` (six decimals) | the same | no scripts |
| a vector, a rotation, a key | quoted strings of their LSL text (`"<1.000000, 2.000000, 3.000000>"`) | the same | no scripts |
| `1.0e20`, `-0.0` | `100000002004087734272.000000`, `-0.000000` (single precision) | `100000000000000000000.000000`, `0.000000` | no scripts |
| `(float)"1e39"` (infinity on Second Life) | **`inf`**, `-inf`: not JSON | the number written out in full: OpenSim's floats are doubles | no scripts |
| `"true"`, `"false"`, `"null"`, `JSON_TRUE`, `JSON_FALSE`, `JSON_NULL` | `true`, `false`, `null` | the same | no scripts |
| a string starting with `[` or `{` | inserted as it is, even when it is no JSON (`{bad`, `[1,]`) | the same | no scripts |
| — with spaces round it (`" [2] "`) | trimmed and inserted (`[2]`) | a string, spaces kept | no scripts |
| `"\"q\""` | **`"q"`**: a quoted string is taken as a JSON string | the string escaped, `"\"q\""` | no scripts |
| `" 7 "`, `"7"`, `"1e3"`, `"-0"` | strings; `" 7 "` trimmed to `"7"` | strings, spaces kept | no scripts |
| a newline, `\`, `/` | `\n`, `\\`, `\/` | the same | no scripts |
| characters 1 and 127 | **raw** in the string | **`\u000`** followed by the raw characters — a broken escape | no scripts |
| `é€`, an emoji | raw UTF-8 | the same | no scripts |
| `JSON_INVALID` as an element | **nothing**: `[,"…"]` | the sentinel character in a string | no scripts |
| `JSON_DELETE`, `JSON_OBJECT`, `JSON_STRING` as elements | the sentinel character in a string | the same | no scripts |
| `JSON_OBJECT` of pairs | `{"a":1,"b":"x","c":[1,2],"d":2.500000}` | the same | no scripts |
| — a key repeated | both pairs kept | the same | no scripts |
| — an odd count, an integer or vector key | `JSON_INVALID` | the same | no scripts |
| — a `key` as a key | `JSON_INVALID` | **accepted** as a string | no scripts |
| — an empty key | `{"":1}` | the same | no scripts |
| a type other than `JSON_ARRAY` or `JSON_OBJECT` | `JSON_INVALID` | the same | no scripts |

### `llJson2List`

Second Life's parser is lenient and parses a scalar; OpenSim parses only a
document whose first character is `[` or `{`, and answers anything it
cannot read with the whole text as one string.

| input | Second Life | OpenSim | fake grid |
| --- | --- | --- | --- |
| `[1,2.5,"a",true,false,null,[1,2],{"a":1}]` | integer 1, float 2.5, `"a"`, `JSON_TRUE`, `JSON_FALSE`, `JSON_NULL`, and the nested two as strings of their text | the same | no scripts |
| an object | key, value, key, value… | the same | no scripts |
| `"  [ 1 , 2 ]  "` | 1, 2 | **the text** | no scripts |
| `1`, `"s"`, `true` | integer 1, `s`, `JSON_TRUE` | the text | no scripts |
| `""`, `{}`, `[]` | an empty list | the same | no scripts |
| `garbage`, `[1,`, `[1] x` | the text | the same | no scripts |
| `[1,]` | 1, `""` | the text | no scripts |
| `{"a"}` | `""`, `a` | the text | no scripts |
| `[01]` | the string `01` | the text | no scripts |
| `[1.]`, `[.5]` | floats 1.0, 0.5 | the text | no scripts |
| `[1e5,1E-2,-0,2.50]` | floats, integer 0, float | the same | no scripts |
| `["a" "b"]` | `b` | the text | no scripts |
| `[truth]` | `truth` | the text | no scripts |
| `{'a':1}` | `JSON_INVALID` | **`a`, 1**: single quotes accepted | no scripts |
| `["A\n\/\"\\"]` | **not decoded**: one string, from the opening quote to the end of the text | decoded: `A`, a newline, `/`, `"`, `\` | no scripts |
| `["😀"]` | `ud83dude00`: the backslashes dropped | the emoji | no scripts |
| `["\ud83d"]` (a lone half) | `ud83d` | a lone surrogate, which `llEscapeURL` then answers with the text of an exception | no scripts |
| `[12345678901234567890,2147483648,1.5e300,-1e400]` | integers **2147483647** twice, floats `Infinity`, `-Infinity` | the text | no scripts |
| a key repeated (`{"a":1,"a":2}`) | both pairs | **the last only** | no scripts |
| a raw newline or control character in a string | kept | the same | no scripts |

### `llJsonGetValue` and `llJsonValueType`

Of `{"a":1,"b":[10,"x",true,null,{"c":2.50}],"s":"t\"u","n":-1.0e2}`.

| path | Second Life | OpenSim | fake grid |
| --- | --- | --- | --- |
| `[]` | the document **as written** | the document **written anew**: `2.5`, `-100.0` | no scripts |
| `["b",4,"c"]`, `["n"]` | `2.50`, `-1.0e2`, as written; `JSON_NUMBER` | `2.5`, `-100.0` | no scripts |
| `["b",2]`, `["b",3]` | `JSON_TRUE`, `JSON_NULL`, and the same type | the same | no scripts |
| `["s"]` | `t"u`, `JSON_STRING` | the same | no scripts |
| `["b","0"]` (a string index) | **`10`** | `JSON_INVALID` | no scripts |
| `["b",5]`, `["b",-1]`, `["b",JSON_APPEND]`, `["b",1.0]` | `JSON_INVALID` | the same | no scripts |
| `["a",0]`, `[0]` on the object, `["z"]` | `JSON_INVALID` | the same | no scripts |
| a key repeated | the last value | the same | no scripts |
| `"\"str\""`, `"  7 "` with `[]` | `str` (`JSON_STRING`), `7` (`JSON_NUMBER`) | the same | no scripts |
| `abc`, `""`, `[1,2`, `[1,2,}` | `JSON_INVALID` | the same | no scripts |

`JSON_APPEND` is the **integer** -1 on Second Life and the **string** `"-1"`
on OpenSim.

### `llJsonSetValue`

| call | Second Life | OpenSim | fake grid |
| --- | --- | --- | --- |
| `{"a":1}`, `["b"]`, `"x"`, `"true"`, `"  7 "`, `""` | `"x"`, `true`, `7`, `""` | the same | no scripts |
| — `"1.50"`, `"-0"` | `1.50`, `-0`, as written | `1.5`, `0` | no scripts |
| — `"01"` | the string `"01"` | the number `1` | no scripts |
| — `"[1, 2]"` | inserted as JSON, as written | **the string** `"[1, 2]"` | no scripts |
| — `"\"q\""` | the string `"q"` | the string escaped, `"\"q\""` | no scripts |
| — `"{bad"` | a string | the same | no scripts |
| — `JSON_TRUE`, `JSON_NULL` | `true`, `null` | the same | no scripts |
| — `JSON_INVALID` | **nothing**: `{"a":1,"b":}` | the text `"﷐"` | no scripts |
| `JSON_DELETE` on a key or an element | removed | the same | no scripts |
| `JSON_DELETE` of what is not there | `JSON_INVALID` | the same | no scripts |
| a new path, `["x","y"]`, `["x",0]` | `{"y":1}`, `[1]` made | the same | no scripts |
| `["x",1]` in a new array, `[3]` in `[1,2]`, `[-2]` | `JSON_INVALID` | the same | no scripts |
| `[2]` in `[1,2]`, `[JSON_APPEND]` | appended | the same | no scripts |
| a key through a number (`["a","b"]`), `[0]` on an object, `[]` | the value replaced: `{"a":{"b":1}}`, `[1]`, the value alone | the same | no scripts |
| a key on an array (`[1,2]`, `["k"]`) | **the array replaced**: `{"k":1}` | **unchanged**: `[1,2]` | no scripts |
| on `[1`, on `abc` | `[2]`, `{"a":1}`: written over | `JSON_INVALID` | no scripts |
| on `""` | `{"a":1}`, `[1]` | the same | no scripts |
| the rest of the document | the level written to compact, the values in it as written (`{"a":2,"c":[ 1 ]}` of `{"a" : 1 , "c" : [ 1 ] }`) | all of it written anew, compact | no scripts |
| a key repeated | one pair, the new value | the same | no scripts |

### The linkset data store

| behaviour | Second Life | OpenSim | fake grid |
| --- | --- | --- | --- |
| an empty store | `llLinksetDataAvailable` 131072, no keys, a missing key reads `""` | none | no scripts |
| what a pair costs | the bytes of name and value in UTF-8 (`a`=`1`: 2; ten `é` as the value: 21) | none | no scripts |
| a protected pair | **32 bytes more**, whatever the password's length (2 or 20 characters) | none | no scripts |
| — with an empty password | an ordinary pair | none | no scripts |
| writing the value it has | `LINKSETDATA_NOUPDATE` (5), no event | none | no scripts |
| writing another value | the difference in bytes | none | no scripts |
| an empty value | deletes the pair (`LINKSETDATA_OK`, a delete event); of a missing name `LINKSETDATA_NOTFOUND` (4) | none | no scripts |
| an empty name | `LINKSETDATA_ENOKEY` (2) | none | no scripts |
| a name of 1100 characters | accepted | none | no scripts |
| reading a protected pair: `llLinksetDataRead`, a wrong password | `""` | none | no scripts |
| — `llLinksetDataReadProtected` of an ordinary pair | `""` | none | no scripts |
| writing or deleting a protected pair without its password, giving it a new one, protecting an ordinary pair | `LINKSETDATA_EPROTECTED` (3) | none | no scripts |
| deleting a protected pair with its password | `LINKSETDATA_OK`; `llLinksetDataDeleteProtected` of an ordinary pair is `LINKSETDATA_EPROTECTED` | none | no scripts |
| deleting a missing name | `LINKSETDATA_NOTFOUND` | none | no scripts |
| `llLinksetDataListKeys` | sorted by code point (`K0,k1,k10,k2,…`); a count of 0 or -1 is all of them, a start past the end none, a negative start 0 | none | no scripts |
| `llLinksetDataFindKeys`, `llLinksetDataCountFound` | a regular expression, with start and count as above; an empty pattern and one that is no regular expression (`(`) find nothing, without an error | none | no scripts |
| `llLinksetDataDeleteFound` | `[deleted, not deleted]`: a protected pair is not deleted without its password (`4, 1`), and is with it | none | no scripts |
| filling it with pairs of 1000 characters | 130 fit, the 131st is `LINKSETDATA_EMEMORY` (1); a pair of exactly the bytes left is accepted, `llLinksetDataAvailable` is then 0 | none | no scripts |
| `llLinksetDataReset` | empty again, 131072 | none | no scripts |
| `llResetScript` | the store is kept | none | no scripts |
| another object | a store of its own: neither sees the other's pairs | none | no scripts |

### The `linkset_data` event

| behaviour | Second Life | OpenSim | fake grid |
| --- | --- | --- | --- |
| a write | `LINKSETDATA_UPDATE`, the name and the value; for a protected pair the value is `""` | no such event | no scripts |
| a delete, an empty value | `LINKSETDATA_DELETE`, the name, `""` | no such event | no scripts |
| `llLinksetDataDeleteFound` | **one** `LINKSETDATA_MULTIDELETE` per call, the names comma-separated without spaces (`k1,k10,k2,k3`), `""` | no such event | no scripts |
| `llLinksetDataReset` | `LINKSETDATA_RESET`, `""`, `""` | no such event | no scripts |
| a refused write, `LINKSETDATA_NOUPDATE`, a delete of a missing name | no event | no such event | no scripts |
| who gets it | every script in the linkset, the writer too, in the order of the calls | no such event | no scripts |
| a script writing 131 pairs in one event | its queue fills: 64 events, one of them its own link message, the rest dropped | no such event | no scripts |

### What the viewer does with it

Nothing: neither JSON nor linkset data reaches a viewer.

## Parcel, region, estate and environment queries

Measured on 2026-10-26 by the probes `land/env`, `land/parcel`,
`land/region`, `land/ground`, `land/day-cycle` and `land/sl-only`, on the
same grids; YEngine and XEngine answered every one of them alike (but for a
dwell that moved by one between the runs), so the OpenSim column is both.
The land is each grid's own: Second Life is the Mauve public sandbox, a
group-owned mainland parcel of 14 176 m² our avatars do not own; OpenSim is
the Default Region, one parcel of the whole region, owned by the estate
owner rather than the probe's owner. So the heights, counts and names in the
transcripts are the land's; the tables hold what the calls do with them.
OpenSim lacks `llGetSimStats`, `llGetEnvironment`, `llGetRegionTimeOfDay`
and `llGetClosestNavPoint`, and every `ENVIRONMENT_*`, `SKY_*` and `WATER_*`
constant: `land/sl-only` holds them so the other probes compile there, and
does not compile under either engine (YEngine: `undefined
constant/function/variable llGetEnvironment(vector,list)`; XEngine:
`CS0103: The name 'llGetEnvironment' does not exist`).

### `llGetEnv`

| key | Second Life | OpenSim | fake grid |
| --- | --- | --- | --- |
| `sim_channel`, `sim_version` | `Luau`, the release (`2026-07-24.30110090421`) | `OpenSim`, the version string with build and platform (`OpenSim 0.9.2.1 Yeti Dev   36f6d16 (Unix/Mono)`) | no scripts |
| `estate_id`, `estate_name` | `1`, `mainland` | the estate's (`101`, `My Estate`) | no scripts |
| `region_product_name`, `region_product_sku` | `Mainland / Full Region`, `023` | `""` (the region type, empty here), always `OpenSim` | no scripts |
| `region_max_prims`, `region_object_bonus` | the land capacity with the bonus (`22510`), the bonus (`10`) | the object capacity (`15000`), the bonus (`1`) | no scripts |
| `agent_limit` | `55` | the region setting (`40`) | no scripts |
| `whisper_range`, `chat_range`, `shout_range` | `10`, `20`, `100` | the same (the configured distances) | no scripts |
| `dynamic_pathfinding` | `enabled` | always `0` | no scripts |
| `region_cpu_ratio`, `region_idle` | `1`, `0` | always `1`, always `0` | no scripts |
| `region_start_time` | Unix seconds | the same | no scripts |
| `frame_number` | counts 45 a second, as `llGetRegionFPS` | counts **11 a second**, while `llGetRegionFPS` says 55 | no scripts |
| `region_up_time` | `""` | seconds since start | no scripts |
| `simulator_hostname` | the host's name (`simhost-….aditi.secondlife.io`), as `llGetSimulatorHostname` | the external host name configured for scripts (`127.0.0.1`), as `llGetSimulatorHostname` | no scripts |
| `agent_reserved`, `agent_unused` | `5`, `""` | `""` | no scripts |
| `region_rating`, `grid`, `region_name` | `ADULT`, `aditi`, `Mauve` | `""` | no scripts |
| `allow_damage_adjust`, `damage_limit` | `1`, `0` | `""` | no scripts |
| `restrict_pushobject`, `estate_owner`, `region_corner`, `region_idle_timeout`, `agent_avatar_limit`, a made-up key, `""` | `""` | `""` | no scripts |
| a key in capitals (`SIM_CHANNEL`, `Sim_Version`) | `""`: the key is case-sensitive | **answered**: OpenSim lowers the key first | no scripts |

### Parcel queries

The `PARCEL_DETAILS_*` codes are the probe's numbers, because the grids name
three of them differently: Second Life's 7, 8 and 10 are `PRIM_CAPACITY`,
`PRIM_USED` and `LANDING_LOOKAT`, OpenSim's `ANY_AVATAR_SOUNDS`,
`GROUP_SOUNDS` and `CLAIMDATE`, and OpenSim adds 64 (`DWELL`). A script
naming a constant of one grid does not compile on the other.

| call | Second Life | OpenSim | fake grid |
| --- | --- | --- | --- |
| `llGetParcelDetails` 0–6: name, description, owner, group, area, id, see-avatars | the parcel's (the owner of group land is its group); see-avatars `0` | the same, the group `NULL_KEY` | no scripts |
| — 7, 8 | the prim capacity (`78532`) and the prims used | `0`, `0` | no scripts |
| — 9, 10 | the landing point and look-at, vectors | integer `0` | no scripts |
| — 11 (`TP_ROUTING`), 12 (`FLAGS`), 13 (`SCRIPT_DANGER`) | `1`, the parcel flags, `1` | integer `0` | no scripts |
| — 64 | nothing, and a `DEBUG_CHANNEL` error | the dwell (`56`) | no scripts |
| — any other code (-1, 14, 15, 99) | **left out**, and `llGetParcelDetails error: Unknown parameter '99'` on `DEBUG_CHANNEL` | **integer `0`** in its place, silently | no scripts |
| — several codes | one element per code, in the order asked, repeats repeated | the same | no scripts |
| — no codes | `[]` | the same | no scripts |
| — a position above the region (z 5000) or under the ground (z −100) | the parcel below or above it | the same | no scripts |
| — a position outside the region (10 m west) | **the parcel at the edge it is clamped to** (the same id) | **one element**, `0`: no parcel | no scripts |
| `llGetParcelFlags` | the parcel's flags, with `PF_ALLOW_VOICE_CHAT` (`0x20000000`) beside the named ones | the same with `PF_ALLOW_DEED_TO_GROUP` (`0x2000`) too | no scripts |
| — outside the region | the edge parcel's flags | **throws**: a `NullReferenceException` that stops the script (`Script must be Reset to re-enable.`) | no scripts |
| `llGetParcelMaxPrims`, parcel and region-wide | the region's capacity both times (`22510`): mainland pools it | the same (`15000`) | no scripts |
| — outside the region | an answer (`11650`, a parcel's) | `0` | no scripts |
| `llGetParcelPrimCount`, total / owner / group / other, parcel and region-wide | counts; region-wide larger | counts; the **region-wide owner and other counts are `0`** while the parcel's are not | no scripts |
| — selected (4), temporary (5) | counts | `0` | no scripts |
| — a category that is none (−1, 6) | `0` | `0` | no scripts |
| — outside the region | a parcel's count | `0` | no scripts |
| `llGetParcelPrimOwners` of land the script's owner does not own | `[]` | **the list**: each owner and its count | no scripts |
| `llGetLandOwnerAt` | the parcel's owner (its group for group land) | the same | no scripts |
| — outside the region | an owner, not `NULL_KEY` | `NULL_KEY` | no scripts |
| `llOverMyLand` of the owner on land it does not own; of `NULL_KEY` | `0`; `0` | the same | no scripts |
| `llScriptDanger` | `1`: damage or push on this public sandbox | `0` | no scripts |
| — outside the region | `1` | `1` | no scripts |
| `llParcelMediaQuery`, any code or several, on land the owner does not own | `[]` | `[]` | no scripts |
| `llGetParcelMusicURL` | the stream (none here: `""`) | the same | no scripts |
| `llSetParcelMusicURL` on land the owner does not own | ignored: the URL is unchanged, and nothing is said | the same | no scripts |

### Region queries

| call | Second Life | OpenSim | fake grid |
| --- | --- | --- | --- |
| `llGetRegionCorner` | the region's corner in metres, a multiple of 256 | the same | no scripts |
| `llGetRegionFlags` | only the documented `REGION_FLAG_*` bits (`ALLOW_DIRECT_TELEPORT` here) | **the raw region flags**: `0x14000006` here, the reference's `REGION_FLAGS_DEFAULT` (landmark, set home, parcel changes, voice), none of them a `REGION_FLAG_*` | no scripts |
| `llGetRegionFPS`, `llGetRegionTimeDilation` | about 45, about 0.99 | about 55, 1.0 | no scripts |
| `llGetRegionAgentCount` | the avatars in the region | the same | no scripts |
| `llGetAgentList` with `AGENT_LIST_PARCEL` (1), `_PARCEL_OWNER` (2), `_REGION` (4) | the avatars in the scope, the owner among them | the same | no scripts |
| — any other scope (0, 8) | `["INVALID_SCOPE"]` | the same | no scripts |
| `llEdgeOfWorld` from inside, along x or y | `1` where no region adjoins (Mauve has none on any side) | `1` where none does (west and south of the Default Region), `0` towards East and North | no scripts |
| — a direction with an upward part | `1` (as every direction on Mauve) | `1` straight up; with a level part, as that part says | no scripts |
| — a direction of zero length | `1` | `1` | no scripts |
| — from a position outside the region | `0` | `1` | no scripts |
| `llRequestSimulatorData`, `DATA_SIM_POS` (5) of the own region | the corner, `<255744,257792,0>`: integers, no spaces | **`<1000.000000, 1000.000000, 0.000000>`: the grid coordinates in regions** — the own region's branch reads `RegionInfo.RegionLocX`, which counts regions | no scripts |
| — of the own region's name in lower case | the same as the name: names are case-insensitive | **the corner in metres** (`<256000.000000, …>`): the name is not the region's own, so the grid service answers | no scripts |
| — `DATA_SIM_STATUS` (6), `DATA_SIM_RATING` (7) | `up`, `ADULT` | `up`, `PG` | no scripts |
| — `DATA_SIM_RELEASE` (128) | **no request**: `NULL_KEY` | `OpenSim` | no scripts |
| — an unknown code (99) | `NULL_KEY` | `NULL_KEY` | no scripts |
| — a region no grid has | status `unknown`; the position is **never answered** | `unknown` to both | no scripts |
| — the empty name | status **`crashed`** | `unknown` | no scripts |

### Ground and weather

| call | Second Life | OpenSim | fake grid |
| --- | --- | --- | --- |
| `llGround` | the terrain under the offset; a vertical offset changes nothing | the same | no scripts |
| — an offset out of the region | the height at the region edge it is clamped to (constant beyond the edge, within 0.04 m of the height at the edge itself) | the height at the edge, exactly | no scripts |
| `llGroundNormal`, `llGroundSlope`, `llGroundContour` on level ground | `<0,0,1>`, zero, zero | the same | no scripts |
| `llWater` | the region's water height (20), wherever the offset points | the same | no scripts |
| `llWind` | level (z 0), a few m/s, different 100 m away | level, about 1 m/s, different 100 m away | no scripts |
| `llCloud` | `0.0` | `0.0` | no scripts |

### Day cycle and environment

| call | Second Life | OpenSim | fake grid |
| --- | --- | --- | --- |
| `llGetDayLength`, `llGetDayOffset` and the region's | `14400`, `57600`, the parcel's the region's | the same | no scripts |
| `llGetTimeOfDay` | seconds into the day, below the day length, advancing | the same | no scripts |
| `llGetSunDirection`, `llGetMoonDirection` and the region's | unit vectors in the x–z plane, the moon opposite the sun, the parcel's the region's | the same | no scripts |
| `llGetSunRotation`, `llGetMoonRotation` and the region's | unit rotations whose forward axis is the direction | the same | no scripts |
| `llGetRegionTimeOfDay` | the region's time of day, the parcel's here | none | no scripts |
| `llGetEnvironment`, `ENVIRONMENT_DAYINFO` | `[day length, offset, time of day]` | none | no scripts |
| — each `SKY_*` and `WATER_*` rule | the setting's values (`SKY_AMBIENT` one vector, `SKY_CLOUDS` eight values, …) | none | no scripts |
| — `SKY_CLOUD_TEXTURE`, `SKY_MOON_TEXTURE`, `SKY_SUN_TEXTURE`, `WATER_NORMAL_TEXTURE` | `[]`, and `llGetEnvironment Error running rule #1 (SKY_SUN_TEXTURE): Attempt to read unreadable value..` on `DEBUG_CHANNEL`: the textures are not the script owner's | none | no scripts |
| — an unknown rule | `[]`, `unknown rule id 999.` | none | no scripts |
| — above the region (z 3000), outside it (10 m west) | answered: the same sky, the same day info | none | no scripts |
| `llGetSimStats`, codes 0–26 | a float each; unknown codes (−1, 27) `0.0` and `Unknown stat.` on `DEBUG_CHANNEL` | none | no scripts |
| `llGetClosestNavPoint` | the nearest point of the navmesh, a few metres away | none | no scripts |

### What the viewer does with it

Nothing directly: these calls read the region a viewer is also told about
(parcel properties, region handshake, the environment), and none of them
sends the viewer anything.

## Memory and limits

Measured on 2026-10-26 by the probes `memory/budget`, `memory/budget-lso`,
`memory/recursion`, `memory/collision` and `memory/object`, on the same
grids. A probe script marked `target: lsl2` is compiled to LSO on Second
Life; OpenSim ignores the upload's target, so there the LSO twins run as
every other script does and answer alike. The exact per-value costs are
`memory/sizes`' ([What a value costs, exactly](#what-a-value-costs-exactly),
measured on 2026-10-30); the first table of costs below is what the earlier
probes saw on the way.

### The memory functions

| call | Second Life, Mono | Second Life, LSO | OpenSim YEngine | OpenSim XEngine | fake grid |
| --- | --- | --- | --- | --- | --- |
| `llGetMemoryLimit` | `65536`, or what `llSetMemoryLimit` set | `16384` | always `65536` | always `65536` | no scripts |
| `llGetUsedMemory` | what the script uses now (about 13 500 bytes for `memory/budget`, 6 500 for a small script); it falls again when a value is cleared | always `16384`, the whole budget | the heap the script's values hold, counted by YEngine's heap tracker (`26` bytes for `memory/budget` at start: its one global string) | always `65536` | no scripts |
| `llGetFreeMemory` | the limit less the **most** the script has used since the limit was last set: it falls as the script grows and never rises when it shrinks | the budget less the most it has used: likewise a low-water mark | the 1 MiB heap (`ScriptHeapSize`) less what is used now: it rises again when a value is cleared | always `65536` | no scripts |
| `llScriptProfiler(PROFILE_SCRIPT_MEMORY)`, then `llGetSPMaxMemory` | `0` before any profiling; after, the most used while profiling (`14562`) | `16384` | always `65536` | always `65536` | no scripts |

### What a value costs

What `llGetUsedMemory` (Mono, YEngine) or `llGetFreeMemory` (LSO) moved by
as a global list grew from 2 to 64 elements, per element; and a global
string per character.

| value | Second Life, Mono | Second Life, LSO | OpenSim YEngine | fake grid |
| --- | --- | --- | --- | --- |
| an integer in a list | 16 bytes (exactly 8 000 per 500 in `memory/collision`); the list itself about 200 | about 30, the copy an append makes included | 4 | no scripts |
| a float | 16 | nothing more after the integers' low-water mark | 8 | no scripts |
| a one-character string | a few bytes, unevenly | nothing more | 1 | no scripts |
| a key | 80 to 100 | the low-water mark fell by 4 251 at 64 keys | 36 | no scripts |
| a vector | 24 | nothing more | 32 | no scripts |
| a rotation | 28 | nothing more | 64 | no scripts |
| a character of a string | 2, whether one, two or three bytes in UTF-8 (U+00E9, U+20AC) | 1 024 ASCII or two-byte characters stayed within the earlier low-water mark; 1 024 three-byte ones lowered it by 2 899 | 2, whatever the character | no scripts |

XEngine counts nothing: every read is `65536`.

### What a value costs, exactly

`memory/sizes` reads `llGetUsedMemory` into locals immediately around one
thing — a global list growing from empty to a count of fresh values, a
global string or key taking a value, a function holding four locals or
taking four arguments, a recursion some calls deep — three times over, and
prints the three differences. Eight more scripts in the prim each declare 32
globals of one type and read themselves, against one that declares none.
On aditi most rows read the same number at least twice; a row of a hundred
values or more rarely repeats, wandering by up to 160 bytes either side,
and the model below sits within that. `sl-lsl-runtime`'s
`value::memory` holds the costs, and its test holds them to all three
transcripts.

| what | Second Life, Mono | OpenSim YEngine | fake grid |
| --- | --- | --- | --- |
| a list, once it holds anything, over an empty one | 64 | 0 | no scripts |
| an integer in a list | 16: a 4-byte reference and the boxed value | 4 | no scripts |
| a float in a list | 16 | 8 (a double) | no scripts |
| a vector in a list | 24 | 32 | no scripts |
| a rotation in a list | 28 | 64 | no scripts |
| a key (a UUID) in a list | 102 | 36: a byte a character | no scripts |
| a string of *n* characters in a list | 18 + 2*n* | *n* | no scripts |
| one literal string in every element | the string once, then 4 an element | *n* every element | no scripts |
| a global string, over `""` | 2 a character | 2 a character | no scripts |
| a global key, over an empty key | **0**: a UUID costs what an empty key does | 72: 2 a character | no scripts |
| a local or an argument | **0**, of any type: not even a fresh string held only by a local shows | a local string 2 a character, a local list its elements; any other local, and any argument, 0 | no scripts |
| a call frame, 32 deep | 0 | 0 | no scripts |
| a global declared holding its default | integer, float or key 20; string 34; list 56; vector 60; rotation 64 | integer 4, float 8, vector 24, rotation 32; string, key or list 0 | no scripts |
| a character of a string literal in the code | 2 | — (the literal's global, 2 a character) | no scripts |
| a script that only reads its memory and says it | 4 456 | 32 | no scripts |

A character is a UTF-16 code unit on both grids: U+00E9 and U+20AC cost what
`a` costs. Every grid's `llGetMemoryLimit` starts at `65536`; XEngine counts
nothing and answers `65536` to every read.

So Second Life's Mono counts what the script's globals reach — a list's
boxes, a string's characters — and its code, but no value on the stack; a
recursion still collides (§ Running out), so the stack is checked
elsewhere. YEngine's numbers are its heap tracker's own
(`XMRHeapTracker.Size`, `LSL_List.Size`), which charges a string in a list
half what it charges the same string anywhere else.

### `llSetMemoryLimit`

| asked for | Second Life, Mono | Second Life, LSO | OpenSim (both engines) | fake grid |
| --- | --- | --- | --- | --- |
| `65536` | `TRUE` | `FALSE` | `FALSE`, and the limit stays `65536` | no scripts |
| more than `65536` (`65537`, `100000`) | `FALSE` | `FALSE` | `FALSE` | no scripts |
| less, but not below what the script uses (`32768`, used + 1024) | `TRUE`; the limit is the number asked, and `llGetFreeMemory` is reset to the new limit less what is used now | `FALSE` | `FALSE` | no scripts |
| exactly `16384` | `TRUE` when the script uses less | `TRUE`, the one value LSO takes | `FALSE` | no scripts |
| below what the script uses, `1`, `0`, `-1` | `FALSE`, the limit unchanged | `FALSE` | `FALSE` | no scripts |

### Running out

| case | Second Life, Mono | Second Life, LSO | OpenSim YEngine | OpenSim XEngine | fake grid |
| --- | --- | --- | --- | --- | --- |
| recursion, one integer argument (`Narrow`) | `Stack-Heap Collision` at a depth that differs from run to run: 1 085, 1 039 and 1 116 in three runs | at depth 763, every run | **the simulator aborts**: with a library call at every level the native stack overflows inside YEngine's own stack check (`CheckRunStack`) — `skip-on` | the simulator aborts — `skip-on` | no scripts |
| recursion, five arguments and two locals (`Wide`) | at 512, 413 and 518 | at 208 | `skip-on` | `skip-on` | no scripts |
| recursion under `llSetMemoryLimit(used + 4096)` | at 253, 129 and 151: the stack counts against the lowered limit | — | `skip-on` | `skip-on` | no scripts |
| a string doubled from one character | `Stack-Heap Collision` making 32 768 characters (step 15) | making 8 192 (step 13) | an `OutOfHeapException` making 524 288 (step 19), against the 1 MiB heap | never: 4 194 304 characters and on | no scripts |
| a list of integers, ten at a time | `Stack-Heap Collision` making 3 660 | — | never within 20 000 (80 KB of its 1 MiB) | never within 20 000 | no scripts |
| the same under `llSetMemoryLimit(used + 8192)` | at 540 | — | `llSetMemoryLimit` refused: never within 4 000 | the same | no scripts |
| what the owner is told | `Object [script:<name>] Script run-time error`, then `Stack-Heap Collision`, both on `DEBUG_CHANNEL` | the same | to the owner alone: `[YEngine]: Exception while running <item>`, `OutOfHeapException: oldtotal=0, newtotal=524288, limit=1048576`, the prim, the script and the place, `Script must be Reset to re-enable.` and the engine's stack trace | — | no scripts |
| the script afterwards | stopped: `llGetScriptState` `0` | the same | reads as running (`1`), as after any YEngine fault | — | no scripts |

### `llGetObjectDetails` and the region's count

An object of five scripts — two idle Mono ones, one whose limit is
lowered to 16 384 and later raised to 32 768, one that stops itself as it
starts and an idle LSO one — and another object with one Mono script.

| what | Second Life | OpenSim (both engines) | fake grid |
| --- | --- | --- | --- |
| `OBJECT_SCRIPT_MEMORY` | the sum of every script's **limit**, running or not: 65 536 a Mono script (or its lowered limit), 16 384 an LSO one — `229376` for the five; stopping a script changes nothing; raising the limit to 32 768 adds 16 384 | **16 384 per running script**: `65536` for the four running, `49152` with one stopped; the other object `16384` | no scripts |
| `OBJECT_RUNNING_SCRIPT_COUNT`, `OBJECT_TOTAL_SCRIPT_COUNT` | the running ones (`4`, `3` with one stopped), all of them (`5`) | the same | no scripts |
| `OBJECT_SCRIPT_TIME` | a few microseconds | a few milliseconds | no scripts |
| of an avatar (the owner) | its attachments' scripts: five, `327680` | `0`, `0`, `0`: the local avatar wears none | no scripts |
| of `NULL_KEY`, of a key that names nothing | `[]` | `[]` | no scripts |
| the region's active-script count (`SimStats` `ActiveScripts`) | a time average (`2563.4`, `2564.7`, … on Mauve) that rose by the probe's five running scripts and fell by one while `Idle` was stopped | a whole number that counts the scripts in the region's prims, running or not (`6` for the probe's six, the self-stopped one among them, as OpenSim adds one when a script is put in a prim and takes one when it is removed) | `0`: it runs no scripts |

### What the viewer does with it

The region's active-script count is what a statistics floater and the
script-count monitor (`viewer-region-script-count-monitor`) read: on
Second Life a fractional average of the running scripts, on OpenSim the
scripts present, stopped or not. Per-object script memory reaches the
viewer through the top-scripts report and the script-limits floater
(`viewer-script-limits`), not from these calls; their numbers are the
same kind of figure — the limit on Second Life, a flat 16 384 per running
script on OpenSim.

## Script permissions, money and payment

Measured on 2026-10-26 by the probes `money/pay-price`, `money/pay`,
`money/debit` and `money/lapse`, on the same grids, with the two avatars of
every probe run: the owner, which owns the prims, and another, which pays
them and is paid. A payment is what a viewer's pay dialog sends (a
`MoneyTransferRequest` of type `5008` to the object, `drive=pay`); the pay
buttons are what a viewer asks before it shows that dialog
(`RequestPayPrice`, `drive=pay-price`). Balances are noted as the change
each avatar's balance reply shows, never as the balance. Every amount is a
few L$ of aditi's beta currency, and the probes give the other avatar back
what it paid, so a run costs neither account anything. The local OpenSim
runs the stock `BetaGridLikeMoneyModule`, which moves no money
([Money](economy.md)); what a viewer sees of payments without a script is
measured there.

### Pay buttons after `llSetPayPrice`

What `RequestPayPrice` is answered after each call, by a script that has a
`money` event (the other avatar is answered alike). YEngine and XEngine
agree on every row but the last but one.

| the script calls | Second Life | OpenSim | fake grid |
| --- | --- | --- | --- |
| nothing yet | default `-2`, buttons `1, 5, 10, 20` | default `-2`, buttons `-2, -2, -2, -2` | no scripts |
| `llSetPayPrice(PAY_DEFAULT, [PAY_DEFAULT ×4])` | the same as nothing | the same as nothing | no scripts |
| `(25, [1, 5, 10, 20])`, `(PAY_HIDE, [PAY_HIDE ×4])`, `(-5, [-7, -3, -1000, 3])`, `(2000000000, [1000000, 99999, 1, 2])` | as set: no grid checks a price's sign or size | as set | no scripts |
| `(0, [0, 0, 0, 0])` | as set, every amount `0` | **no answer at all**: `LLClientView.SendPayPrice` sends nothing for a layout of zeros | no scripts |
| two buttons, `(7, [1, 2])` | `7`, `1, 2, -1, -1`: the missing buttons hidden | the same — and the **script's own list** has grown to `[1, 2, -1, -1, -1]`: OpenSim pads the argument in place | no scripts |
| six buttons | the first four | the first four | no scripts |
| no buttons, `(7, [])` | `7`, **`1, 5, 10, 20`**: an empty list leaves the buttons at the defaults | `7`, `-1, -1, -1, -1`, and the empty list is now five `-1`s | no scripts |
| buttons of other types, `(7, ["5", 2.5, <1, 2, 3>, 8])` | `7`, `-1, -1, -1, 8`: a button that is not an integer is hidden; the script runs on | the call throws an `InvalidCastException` and the layout stays as it was (`-2` ×5). YEngine tells the owner and stops the script (`Script must be Reset to re-enable.`); XEngine shouts the exception on `DEBUG_CHANNEL` and the event ends | no scripts |
| a prim whose only script has **no** `money` event | answered as that script set it | the same | no scripts |
| set, then `llResetScript` | the layout survives the reset: it is the prim's | the same | no scripts |

OpenSim also ignores `llSetPayPrice` from a script in a child prim (it
returns at once unless the script is in the root, `LSL_Api.llSetPayPrice`);
the probe measured only a root prim.

### Giving money: `llGiveMoney` and `llTransferLindenDollars`

The other avatar touches first, so the script learns its key; the runner
grants `PERMISSION_DEBIT` when asked. `transaction_result`'s data is
printed with each key in it put as a role.

| the script calls | Second Life | OpenSim (both engines) | fake grid |
| --- | --- | --- | --- |
| `llGiveMoney(other, 1)` before any grant | returns `0`; `Script trying to debit L$ from owner but PERMISSION_DEBIT permission not set!` on `DEBUG_CHANNEL` | returns `0`, nothing said | no scripts |
| `llTransferLindenDollars(other, 1)` before any grant | a handle (not `NULL_KEY`); `transaction_result(handle, FALSE, "MISSING_PERMISSION_DEBIT")` | the same | no scripts |
| asking the owner for `PERMISSION_DEBIT` | an ordinary question, mask `2` | the same | the viewer's money-caution card |
| `llGiveMoney(other, 1)`, granted | returns `0`, no `transaction_result`; L$ 1 moves: the owner's balance reply −1, the other's +1, both of type **`5009`** (`TRANS_OBJECT_PAYS`), the object's name as the item, "You paid *other* L$1." and "*owner* paid you L$1." | returns `0`; nothing moves, no balance reply | no scripts |
| `llTransferLindenDollars(other, 1)`, granted | `transaction_result(handle, TRUE, "<other's key>, 1")` and the same two balance replies | `TRUE`, `"<other's key>, 1"` — and nothing moves | no scripts |
| … to the owner itself | `TRUE`, `"<owner>, 1"`; two balance replies to the owner, no change | `TRUE` | no scripts |
| … to `NULL_KEY` | `FALSE`, `INVALID_DESTINATION` | `FALSE`, `LINDENDOLLAR_ENTITYDOESNOTEXIST` | no scripts |
| … to the prim's own key, to a key that names nobody | `FALSE`, `LINDENDOLLAR_ENTITYDOESNOTEXIST`, and a **failed** balance reply to the owner naming that key, "There has been a problem transferring L$. This error has been logged and reported." | `FALSE`, `LINDENDOLLAR_ENTITYDOESNOTEXIST` | no scripts |
| … of L$ 0 or L$ −1 | `FALSE`, `INVALID_AMOUNT` | the same | no scripts |
| … of more than the owner holds (L$ 1 000 000 000) | `FALSE`, `LINDENDOLLAR_INSUFFICIENTFUNDS`, and a failed reply "Insufficient funds." | **`TRUE`**, `"<other's key>, 1000000000"`: the stock module refuses nothing | no scripts |
| `llGiveMoney(other, 0)`, `llGiveMoney(other, -1)` | returns `0`; `Invalid amount for llGiveMoney().` on `DEBUG_CHANNEL` for each | returns `0`, nothing said | no scripts |
| asking the **other** avatar for `PERMISSION_DEBIT` | no question; `Script asking non-owner for run time permissions only owner can grant` on `DEBUG_CHANNEL`, no `run_time_permissions`, the owner's grant kept | **put to the other avatar like any question**; granted, the script holds `DEBIT` with the other avatar as the granter | no scripts |

A balance reply comes on its own time, often after the
`transaction_result` and sometimes after the script's next call, so a
transcript's balance notes trail the step that caused them.

### Paying a scripted object

Prim A holds two scripts with a `money` event, Till and Second till; the
runner pays it as a viewer does. A payment the grid takes is told to both
avatars as for a gift, but of type `5008`, the object's name as the item
([Money § Paying an object](economy.md#paying-an-object)).

| the payment | Second Life | OpenSim (both engines) | fake grid |
| --- | --- | --- | --- |
| the owner, L$ 1 | taken; `money(owner, 1)` in **both** scripts; two balance replies to the owner, no change | nothing: no `money` event, no balance reply (the stock module drops every payment) | no scripts |
| the other avatar, L$ 1 | taken, the other −1 and the owner +1; `money(other, 1)` in both scripts, in either order | nothing | no scripts |
| L$ 0 | nothing at all | nothing | no scripts |
| L$ 7 under price `5`, buttons `1, 2, 3, 4` | taken: a shown price field takes any amount | nothing | no scripts |
| L$ 7 or L$ 5 under price `5`, every button hidden | both taken | nothing | no scripts |
| L$ 2 under price `PAY_HIDE`, buttons `1, 2, 3, 4` | taken: the amount is a button's | nothing | no scripts |
| L$ 7 under the same | **refused silently**: no event, no reply, no alert | nothing | no scripts |
| L$ 1 when the price and every button are hidden | refused: the payer is alerted "Payment stopped:  the price paid does not match any of the pay buttons set for this object" | nothing | no scripts |
| L$ 1 while Till is in a state without a `money` event | taken; only Second till's `money` runs | nothing | no scripts |
| L$ 1 to the prim once no script in it has a `money` event (Till in that state, Second till removed) | taken all the same | nothing | no scripts |

So Second Life checks the amount only when the price field is hidden: then
it must be one of the buttons, and a payment with no button to match is
dropped without a word — unless every amount is hidden, which is refused
with the alert. It does not check against a shown price at all, and takes a
payment to an object none of whose scripts can hear it. The probe gives the
other avatar its L$ 23 back with `llGiveMoney` at the end (type `5009`).

### When a grant lapses

The script holds `TAKE_CONTROLS | TRIGGER_ANIMATION | TRACK_CAMERA | DEBIT`
(`1046`), granted by the owner, before each of these. A revocation is the
`RevokePermissions` a viewer sends (`drive=revoke`): the reference sends
`TRIGGER_ANIMATION | OVERRIDE_ANIMATIONS` of the whole region when its
avatar stops all animations, and every bit of an object when its avatar
stands up from it. YEngine agrees with Second Life on every row.

| after | Second Life | OpenSim YEngine | OpenSim XEngine | fake grid |
| --- | --- | --- | --- | --- |
| a state change and back | all kept | the same | the same | no scripts |
| the viewer revoking `TRIGGER_ANIMATION` of the object | that bit gone (`1030`), the rest and the granter kept; **no** `run_time_permissions` | the same | the same | no scripts (the grid surfaces `ServerEvent::RevokeScriptPermissions`) |
| … `TRIGGER_ANIMATION \| OVERRIDE_ANIMATIONS` of the region | the same: `1030` | the same | the same | no scripts |
| … every bit (`0xFFFFFFFF`) of the object | nothing held, the granter `NULL_KEY`; no event | the same | the same | no scripts |
| `llTakeControls`, then `llReleaseControls` | `TAKE_CONTROLS` gone (`1042`) | the same | the same | no scripts |
| the owner in another region for 20 s | all kept, while it is gone and after | the same | the same | no scripts |
| `llResetScript` | nothing held | the same | the same | no scripts |
| the owner taking the prim and rezzing it again | all kept: the script carries on and runs `on_rez` | the same | **the script is reset**: `state_entry` again, nothing held | no scripts |
| the owner attaching it from the ground | all kept; asking again while worn puts the question to the wearer as before | the same | the same | no scripts |
| the owner dropping it | `TAKE_CONTROLS` gone (`1042`), the rest kept | the same (by the time the drop is reported, or within three seconds of it) | the same | no scripts |

No grid ran `run_time_permissions` for anything but an answer: a lapse is
silent, and a script learns of it only by asking `llGetPermissions`.

### What the viewer does with it

A `PERMISSION_DEBIT` question raises the viewer's money-caution card (the
reference's `ScriptQuestionCaution`), and balance replies of type `5009`
raise the same payment notices as any other transaction
([Money § The viewer](economy.md#the-viewer)). The viewer has no pay dialog
yet, so nothing of it sends `RequestPayPrice` or a payment to an object
(`viewer-money-economy-ui`), nor anything that sends `RevokePermissions`.

## OSSL

Measured on 2026-10-26 by the probes `ossl/available` and `ossl/errors`, on
the same grids. OSSL (the `os*` functions) and its two sibling families —
LightShare (`ls*`) and MOD (`mod*`) — are OpenSim's own; Second Life has
none of them. On OpenSim a script may call an OSSL function when the
region's `[OSSL]` configuration allows it for the script's owner. The local
grid runs the stock `config-include/osslDefaultEnable.ini`:
`AllowOSFunctions`, `AllowMODFunctions` and `AllowLightShareFunctions` on,
`OSFunctionThreatLevel = VeryLow`, `PermissionErrorToOwner = false`, and an
`Allow_<function>` line for most functions, which overrides the threat level
in either direction: `true` for everyone, `false` for no one, or a list of
who may (`ESTATE_OWNER`, `ESTATE_MANAGER`, parcel owners and group members,
gods, avatar or group keys). The probes' owner is an ordinary account,
neither estate owner nor manager.

### What runs

`ossl/available` calls one or more functions of each way a function comes to
be allowed, and prints what each answered. The two engines answer alike but
for `osGetScriptEngineName`.

| allowed because | the calls and what OpenSim answered | Second Life | fake grid |
| --- | --- | --- | --- |
| its `Allow_` line is `true`, most of them above `VeryLow` | `osGetScriptEngineName` `YEngine` or `XEngine`; `osGetSimulatorVersion` the region's `sim_version`; `osGetPhysicsEngineType` `ubODE`; `osGetGridName` and `osGetGridNick` the stock `the lost continent of hippo` and `hippogrid`; `osFormatString("{0}-{1}-{0}", ["a", 2])` `a-2-a`; `osMatchString("abcabc", "b", 0)` `b, 1, b, 4`; `osReplaceString("abcabc", "b", "x", -1, 0)` `axcaxc`; `osRegexIsMatch("abc", "^a.c$")` `1`; `osUnixTimeToTimestamp(0)` `1970-01-01T00:00:00.0000000Z`; `osGetWindParam` `1.0`; `osListenRegex` a listen handle | none of it compiles: `(76, 35) : ERROR : Name not defined within scope` | no scripts |
| … `true`, with a second check of its own | `osGetAgentIP(owner)`: `""`, no error — the address is for gods only | — | no scripts |
| no `Allow_` line, threat level `None`, under the configured `VeryLow` | `osGetGender(owner)` `female`; `osGetHealth` `100.0`; `osGetRezzingObject` `NULL_KEY`; `osWindActiveModelPluginName` `SimpleRandomWind`; `osDrawText("", "hi")` `Text hi;` and a trailing space | — | no scripts |
| it checks only that OSSL is on | `osGetRegionSize` `<256, 256, 0>`; `osIsNpc(owner)` `0`; `osIsUUID`; `osMin`, `osMax`; `osGetSunParam("day_length")` `14400.0`; `osGetLinkNumber` of the prim's own name in an unlinked prim `-1`; `osGetMapTexture` a key; `osGetPhysicsEngineName` `ubODE 1.0-0.13.2`; `osCheckODE` `0`; `osStringSubString("abcdef", 2, 3)` `cde` | — | no scripts |
| it checks nothing | `osSHA256("abc")` the SHA-256 in lower-case hex; `osRound(1.23456, 2)` `1.23`; `osVecMagSquare`, `osApproxEquals`, `osStringReplace`, `osIsNotValidNumber`; `osGetPSTWallclock`; `osGetInventoryItemKey` of an item the prim lacks `NULL_KEY` | — | no scripts |

Second Life reports one compile error per script, and not the first in the
source: `ossl/available` calls `osGetScriptEngineName` first, yet the error
is the constant `OS_LISTEN_REGEX_MESSAGE` two dozen lines further on,
pointing as ever at the token after it: an undefined constant is found ahead
of the undefined calls. In `ossl/errors`' first script, which uses no
constant, it is the inner of two nested calls, `osGetTerrainHeight(128, 128)`
inside `osTerrainSetHeight(…)`, at the inner call's closing parenthesis. The
text is the one any undefined name gets ([Compiling](#compiling)).

### Denials and errors

`ossl/errors` puts each call that may throw in a script of its own in prim
A, has them called one at a time, then reads every script's
`llGetScriptState`, pings them all with a link message, resets the first and
has it call again. Two listeners in other objects, 10 m and 50 m away, listen
on `DEBUG_CHANNEL`.

| the call | what is said (both engines) | Second Life | fake grid |
| --- | --- | --- | --- |
| a threat level above the configured one, no `Allow_` line: `osTerrainSetHeight` (`High`) | `OSSL Permission Error: osTerrainSetHeight permission denied.  Allowed threat level is VeryLow but function threat level is High.` (two spaces after the first full stop) | the probe stops here, at this script's compile error: `(39, 81) : ERROR : Name not defined within scope` | no scripts |
| an `Allow_` list the owner is not on, threat level `None`: `osGetAgents` | `OSSL Permission Error: osGetAgents permission denied.` — beneath the threat level, the list alone decides | — | no scripts |
| `Allow_` `false`: `osAvatarStopAnimation` | `OSSL Permission Error: osAvatarStopAnimation disabled in region configuration` | — | no scripts |
| an argument error: `osGetTerrainHeight(-1, -1)` | `OSSL Runtime Error: osGetTerrainHeight: Coordinate out of bounds` | — | no scripts |
| a deprecated function: `osSunGetParam` | shouted on `DEBUG_CHANNEL`: `Use of function osSunGetParam is deprecated. Use osGetSunParam instead.`; the call returns its value (`14400.0`) a second later: the warning costs the script a one-second sleep | — | no scripts |
| a pattern that is no regular expression: `osRegexIsMatch("a", "(")` | shouted: `Possible invalid regular expression detected.`; returns `0`, a second later | — | no scripts |
| `lsGetWindlightScene([WL_WATER_COLOR])` | nothing; `[0, <4, 38, 64>]` (the rule, then the region's water colour) | — | no scripts |
| `modSendCommand` to a module no region has | nothing; a fresh request key, though no module answers | — | no scripts |

A **permission or run-time error** is thrown: the event ends at the call
(its `returned` line never prints) and the engine reports it as a script
error, on `DEBUG_CHANNEL` to **both** avatars near it, as
`PermissionErrorToOwner = false` asks — no script hears it, at 10 m or at
50 m. Each engine wraps the text in its own way:

| | OpenSim YEngine | OpenSim XEngine |
| --- | --- | --- |
| the message | `YEngine: <text> (script: Threat event: link_message primID: <key> at: <120,128,27>) Script must be Reset to re-enable.` and a newline | `<text>(script: Threat event: link_message primID:<key> at <120, 128, 27.5>)` — no space before the bracket, the position as a vector |
| the script afterwards | reads as running (`llGetScriptState` `1`) but answers nothing, as after any YEngine fault | runs on: the next link message is answered |
| after `llResetOtherScript` | answers again; the same call is denied again, with the same message | the same |

A **warning or a shouted error** is not thrown: the call returns, and the
message is a shout on `DEBUG_CHANNEL`, from the object's name, that both
avatars and both listening scripts hear.

With `PermissionErrorToOwner = true` the source (`OSSL_Api.CheckThreatLevel`)
prefixes `(OWNER)` to a permission error and both engines send it to the
owner alone; the grid here does not run that way. The source also reads
`ESTATE_MANAGER` as a manager who is **not** the estate owner, and
`ESTATE_OWNER` as the owner alone, which is why the stock lines name both.

### The syntax document

OpenSim serves its `bin/ScriptSyntax.xml` as the `LSLSyntax` capability
(`[SimulatorFeatures] ScriptSyntax`, on by default), with the `LSLSyntaxId`
in the simulator features. The document the local grid served lists 224
`os*` functions and the four `ls*` ones, with their arguments and return
types and nothing about who may call them: `osAvatarStopAnimation` and
`osConsoleCommand` (disabled here) and `osTerrainSetHeight` (denied here)
read like any other. It leaves out functions that compile and run:
`osListenRegex`, which `ossl/available` called, and every `mod*` function.
Second Life's lists no OpenSim function.

### What the viewer does with it

Every message in the tables above reaches the viewer as `DEBUG_CHANNEL` chat
(`ChatType::DebugChannel`), which the viewer's Script Errors window gathers
per object ([Run-time errors](#run-time-errors)). OpenSim sends a denial to
everyone near the object, and the window shows another resident's object's
only when it is set to show others' lines too. A script's compile errors on
Second Life are the editor's usual error list. The script editor has no
highlighting yet (`viewer-lsl-editor-highlight`): when it takes its keywords
from the grid's `LSLSyntax` document, OSSL is highlighted on OpenSim with no
code of its own — denied functions included, and `osListenRegex` and
`modSendCommand` left out, since that is what the document says.

## Persistence

Measured on 2026-10-26 by the probes `persistence/rerez` and
`persistence/restart`, on the same grids. Each probe's script sets every
global to a value no initialiser gives it, goes into a state of its own
(`kept`), starts a one-second timer and a listen, is granted
`PERMISSION_TRIGGER_ANIMATION` and `PERMISSION_TRACK_CAMERA`, stops a second
script in its prim, resets its script time, and prints what it holds. Then
it asks for the take or the restart, sends itself three link messages and
sleeps, and prints what it holds again on `on_rez` or `CHANGED_REGION_START`.
A take and a rez is the runner's `rerez` (into the owner's Trash and back
where the prim stood). A restart is the runner's `restart`: the estate owner,
logged in for that alone, asks for one in 15 seconds (`EstateOwnerMessage`
`restart`), and the owner and the other avatar are logged in again once the
region takes a login. The probe's lines wait in a list while its owner is
not in the region, since an owner-say to an absent owner is lost.

### Taken and rezzed again

| what the script had | Second Life | OpenSim | fake grid |
| --- | --- | --- | --- |
| its globals and its state | kept: still in `kept`, every value as set | YEngine the same; **XEngine resets the script**: `state_entry` in `default`, every global at its default | no scripts |
| its permissions and their key | kept: 1040, the owner's | YEngine the same; XEngine none (0) | no scripts |
| its timer and its listen | kept: the timer ticks on, and the line on 7 is heard | YEngine the same; XEngine has only what its reset opened again | no scripts |
| three link messages to itself, queued before the take | delivered, **after** `on_rez` | YEngine delivered, **before** `on_rez`; XEngine lost with the reset | no scripts |
| an 8 s `llSleep` the take came in | runs its full 8 s, script time and clock alike: the handler finishes after the rez, with the prim's new key | YEngine **ends at the take**: the handler goes on once the prim is rezzed, 1.2 s of script time and 2 s by the clock after the sleep began; XEngine lost: the handler never finishes | no scripts |
| its script time (`llGetTime`) | counts on through the take and the rez | YEngine counts on; XEngine starts again with the reset | no scripts |
| a second script it had stopped (`llSetScriptState(…, FALSE)`) | stays stopped: no `on_rez`, no answer to a link message, `llGetScriptState` 0 | YEngine the same; **XEngine runs it again**: its `state_entry` and `on_rez` run, it answers, `llGetScriptState` 1 | no scripts |
| what runs on the rez | the sleeping handler's end, `on_rez(0)`, the queued link messages | YEngine the sleeping handler's end, the link messages, `on_rez(0)`; XEngine the stopped script's `state_entry` and `on_rez`, then its own `state_entry` and `on_rez(0)` | no scripts |

`llGetStartParameter` is 0 on every grid, as for any rez from inventory, and
no grid sends a `changed`. The prim's description (where the probe marks its
first start) is kept on all three.

### A restart of the region

Not measured on Second Life: neither test avatar holds estate rights on aditi
(`gridspec-aditi-test-land`), so the runner asks for no restart there and the
probe waits its patience out. On OpenSim each avatar in the region is told
`/Region will restart in 15 seconds`, an agent alert, then kicked with `The
simulator is going down.`, as [Estate](estate.md) measured too. The region
was taking logins again four seconds after it went down. A login meanwhile
does not fail: OpenSim cannot find the region, falls back to its defaults and
puts the avatar in a neighbour (East Region here), which the runner logs out
and tries again.

| what the script had | Second Life | OpenSim YEngine | OpenSim XEngine | fake grid |
| --- | --- | --- | --- | --- |
| its key, globals and state | not measured | kept: the same key, still in `kept`, every value as set | **kept** too — unlike a take and a rez, no reset | no scripts |
| its permissions and their key | not measured | kept | kept | no scripts |
| its timer and its listen | not measured | kept | kept | no scripts |
| three link messages to itself, queued before the restart | not measured | delivered, first | delivered, first | no scripts |
| a 60 s `llSleep` the restart came in | not measured (it ran its full 60 s on aditi, where nothing restarted) | **ends at the shutdown**: the handler goes on once the region is back, 18 s of script time and 20 s by the clock after the sleep began | lost: the handler never finishes, though the events queued behind it run | no scripts |
| its script time | not measured | counts on | starts again from 0 | no scripts |
| a second script it had stopped | not measured | stays stopped | **runs again**, and answers | no scripts |
| what runs when the region is back | not measured | the sleeping handler's end, the link messages, `changed(32768)` — OpenSim's own `POSITION` bit — then `changed(1024)`, `CHANGED_REGION_START` | the link messages, `changed(32768)`, `changed(1024)` | no scripts |

Neither engine runs `state_entry` or `on_rez` after a restart: a script learns
of it from `CHANGED_REGION_START` alone. The prims come back with their keys
(`back prims=all`).

### What the viewer does with it

Nothing of a script's state reaches the viewer. A take and a rez are the
viewer's own messages (`DeRezObject`, `RezObject`), sent by the runner with
our own client on every grid. Of a restart the viewer sees the notice and the
kick: the notice opens the restart countdown card in either grid's form
(`e2e_windows`' `a_restart_notice_counts_down_and_a_cancellation_closes_it`,
against both fake flavours), and the kick, `Disconnected(Kicked)` with
`The simulator is going down.`, ends the session as any disconnect does
today; the reference's disconnect dialog, and a login again that does not
land in a neighbour while the region is still down, are
`viewer-disconnect-screen`.

## Status flags, forces, targets and vehicles

Measured on 2026-10-27 by the seven `physics/*` probes, on the same grids:
aditi's Mauve, and the local OpenSim, whose physics engine is ubODE, under
YEngine and XEngine. Each works on a 0.5 m cube that is not physical, makes
it physical only for a short leg, and puts it back where it stood after. What
a grid says about a call is its debug-channel chat, which the runner notes
beside the `try` record the probe prints before the call. Velocities, times
and distances are measurements and differ from run to run; the tables give
what a grid does with them. Where the two engines agree, OpenSim is one
column: the physics is the region's, not the engine's.

### Status flags

`physics/status`: every flag of a fresh prim, masks that are not one flag,
each flag set and cleared on its own, and physics last.

| what | Second Life | OpenSim | fake grid |
| --- | --- | --- | --- |
| a fresh prim | `STATUS_ROTATE_X`, `_Y` and `_Z` 1, every other flag 0 | the same | no scripts |
| `llGetStatus` of several flags at once | TRUE if **any** of them is set: `ROTATE_X \| ROTATE_Y \| ROTATE_Z`, `PHYSICS \| ROTATE_X` and `PHANTOM \| ROTATE_X` all read 1 | **0** for any mask that is not exactly one flag | no scripts |
| `llGetStatus` of 0, 2048 (no flag) and −1 | 0 | 0 | no scripts |
| each of the rotation flags, `PHANTOM`, `SANDBOX`, `BLOCK_GRAB`, `DIE_AT_EDGE`, `RETURN_AT_EDGE` and `BLOCK_GRAB_OBJECT`, set and cleared | reads back as set; no other flag moves | the same | no scripts |
| `STATUS_CAST_SHADOWS` | cannot be set: reads 0 after TRUE | **set by either value**, TRUE or FALSE, and never cleared again | no scripts |
| a value of 2 or −1 | TRUE | TRUE | no scripts |
| two flags in one call | both set | both set | no scripts |
| `PRIM_PHYSICS`, `PRIM_PHANTOM` | follow the flags; `PRIM_TEMP_ON_REZ` stays 0 | the same | no scripts |
| `llVolumeDetect(TRUE)` | reads as phantom (`STATUS_PHANTOM` and `PRIM_PHANTOM` 1), which `llSetStatus(STATUS_PHANTOM, FALSE)` does not clear; `llVolumeDetect(FALSE)` clears it, also on a prim that was phantom before | the same | no scripts |
| physics on; a rotation flag cleared while physical; physics with phantom; physics under volume detect | each reads as set, the last as physical and phantom | the same | no scripts |

### Physics material and mass

`physics/material`: `llSetPhysicsMaterial` term by term, inside and outside
the documented ranges, with the mass after each, then the mass at other
scales and physics shapes.

| what | Second Life | OpenSim | fake grid |
| --- | --- | --- | --- |
| a fresh prim's `llGetPhysicsMaterial` | `[1.0, 0.5, 0.6, 1000.0]`: gravity multiplier, restitution, friction, density | the same | no scripts |
| values in range: gravity −1 to 28, restitution 0 to 1, friction 0 to 255, density 1 to 22587 | set as given | the same | no scripts |
| a value out of range: gravity −2 or 40, restitution −1 or 2, friction −1 or 300, density −5, 0 or 30000 | refused, nothing changes: `ERROR: Failed to set material properties.` | refused **silently**, nothing changes | no scripts |
| no bits, all four bits | nothing changes; all four set | the same | no scripts |
| a fresh 0.5 m cube's `llGetMass`, `llGetMassMKS`, `llGetObjectMass` | 1.25, 125, 1.25: `llGetMass` is the kilograms over 100 | the same | no scripts |
| the mass after a change of density | follows it: 0.00125 at 1, 28.23 at 22587, 0.625 at 500 | **stays 1.25** | no scripts |
| the mass after `llSetScale`, read in the same event | 10 at 1 m, and still 10 at 2 m and at 4 × 0.5 × 0.5 m, and for two calls after the prim is scaled back to 0.5 m: it follows a scale late | stays 1.25 | no scripts |
| `llGetCenterOfMass` | the prim's position | the same | no scripts |
| `PRIM_PHYSICS_SHAPE_TYPE` on the root | convex and prim are set; none is refused (the type stays prim) | the same | no scripts |

### Forces, velocities and impulses

`physics/forces`: on a prim that is not physical, then for two seconds
physical and weightless (`llSetBuoyancy(1.0)`).

| what | Second Life | OpenSim | fake grid |
| --- | --- | --- | --- |
| `llSetForce` and `llSetTorque` on a prim that is not physical | read back by `llGetForce` and `llGetTorque` as set, a million newtons too: no clamp | the same | no scripts |
| a local force or torque, `<1,0,0>`, on a prim turned a quarter round Z | read back **as given**, in the prim's frame | read back **rotated into the region's**, `<0,1,0>` | no scripts |
| `llSetVelocity`, `llSetAngularVelocity`, `llApplyImpulse`, `llApplyRotationalImpulse` on a prim that is not physical | nothing: `llGetVel`, `llGetOmega`, `llGetAccel` read 0 | the same | no scripts |
| `llSetBuoyancy`, `llSetHoverHeight`, `llGroundRepel`, `llPushObject` of itself, not physical | it does not move | the same | no scripts |
| turning physical, weightless, after those impulses and the push | drifts up at about 1 m/s, as it does every time it turns physical weightless here: the probe does not separate why | **shoots up at 4.6 m/s**: the impulse (1) and the push (5) it was given while not physical are applied now, (1 + 5) / 1.25 kg | no scripts |
| turning physical weightless again | drifts up as before | stays still | no scripts |
| a force set before physics | kept in `llGetForce` | kept in `llGetForce`, but the prim does not move | no scripts |
| `llSetVelocity`, read back in the same event | not yet: `llGetVel` still has the old velocity | the same | no scripts |
| `llSetVelocity(<0,0,200>)` stopped in the same event | not seen, and the prim does not fly off | the same | no scripts |

### Targets, moving to them and looking at them

`physics/targets`: each phase runs a second and counts the events. A target
at the prim and one 10 m above it, ten at once, ranges of 0 and −1, the same
for rotation targets, the movers on a prim that is not physical, then
physical and weightless.

| what | Second Life | OpenSim | fake grid |
| --- | --- | --- | --- |
| a script's first position targets | positive handles, one apart | positive handles from one counter of the whole scene, which other ids share: one apart, in either direction | no scripts |
| a target in range on a prim that is not physical | `at_target` every other frame or so (21 a second), with the handle, the target as given and the prim's position | the same arguments, 11 a second | no scripts |
| a second target out of range | raises nothing, and `not_at_target` does not run while another target is reached | YEngine the same; **XEngine runs `not_at_target` too**, as often as `at_target` | no scripts |
| ten targets | only the newest eight raise events: a script keeps eight | the same | no scripts |
| a range of 0, at the target | **never reached** | reached: the distance squared is compared with the range squared, `<=` | no scripts |
| a range of −1 | reached: the range is squared | reached | no scripts |
| rotation targets | handles from the same counter as position targets (the next one); eight kept, the same arguments | handles from the scene's counter (one or two on); eight kept | no scripts |
| `llMoveToTarget` on a prim that is not physical | it does not move | the same | no scripts |
| `llLookAt`, `llRotLookAt` on a prim that is not physical | **turn it** | the same | no scripts |
| `llMoveToTarget` a metre up with a tau of 0.3, physical | reaches a 0.1 m target in 0.69 s | 0.71 s | no scripts |
| a target 70 m up (the documented limit is 65 m) | it does not move | **it moves**: 27 m in half a second | no scripts |
| a tau of 0 | it does not move | the same | no scripts |
| `llRotLookAt` a quarter round, physical | reaches a 0.1 rad rotation target in 1.36 s | 0.87 s | no scripts |

### Keyframed motion

`physics/keyframed`: each try is one `llSetKeyframedMotion` and, a second
and a half later, where the prim is and which `moving_start` and
`moving_end` it ran. Two tries that OpenSim throws on are scripts of their
own in prims B and C. A valid frame here is half a metre up in a second.

| what | Second Life | OpenSim | fake grid |
| --- | --- | --- | --- |
| a prim of the default physics shape (prim) | refused: `Error, failed to play animation. Does your linkset exceed the complexity limit for keyframing?` | plays | no scripts |
| a valid frame, once the shape is convex | plays, `moving_start` then `moving_end`; the first motion after the shape changed started late (0.44–0.46 m of the 0.5 at 1.5 s, no `moving_end` yet) | plays, exactly 0.5 m | no scripts |
| a frame time under 0.1 s (0.05), or negative | refused: `Delta times must be >0.1s` | plays it as given | no scripts |
| an integer time | accepted | accepted | no scripts |
| a list one element short | refused: `Expected delta time in keyframe list.` | nothing, silently: the incomplete frame is dropped | no scripts |
| rotation and translation in each other's places | refused: `Error, expected translation vector in keyframe list.` | **throws**: YEngine stops the script (`InvalidCastException: Vector3 expected but Quaternion given`, to the owner), XEngine ends the event with the exception on the debug channel | no scripts |
| three-element frames under `KFM_DATA, KFM_TRANSLATION` | refused: `Expected delta time in keyframe list.` | throws: `Specified cast is not valid.` | no scripts |
| two frames | relative to each other: they add up to 0.5 m | the same | no scripts |
| translation only, rotation only, both in one frame | play | the same | no scripts |
| an unknown `KFM_MODE` (7) | refused: `Unknown playback mode.` | plays forward | no scripts |
| an unknown option (99) | refused: `Unknown parameter type.` | ignored; plays | no scripts |
| `KFM_REVERSE` | plays the frame backwards: ends 0.5 m below | the same | no scripts |
| 20 m in 0.2 s | plays: no speed limit | the same | no scripts |
| a loop paused, played, stopped | the pause holds it, play goes on, stop leaves it where it is (not back at the start) | the same | no scripts |
| `llSetPos` during a motion | refused: `Cannot use a script to move a linkset while it is playing an animation. Stop the animation first.` | **moves it**, and the motion goes on from there | no scripts |
| physics on during a motion | refused: `Can't enable physics for keyframed objects.` | **turns physical** and falls | no scripts |
| a motion on a physical prim | nothing, silently (a `moving_end`) | nothing, silently | no scripts |
| `KFM_CMD_PLAY` with nothing playing | plays the last motion given, here the one given while physical | nothing | no scripts |
| an empty call with nothing playing | a `moving_start` and a `moving_end`, no motion | nothing | no scripts |

### Vehicles

`physics/vehicles`: no function reads a vehicle parameter back, so a setter
answers with its debug-channel chat and with what the prim does.

| what | Second Life | OpenSim | fake grid |
| --- | --- | --- | --- |
| a parameter or flag set before any vehicle type | silent | silent | no scripts |
| each type, 0, 99 and −1 on a prim that is not physical | silent; no status flag moves | the same | no scripts |
| an unknown parameter code (999) | `Unknown vehicle float parameter 999`, and `vector`, `rotation` for the other setters | silent | no scripts |
| a float parameter through the vector setter | `Unknown vehicle vector parameter 24` | silent | no scripts |
| timescales and efficiencies out of range, unknown flag bits | silent | silent | no scripts |
| a sled whose linear motor pushes it east at 1 m/s | goes east, 1.7 m in 1.5 s | goes east, 0.9 m | no scripts |
| `VEHICLE_TYPE_NONE` then | stops at once | still moving at 6 m/s half a second later | no scripts |
| a linear motor of 100 m/s up for half a second | 98 m/s: no clamp at the documented 30 | 97 m/s | no scripts |
| an angular motor of 100 rad/s about Z | 12.4 rad/s after a second, under the documented 4π clamp | 12.3 rad/s | no scripts |

### Pathfinding characters

`physics/character`. OpenSim has none of these functions, and the script
does not compile there: XEngine stops at the `path_update` event, a `syntax
error`, and YEngine names four undefined names, all in the last handler
(`CHARACTER_CMD_STOP`, `llExecCharacterCmd`, `llDeleteCharacter` twice), not
the ones before it.

| what | Second Life | fake grid |
| --- | --- | --- |
| `llGetClosestNavPoint` of the prim, a metre above the reference prim | one point, 2.2 m away | no scripts |
| `llGetStaticPath` two metres east from there | `[3]`: a status alone, `PU_FAILURE_INVALID_GOAL` | no scripts |
| `llNavigateTo` with no character | `You must call llCreateCharacter before llNavigateTo.` | no scripts |
| `llCreateCharacter` | the prim turns physical, `OBJECT_PATHFINDING_TYPE` 2 (`OPT_CHARACTER`); the rotation flags stay set | no scripts |
| `CHARACTER_DESIRED_SPEED` 100 | `Error: CHARACTER_DESIRED_SPEED 100 larger than max 40` | no scripts |
| `llNavigateTo` the nearest navmesh point two metres east | `path_update` 0 (`PU_SLOWDOWN_DISTANCE_REACHED`), then 1 (`PU_GOAL_REACHED`), each with an empty list; the prim moved east | no scripts |
| `llExecCharacterCmd` with a command no grid has (999) | `Unrecognized character command.` | no scripts |
| `llDeleteCharacter`, twice | not physical, pathfinding type 0; the second is silent | no scripts |

### What the viewer does with it

Nothing reads these calls. What a viewer sees is the prim's object updates —
its position, rotation and velocities, its physics and phantom flags — the
same updates any moving prim sends, which it already renders. A keyframed
motion is no message of its own: the grid moves the object and sends the
updates.

## Reading and writing the prim

Measured on 2026-10-27 by the eight `prim/*` probes, on aditi's Mauve and
the local OpenSim under YEngine and XEngine. Each works on the 0.5 m cube the
runner rezzes, which is not physical; a probe that moves it puts it back. What
a grid says about a call is its debug-channel chat, which the runner notes
beside the `try` record the probe prints before the call. Where the two
engines agree, OpenSim is one column: the prim is the region's, not the
engine's. OpenSim stores a colour, an alpha and a glow as a byte, so a value
it reads back is the nearest 1/255 (0.5 reads 0.498039); Second Life reads
back what was set.

### Placement

`prim/placement`: each move starts where the prim was rezzed and is read as
an offset from there.

| what | Second Life | OpenSim | fake grid |
| --- | --- | --- | --- |
| `llSetPos` 5 and 10 m | moves there | the same | no scripts |
| `llSetPos` 15 or 50 m east, 15 or 100 m up, past the region's east edge, past 4096 m | **10 m** towards the target, in one call | the same | no scripts |
| `llSetPos` or `llSetRegionPos` 20 m down, below the ground | stops at the ground (here 2.1 m below the prim) | the same | no scripts |
| `PRIM_POS_LOCAL` of a root prim | the region position, like `llSetPos` | the same | no scripts |
| `llSetRegionPos` 50 m east, 100 m up, to 4000 m | moves there in one call, answers TRUE | the same | no scripts |
| `llSetRegionPos` into the ground | stops at the ground and answers **FALSE** | the same | no scripts |
| `llSetRegionPos` to 5000 m | does not move, FALSE | the same | no scripts |
| `llSetRegionPos` past the west edge, no neighbour there | moves the object off the region and **returns it to its owner's inventory** (seen once, 2026-10-27; the probe leaves it out since) | FALSE, it does not move (OpenSim's code refuses any position outside the region) | no scripts |
| `llSetRot` of a rotation that is not normalised, `<0,0,2,2>`, and `PRIM_ROTATION` of it | normalised: `<0,0,0.7071,0.7071>` | the same | no scripts |
| `llSetRot` of `<0,0,0,0>` | the identity | the same | no scripts |
| `llSetRot` of `<0,0,0,−1>` | read back as `<0,0,0,−1>`, not flipped | the same | no scripts |
| `llSetLocalRot` of a root prim | the region rotation | the same | no scripts |
| `llSetScale` and `PRIM_SIZE` under 0.01 (0.005) | **0.01** | **0.005**: kept | no scripts |
| a negative or zero axis | 0.01 | 0.001 | no scripts |
| past 64 m (65) | **64** | kept | no scripts |
| past 256 m (300) | 64 | **256** | no scripts |
| `llGetBoundingBox` in the event that set the scale | **the box of an earlier scale**: it follows a scale late | the new box | no scripts |
| `llGetBoundingBox` a second later; `llGetGeometricCenter` | the half sizes of the new scale, in the prim's frame (a quarter turn does not change it); the centre 0 | the same | no scripts |
| `llTargetOmega(<0,0,1>, 2, 1)` | `PRIM_OMEGA` reads it back; `llGetRot` does not move (the spin is the viewer's) | the same | no scripts |
| `PRIM_OMEGA` with a gain of 0.5 | read back 0.5 | read back **1.0** | no scripts |

### `PRIM_*` round trips

`prim/params`: each rule set, then read back with the same rule. The
newer rules go by number, since OpenSim has no constants for them.

| what | Second Life | OpenSim | fake grid |
| --- | --- | --- | --- |
| a fresh prim's `PRIM_MATERIAL` | 3 (wood) | the same | no scripts |
| a fresh prim's `PRIM_POINT_LIGHT` intensity, `PRIM_TEXT` alpha | 0 and 1 | **1 and 0** | no scripts |
| a fresh prim's `PRIM_ALLOW_UNSIT`, `PRIM_SCRIPTED_SIT_ONLY`, `PRIM_CLICK_ACTION`, `PRIM_REFLECTION_PROBE`, `PRIM_RENDER_MATERIAL`, `PRIM_SIT_FLAGS` | 1, 0, 0, off, `NULL_KEY`, 2 | nothing: each is `unsupported parameter` on the debug channel, set or get | no scripts |
| `PRIM_PROJECTOR` (42), `PRIM_CAST_SHADOWS` (24), get | nothing, `llGetPrimitiveParams error: Param 0 error, unknown parameter` | nothing, silently; a set of 24 is `unsupported parameter` under YEngine and **never returns under XEngine** (left out there) | no scripts |
| hollow 0.99 | **0.95** | kept, 0.99 | no scripts |
| cut, twist, taper and shear past their ranges | clamped to `[0,1]`, `[−1,1]`, `[0,2]` and `[−0.5,0.5]` | the same | no scripts |
| a cut whose begin is past its end (0.8, 0.2) | the begin pulled to 0.02 below the end: `<0.18, 0.2>` | the same | no scripts |
| a hole shape that does not exist (`0x40`) | kept as the default, with `warning running rule #1 (PRIM_TYPE_BOX): Invalid hole type.` | the default, silently | no scripts |
| a torus's revolutions 0, radius offset 2 | revolutions 1; radius offset **0.666667** | revolutions 1; radius offset 1 | no scripts |
| a torus's hole size `<2, 0.01>`, top shear 1, taper `<2, 2>`, radius offset 5, skew 2 | hole **`<1, 0.05>`**, shear 0.5, taper `<1, 1>`, radius offset 1, skew 0.95 | hole `<1, 0.01>`; the rest the same | no scripts |
| a tube's or ring's radius offset 1 | read back **0** | read back 1 | no scripts |
| a sculpt type no grid has (6) | kept, 6 | 7 (mesh) | no scripts |
| a sculpt map that is not a key, a prim type that does not exist (9) | nothing changes; the second says `error running rule #1: unknown rule.` | nothing changes; the second says `Error running rule #2: arg #0 - unsupported parameter` | no scripts |
| colour and alpha past their ranges | clamped to `[0,1]` | the same | no scripts |
| texture offsets past ±1, rotation past 2π | kept as given (3, 7) | clamped to ±1, the rotation taken modulo 2π | no scripts |
| texture repeats of ±200 and 0 | kept | the same | no scripts |
| a texture `NULL_KEY` | **applied**, silently (unlike `llSetTexture`, below) | not applied, silently | no scripts |
| a texture name not in the inventory | not applied, silently | the same | no scripts |
| a texture key no asset has | applied | the same | no scripts |
| shiny 7, bump 40 | refused, `bounds error; 7 is not in (0, 3).` | **set to 0 and 8** | no scripts |
| fullbright 2, glow 2 or −1, texgen 5, material 8 or −1 | refused, each with a `bounds error` line naming the range | fullbright TRUE, glow clamped to `[0,1]`, texgen 0, the materials refused silently | no scripts |
| a light with intensity 2, radius 30, falloff 3 | clamped to 1, 20, 2 | the same | no scripts |
| a light with negative values | 0, 0, 0 | 0, **0.1, 0.01** | no scripts |
| a light or flexible path turned off | every value read back as 0 | the other values kept (a light's colour, radius 0.1 and falloff 0.01) | no scripts |
| a flexible path past its ranges: softness 5, gravity 20, friction −20, wind 20, tension 20 | clamped: 3, 10, 0, 10, 10 | kept as given | no scripts |
| a flexible tension of 0.25 | read back 0.2: kept in tenths | 0.25 | no scripts |
| hover text of 300 characters | **refused**: the old text stays | cut to 254 | no scripts |
| a slice past its range; reversed (0.8, 0.2) | clamped; the begin pulled below the end, `<0.18, 0.2>` | clamped; **swapped round**, `<0.2, 0.8>` | no scripts |
| a name of 100 characters | cut to 63 | **kept** | no scripts |
| a description of 200 characters | cut to 127 | the same | no scripts |
| a pipe or a newline in a name or description | each becomes `?` | kept | no scripts |
| alpha mode with a cutoff past 255, an alpha mode that does not exist (9) | 255; mode 3 | refused: `must be 0 to 255`, `must be 0 to 3` | no scripts |
| normal and specular maps | round-trip | the same | no scripts |
| the sit target, `PRIM_ALLOW_UNSIT`, `PRIM_SCRIPTED_SIT_ONLY`, a click action, sit flags, a reflection probe, a render material | set and read back; a click action of 99 is ignored | the sit target round-trips; the rest are `unsupported parameter` | no scripts |
| `PRIM_POSITION` 15 m east | 10 m | 10 m | no scripts |

### Rule lists

`prim/params-rules`: a rule list as a whole.

| what | Second Life | OpenSim | fake grid |
| --- | --- | --- | --- |
| several rules in a get, the same rule twice | in the order asked, once per asking | the same | no scripts |
| `ALL_SIDES` in a get | one entry per face, in face order | the same | no scripts |
| a get of a face past the prim's (6, 100, −2) | nothing for that rule; the rules after it are answered (−2 is `ALL_SIDES`) | face 6 is answered as if it were there; **face 100 and −2 throw**: YEngine stops the script (`100 is outside the range of MAX_FACES`), XEngine ends the event | no scripts |
| a set of face 6, of face −2 | face 6 nothing; −2 as `ALL_SIDES` | nothing for either | no scripts |
| an unknown rule code in a get (9999, −5) | the whole get answers **nothing**, `Param 0 error, unknown parameter` | the code is skipped, the rules after it answered | no scripts |
| an unknown rule code in a set | `error running rule #1: unknown rule.`; the rules after it do not run | `unsupported parameter`; the rules after it do not run | no scripts |
| a rule code as a string (`"7"`) | the get answers nothing | read as the integer | no scripts |
| a value of the wrong type: a glow as a string, a vector; a face as a float; a colour as a string; a name as an integer or a key | refused with `arg #2 (glow) float expected but string given.` and its like; the rules after it do not run | a string, integer or key that converts is **taken** (glow `"0.5"`, name `7`); a vector, a float face and a string colour are refused (`parameter 3 must be float`), and the rules after them do not run | no scripts |
| a glow as an integer | taken | taken | no scripts |
| a rule with too few values | the rule does nothing, silently | the same | no scripts |
| a name, a glow of `"x"`, then a colour | the name is set, the colour is not | all three run: `"x"` is read as a glow of 0 | no scripts |
| `PRIM_LINK_TARGET` 0, `LINK_THIS` on a single prim | the prim | the same | no scripts |
| `PRIM_LINK_TARGET` 1 or `LINK_SET` on a single prim, get | nothing | the prim | no scripts |
| `PRIM_LINK_TARGET` 2 on a single prim | nothing for the rules after it | the same | no scripts |
| `llGetLinkPrimitiveParams` of 0, `LINK_THIS` on a single prim | the prim | the same | no scripts |
| `llGetLinkPrimitiveParams` of 1, `LINK_SET`, `LINK_ROOT` on a single prim | nothing | **the prim** | no scripts |
| `llSetLinkPrimitiveParamsFast` of 0, `LINK_THIS`, `LINK_SET` on a single prim | the prim | the same | no scripts |
| `llSetLinkPrimitiveParamsFast` of 1, `LINK_ROOT` on a single prim | nothing | **the prim** | no scripts |

### The older per-face calls

`prim/faces`.

| what | Second Life | OpenSim | fake grid |
| --- | --- | --- | --- |
| `llGetColor(ALL_SIDES)` | the **mean** of the faces' colours | the same | no scripts |
| `llGetAlpha(ALL_SIDES)` | the **sum** of the faces' alphas (6 for an opaque cube) | the same | no scripts |
| `llSetColor` past its range, read by `llGetColor` in the same event | **as given**, `<2,−1,0.5>` (`PRIM_COLOR` reads it clamped) | clamped | no scripts |
| `llSetColor` of face 6 | nothing | the same | no scripts |
| `llSetColor` of face −2 | every face, as `ALL_SIDES` | nothing | no scripts |
| `llGetColor` or `llGetAlpha` of face 6 | white, 1 | **black, 0** | no scripts |
| `llGetColor` of face −2 | the mean, as `ALL_SIDES` | black | no scripts |
| `llSetAlpha` past 1 and below 0 | clamped | the same | no scripts |
| `llSetTexture` of a name not in the inventory, `NULL_KEY` or `""` | not applied, `Could not find texture '…'.` (also for `NULL_KEY`) | a name or `""`: not applied, silently; **`NULL_KEY` is applied** | no scripts |
| `llGetTexture` of face 6 | `NULL_KEY` | the same | no scripts |
| `llScaleTexture` past its range, of 0 | kept as given | the same | no scripts |
| `llOffsetTexture` past ±1 | kept | clamped to ±1 | no scripts |
| `llRotateTexture` past 2π | kept | modulo 2π | no scripts |
| `llSetText` alpha 2 | 1 | **0** | no scripts |
| `llSetText` alpha −1 | 0 | **1** | no scripts |
| `llSetText` of 300 characters; of 100 + three `é` + 300 | cut to 254; to 251 characters (254 bytes) | the same | no scripts |
| `llGetNumberOfSides`: box 6, hollow 7, cut 6, hollow and cut 7, tapered to a point 6; cylinder 3, hollow and cut 6; prism 5; sphere 1, hollow, cut and dimpled 6; torus 3; tube 6; ring 5; sculpt 1 | as listed | the same | no scripts |
| `llSetLinkColor`, `llSetLinkAlpha`, `llSetLinkTexture` of `LINK_THIS`; of link 0 | the prim | the same | no scripts |
| `llSetLinkColor` of link 2 on a single prim | nothing | the same | no scripts |

### The object's identity

`prim/identity`.

| what | Second Life | OpenSim | fake grid |
| --- | --- | --- | --- |
| a fresh prim | `llGetLinkNumber` 0, `llGetNumberOfPrims` 1, `llGetCreator` the owner | the same | no scripts |
| `llGetLinkKey(0)`, `llGetLinkKey(1)` on a single prim | the prim, `NULL_KEY` | the prim, **the prim** | no scripts |
| `llGetLinkName(1)` on a single prim | `NULL_KEY` as text | the same | no scripts |
| `llSetObjectName` of 64 or 100 characters | cut to **63** | **kept** | no scripts |
| a name of forty `é` | sixty-two `?`: cut to whole characters within 63 bytes, and every byte of a character outside ASCII reads back as `?` | kept | no scripts |
| a description of 128 or 200 characters | cut to 127 | the same | no scripts |
| a description of a hundred `é` | a hundred and twenty-six `?` | sixty-three `é`: cut within 127 bytes | no scripts |
| a pipe or a newline in a name or description | `?` | kept | no scripts |
| leading and trailing spaces, an empty name | kept | the same | no scripts |
| `llSetClickAction` 0 to 9 | `PRIM_CLICK_ACTION` and `OBJECT_CLICK_ACTION` read it back | `OBJECT_CLICK_ACTION` reads it back; `PRIM_CLICK_ACTION` is unsupported | no scripts |
| `llSetClickAction` 10, 99, −1 | ignored: reads 0 | **kept**: 10, 99, 255 | no scripts |
| `llGetObjectDetails` of `NULL_KEY`, a key nothing has, text that is no key, or with no codes | an empty list | the same | no scripts |
| an unknown code among known ones | −1 in its place (`OBJECT_UNKNOWN_DETAIL`) | the same | no scripts |
| `OBJECT_VELOCITY` of a prim at rest | a vector | a value `llGetListEntryType` calls `TYPE_INVALID`, printed `<0, 0, 0>` | no scripts |
| `OBJECT_STREAMING_COST`, `OBJECT_PHYSICS_COST`, `OBJECT_SERVER_COST` of the cube | 0.05, 1.0, 0.5 | 1.0, 0.1, 0 | no scripts |
| `OBJECT_LAST_OWNER_ID` | `NULL_KEY` | the owner | no scripts |
| codes 42 to 56 (`OBJECT_MATERIAL` to `OBJECT_VOLUME_DETECT`) | answered (material 3, mass 125, scale, text colour and alpha, health, damage, the five permission masks, …) | **−1** for each | no scripts |
| `OBJECT_PRIM_COUNT` of the owner | the prims the avatar wears (22 here) | 0 | no scripts |

### Linking

`prim/link`: prim A links prim B, two metres east, once the owner has
granted `PERMISSION_CHANGE_LINKS`; each step is a second and a half apart.

| what | Second Life | OpenSim | fake grid |
| --- | --- | --- | --- |
| `llCreateLink` with no permission | `Script trying to link but PERMISSION_CHANGE_LINKS permission not set!` | `llCreateLink: PERMISSION_CHANGE_LINKS permission not set` | no scripts |
| `llCreateLink` of `NULL_KEY`, a key nothing has, the owner | `Script trying to link but target is invalid.` each | nothing, silently | no scripts |
| linked | `CHANGED_LINK` in both prims; the root is link 1, the child 2, `llGetNumberOfPrims` 2 | the same, and the child gets a `changed` of 32768 (OpenSim's `CHANGED_POSITION`) whenever it moves | no scripts |
| `llGetLinkKey(0)` in a linkset | `NULL_KEY` | the prim itself | no scripts |
| `llSetLinkPrimitiveParamsFast` of `LINK_SET`, `LINK_ALL_OTHERS`, `LINK_ALL_CHILDREN`, `LINK_ROOT`, `LINK_THIS`, 1, 2 | the prims each names | the same | no scripts |
| … of link 0 or 3 in a linkset of two | nothing | the same | no scripts |
| `llGetLinkPrimitiveParams` of `LINK_SET`, `LINK_ALL_OTHERS`, `LINK_ALL_CHILDREN` | **nothing** | the prims each names, in link order | no scripts |
| `PRIM_LINK_TARGET` in a get and in a set | moves on to the link named | the same | no scripts |
| the child's `PRIM_POS_LOCAL` 5, 20, 60 m | set as given | the same | no scripts |
| the child's `PRIM_POSITION` of a region position 5, 20, 60 m from the root | **nothing**: the child stays where it was | the region position is taken **as the offset**: the child goes that far from the root | no scripts |
| `llSetPos` in a child, 15 m from the root | 15 m: no 10 m cap for a child's offset | **12 m**: capped 10 m from where it was (2 m) | no scripts |
| `llBreakLink(2)`, `llBreakLink(1)` | unlinks; `CHANGED_LINK` in both | the same | no scripts |
| `llBreakLink(3)` | nothing | the same | no scripts |
| `llCreateLink(…, FALSE)` | the target becomes the root, this script's prim link 2 | the same | no scripts |
| `llBreakAllLinks` from a child | unlinks every prim | the same | no scripts |

### `changed`: which setter raises what

`prim/changed`: one setter a second, and every `changed` event in the second
after it.

| setter | Second Life | OpenSim | fake grid |
| --- | --- | --- | --- |
| `llSetColor`, `llSetAlpha`, `llSetLinkColor` | `CHANGED_COLOR` | the same | no scripts |
| the same colour again | `CHANGED_COLOR` again | **nothing** | no scripts |
| the same alpha again | `CHANGED_COLOR` | the same | no scripts |
| `llSetTexture`, `llScaleTexture`, `llOffsetTexture`, `llRotateTexture`, glow, fullbright, bump and shiny, texgen, alpha mode, normal and specular maps | **nothing** | `CHANGED_TEXTURE` (the same texture again: nothing) | no scripts |
| `llSetScale`, `PRIM_SIZE` | `CHANGED_SCALE` | the same | no scripts |
| the same scale again | nothing | `CHANGED_SCALE` again | no scripts |
| `PRIM_TYPE` | `CHANGED_SHAPE` | the same | no scripts |
| `llSetPos` | nothing | 32768 (`CHANGED_POSITION`, OpenSim's own) | no scripts |
| `llSetRot`, text, name, description, light, flexible, material, phantom, slice, `llTargetOmega`, texture animation, particles, sit target, sit text, click action, `llAllowInventoryDrop` | nothing | the same | no scripts |
| `llSetPrimMediaParams`, `llClearPrimMedia` | `CHANGED_MEDIA` | YEngine the same; XEngine nothing for the set, **two** for the clear | no scripts |
| a render material (`PRIM_RENDER_MATERIAL`) | `CHANGED_RENDER_MATERIAL` | nothing: the rule is unsupported | no scripts |
| colour and size in one call | one event, `CHANGED_COLOR \| CHANGED_SCALE` | **two** events | no scripts |

### Sound, particles, texture animation and media

`prim/effects`: no function reads these back.

| what | Second Life | OpenSim | fake grid |
| --- | --- | --- | --- |
| a sound name not in the inventory, `NULL_KEY` or `""`, to `llPlaySound`, `llLoopSound`, its master and slave, `llTriggerSound`, `llPreloadSound` | `Could not find sound '…'.` | nothing, silently | no scripts |
| a sound key no asset has, a volume past 1 or below 0, `llCollisionSound` of a missing name, the radius, volume and queueing calls past their ranges, `llStopSound` with nothing playing | nothing | the same | no scripts |
| `llParticleSystem` with a value of the wrong type | `PSYS error: Rule 1 error, PSYS_PART_MAX_AGE needs float data.` | nothing | no scripts |
| … with a rule missing its value | nothing; the script carries on | **throws** (`IndexOutOfRangeException`): YEngine stops the script, XEngine ends the event | no scripts |
| … with an unknown rule, a missing texture, values past their ranges; `llLinkParticleSystem` of link 3; texture animations of zero frames, a negative rate, face 9, without `ANIM_ON` | nothing | the same | no scripts |
| `llSetPrimMediaParams` of face 9 | 1003 (`STATUS_NOT_FOUND`), `PRIM_MEDIA error: face 9 on object … not found` | 1003, silently | no scripts |
| a URL past 1024 characters, a width past 2048 | 1002 (`STATUS_BOUNDS_ERROR`), `Bounds error near parameter #0, param=3.  No values have been set.` | **0**: kept as given | no scripts |
| an unknown media rule | 1000 (`STATUS_MALFORMED_PARAMS`), `Malformed parameters …` | 1000, silently | no scripts |
| `llGetPrimMediaParams` of an unknown rule | nothing | `1000` | no scripts |
| `llClearPrimMedia` of face 9 | 1003 | the same | no scripts |

### What the viewer does with it

Every setter here ends in an object update, which the viewer already renders
as it arrives: the position and rotation, the scale, the shape, each face's
colour, texture and material, the light, the flexible path, hover text, the
particle system and the texture animation. `llTargetOmega`'s spin is applied
by the viewer itself from the update's angular velocity. The grid's clamps are
the grid's: the viewer draws what it is sent.

## Run-time errors

Measured on 2026-10-23 by the probe `errors/runtime-errors` and on
2026-10-27 by `errors/math`, `errors/math-lso`, `errors/bounds`,
`errors/bounds-newer` and `errors/flood`, on the same grids. Each case that
might fault is a script of its own, since a fault stops a script on Second
Life; a controller sends each its case in turn, reads every script's
`llGetScriptState`, tries two ways back for a faulted one and pings them
all. Listeners in other objects hear `DEBUG_CHANNEL` at 10, 19, 21 and 50 m.
`errors/math-lso` is `errors/math` with the faulters compiled to LSO, which
only Second Life honours.

### What faults

The cases that fault on some grid; the zeros come from a link message's
text, so no compiler folds them away.

| case | Second Life, Mono | Second Life, LSO | OpenSim YEngine | OpenSim XEngine | fake grid |
| --- | --- | --- | --- | --- | --- |
| integer division or modulo by zero | `Math Error` | `Math Error` | `Division by Zero` | `Integer division by Zero` | no scripts |
| float division by zero | `Math Error` | `Math Error` | `Division by Zero` | `Float division by zero` | no scripts |
| a vector divided by `0.0` | `Math Error` | `Math Error` | `Vector division by zero` | `Vector division by zero` | no scripts |
| a rotation divided by `<0, 0, 0, 0>` | `<0, 0, 0, 0>` | the same | `<0, 0, 0, 1>` | the same | no scripts |
| `-2147483648 / -1`, `% -1` | `-2147483648`, `0` | the same | the same | **throws** `System.OverflowException: Arithmetic operation resulted in an overflow.` and the engine's stack | no scripts |
| `llSqrt(-1.0)` | `NaN` | `Math Error` | `NaN` | `NaN` | no scripts |
| `1.0e38 * 1.0e38` | `Infinity` | `Math Error` | `1e76`, written out in 77 digits: OpenSim's floats are doubles | the same | no scripts |
| `llModPow(2, 3, 0)` | `0` | `0` | throws `DivideByZeroException` (below) | throws `DivideByZeroException` (below) | no scripts |
| `llLog(0.0)`, `llLog10(-1.0)` | `0.0`, `0.0` | the same | `-Infinity`, `NaN` | the same | no scripts |
| `llAcos(2.0)`, `llAsin(-2.0)` | `NaN`, `NaN` | `Math Error` | `NaN`, `NaN` | the same | no scripts |
| `llPow(-1.0, 0.5)`, `llPow(0.0, -1.0)` | `NaN`, `Infinity` | `Math Error` | `NaN`, `Infinity` | the same | no scripts |
| `(float)"inf"`, `(float)"nan"` and their casts to integer | `Infinity`, `-2147483648`, `NaN`, `-2147483648` | `Math Error` | `0.0` and `0` for all four: neither string parses | the same | no scripts |
| `llVecNorm(ZERO_VECTOR)`, `llRot2Euler(<0, 0, 0, 0>)`, `llAxes2Rot` of three zero vectors | zero, zero, `<1, 0, 0, 0>` | the same | zero, zero, `<0, 0, 0, 0.5>` | the same | no scripts |
| unbounded recursion | `Stack-Heap Collision` ([Running out](#running-out)) | the same | the simulator aborts (YEngine with a library call at each level; without one it catches a `StackOverflowException` and says nothing) | the simulator aborts | no scripts |

So Mono faults only on division by zero, where LSO also faults on every
result that is not a finite number — and Mono hands such a result on as
`Infinity` or `NaN`.

### What is said, and to whom

| | Second Life (Mono and LSO) | OpenSim YEngine | OpenSim XEngine | fake grid |
| --- | --- | --- | --- | --- |
| a fault | **two** messages on `DEBUG_CHANNEL`: `<object name> [script:<script name>] Script run-time error`, then the error (`Math Error`, `Stack-Heap Collision`); the prefix is the object's name at the time (`Faulty prim [script:Math 1] …` after `llSetObjectName`) | **one**: `YEngine: <error> (script: <name> event: <event> primID: <key> at: <x,y,z>) Script must be Reset to re-enable.` and a newline; the position rounded to whole metres | **one**: `<error>(script: <name> event: <event> primID:<key> at <x, y, z>)` — no space before the bracket, the position as a vector | no scripts |
| an exception inside a library function (`llModPow`, `llList2ListStrided`, `llListStatistics`, `llInsertString` below) | — | **not on `DEBUG_CHANNEL`**: ten or so owner-say lines (`ChatType::Owner`) to the owner — `[YEngine]: Exception while running <item>`, the exception (`DivideByZeroException: Attempted to divide by zero.`), `Prim: <name>, Script: <name>, Location: <region> <x,y,z>`, `Script must be Reset to re-enable.` and one line per frame of the stack | one `DEBUG_CHANNEL` message holding the .NET exception and its stack: `System.Reflection.TargetInvocationException: Exception has been thrown by the target of an invocation. ---> System.DivideByZeroException: …` | no scripts |
| a library call refusing its arguments (`llGetScriptState` of a missing script) | one message: `Could not find script 'Nope'.` | one: `llGetScriptState: Can't find script 'Nope'` | the same | no scripts |
| the range | **said**: a script 19 m away hears both messages, one 21 m away neither; an avatar past 20 m is sent the lines barely audible, with their text left out | a fault reaches every avatar near the object but **no script**, at 19, 21 or 50 m; a refused call is heard by scripts 50 m away | the same | no scripts |
| the speaker | the prim, by its name; its owner's key in the chat's owner field | the same | the same | no scripts |

### Floods

`errors/flood` makes twelve errors as fast as a script can (a missing
script's name to `llGetScriptState`, which costs the script a second on
every grid), asks a second script in the same prim and a script in another
object of the same owner's for one each after the tenth, then makes one more
after 10, 30 and 60 seconds of quiet.

| | Second Life | OpenSim (both engines) | fake grid |
| --- | --- | --- | --- |
| a run of errors | the first **eight** pass, then `Too many errors... dropping further messages until the flood stops` and nothing more | every one passes | no scripts |
| whose flood it is | the **object's**: the second script in the same prim is dropped, the other object's error passes | — | no scripts |
| when errors pass again | dropped after 10 seconds of quiet, passed after 30 | — | no scripts |
| a fault's two lines | count as two: a fourth fault within the window lost its `Math Error` to the notice | — | no scripts |

### The faulted script

| | Second Life (Mono and LSO) | OpenSim YEngine | OpenSim XEngine | fake grid |
| --- | --- | --- | --- | --- |
| `llGetScriptState` | `0`: stopped | `1`, yet it answers nothing — a link message goes nowhere | `1`, and it runs on: the event ends, the next is handled, in the state it faulted in with its globals | no scripts |
| `llSetScriptState(…, TRUE)` alone | does not start it | answers nothing still | — | no scripts |
| `llResetOtherScript`, then `llSetScriptState(…, TRUE)`, or the other way round | running again: it answers | answers again | — | no scripts |

### Indices and strides out of range

`errors/bounds` and `errors/bounds-newer` read and cut a six-element list and
a six-character string past both ends, in reverse and with strides of zero,
negative and larger than the list. **Nothing here faults on Second Life**;
where OpenSim throws, it is an exception inside the library function, said
as above.

| case | Second Life | OpenSim (both engines) | fake grid |
| --- | --- | --- | --- |
| `llList2*` and `llGetListEntryType` at 99 or -99 | the type's zero (`ZERO_ROTATION` for a rotation), `TYPE_INVALID` | the same | no scripts |
| `llList2Vector` of the string `"<1,2,3>"`, `llList2Rot` of `"<1,2,3,4>"` | `ZERO_VECTOR`, `ZERO_ROTATION`: a string is not converted | converted: `<1, 2, 3>`, `<1, 2, 3, 4>` | no scripts |
| `llList2Integer` of `1.9`, `llList2Float` of `"1e3"` | `1` (truncated), `1000.0` | `2` (**rounded**), `1.0` (the exponent ignored) | no scripts |
| `llList2Integer` of `"0x10"`, of `" 7 apples"`, of a vector; `llList2Key` of an integer | `16`, `7`, `0`, `""` | the same | no scripts |
| `llList2List`, `llListReplaceList`, `llListInsertList`, `llGetSubString`, `llDeleteSubString` past either end and reversed | the documented wrap-around | the same | no scripts |
| `llDeleteSubList(l, 1, -99)` | `[a]` | `[]` | no scripts |
| `llList2ListStrided` with a stride of `0`; `llListSort` with `0`, `-1`, `4` or `99`; `llListRandomize` with `0`, `-1`, `4` | the list unchanged (sorted for a sort), six elements | the same | no scripts |
| `llList2ListStrided` with a stride of `-1` or `99`, or a reversed range | `[a, b, c, d, e, f]`, `[a]`, `[a, c, e]`, `[]` | **throws** `IndexOutOfRangeException` | no scripts |
| `llListStatistics` of an empty list, an unknown operation, the median of strings | `0.0` each | **throws** `IndexOutOfRangeException` at the median of strings | no scripts |
| `llListFindList` of an empty pattern | `0`, in an empty list too | `-1` | no scripts |
| `llInsertString` at 99, -5, -1 | appended, prepended, prepended | **throws** `IndexOutOfRangeException` at a negative index | no scripts |
| `llParseString2List` with nine separators | uses the first **eight**: `[a, …, h, i9j]` | uses all nine | no scripts |
| `llParseString2List` with an empty separator or spacer | ignored | the same | no scripts |
| `llOrd` at 99 or -99; `llChar(-1)`, `llChar(0x110000)`, `llChar(0)` | `0`; U+FFFD, U+FFFD, `""` | the same | no scripts |
| `llReplaceSubString` with an empty pattern; a count of -1 or 99 | the string unchanged; the last, or every, match | does not compile: `undefined name llReplaceSubString(…)` (YEngine), CS0103 (XEngine) | no scripts |
| `llList2ListSlice`, `llListFindStrided`, `llListSortStrided` with a stride of zero | `[]`, `0`, the list unchanged | do not compile | no scripts |

### What the viewer does with it

Every `DEBUG_CHANNEL` line reaches the viewer as `ChatType::DebugChannel`
chat. The reference shows its own objects' lines in its Script Warning
window (`llfloaterscriptdebug.cpp`) and drops the rest on Second Life;
Firestorm's OpenSim build keeps every object's, since OpenSim sends an OSSL
denial to everyone near ([OSSL](#ossl)).

The viewer's **Script Errors** window (Advanced ▸ Script Errors,
`sl-viewer-notices`' `script_errors`) does the same by default: it takes the
debug lines of the agent's own objects — told by the chat's owner field —
opens on the first, and keeps them under an All tab and a tab per object,
each line stamped with the local time it arrived and a message of several
lines (XEngine's .NET stack, YEngine's report) a row per line. An object's
tab is headed by its name, a link to the object inspector (with Show on
Map), and where it was, and marks the object with a beacon while it is
shown. Preferences ▸ Chat moves the lines to nearby chat, where they wear
their own colour (*Script errors*, the reference's `ScriptErrorColor`), or
nowhere; shows other residents' objects' lines too; sends `llOwnerSay` to
the window instead of nearby chat; and empties the window when it is closed.
One rule (`script_chat_route`) decides for the window, the transcript and
the overlay alike, so a line shows in exactly one of them. A line Second
Life sends an avatar past say range comes with no text, and the window
takes nothing from it. No grid names the line a script faulted on, so
nothing opens the editor there. A YEngine library exception is owner chat,
not a debug line, so it lands in nearby chat as any `llOwnerSay` does
(unless that is sent to the window).

## Strings, lists, maths and rotations

Measured on 2026-10-27 by `math/rotation-bits`, `math/rotations`,
`math/negate-rotation`, `math/functions`, `strings/conversions`,
`strings/functions`, `strings/unicode`, `strings/newer` and the three
`vm/call-cost-*` timing probes, on aditi's Mauve and the local OpenSim under
YEngine and XEngine. Every operand comes out of a list built at run time, so
no compiler folds it. Where the two engines agree, OpenSim is one column: the
library is shared (`LSL_Api.cs`, `LSL_Types.cs`), only the compilers differ.
Out-of-range indices and the cases that fault are
[Run-time errors](#run-time-errors)'.

### Precision

`math/rotation-bits` prints each result's mantissa and exponent, found by
scaling by two, so a float can be told from its neighbour; the result was
then matched against candidate evaluations (each operation rounded to a
float, everything in double rounded once, products rounded and summed in
double, and so on). Seven pairs of rotations, the first PyOptimizer's open
case `<3, 5, 7, 17> * <.22, .26, .38, .86>`, the others chosen where the
candidates disagree. `math/float-rounding` (2026-10-30) then asked every
place a value can go whether it rounds there: `1.1 * 1.3` is exact in
double and not a float, so a value that reaches the place unrounded and is
then taken less the stored product leaves a residual, and a rounded one
leaves 0.

| what | Second Life (Mono) | OpenSim YEngine | OpenSim XEngine | fake grid |
| --- | --- | --- | --- | --- |
| a variable, a list element, a vector's or rotation's component | a float: 24 bits | a **double**: 53 bits (`0.1` is the double 0.1) | the same | no scripts |
| an expression of floats (`a * b + c * d`, `a * b * c`) | evaluated in **double** and rounded to a float only when stored: the same sum split over two statements comes out a bit apart | in double throughout | the same | no scripts |
| where a float expression is rounded (`math/float-rounding`) | **rounded** when stored into a variable or a component, passed to a user or library function, returned, put in a list (`[x]`, `l += x`) or a vector or rotation being built, as the factor of `vector * float`, and by `(integer)` (`(integer)(1.4999996 * 2.0000005)` is 3) and `(string)` (`(string)(1.05 * 3.86)` is `4.052999`); **kept in double** by `/` and the other operators, unary `-`, the dot product inside a longer expression, a `(float)` cast, a comparison (`a * b == R` is false where `R` holds the stored product), a condition (`if (1e-30 * 1e-20)` is taken), the value of `=`, `+=` and `++` (`++` of 16777216.0 is 16777217 until stored) and an integer met by a float (`(float)16777217` and `16777217 * 1.0` are exact); `1e30 * 1e30 / 1e30` is `1e30`. Double, not x87's 64-bit mantissa: `1 + 2^-24 + 2^-60` is 1 | every value a double, so nothing to round; `(integer)` gives 2, `(string)` `4.053000`, the comparison is true | the same | no scripts |
| `rotation * rotation`, `vector * rotation`, `vector * vector` (dot) | in double, rounded once at the end: all seven products matched that, five of them nothing else; PyOptimizer's case gives `y` = 8.3199997 (prints as `8.320000`) | in double | the same | no scripts |
| `rotation / rotation`, `vector / rotation`, `vector % vector` (cross) | **each operation rounded to a float**: `/ rotation` is the product with the conjugate, `vector / rotation` Linden's two-step formula (`llquaternion.cpp`), each product and sum rounded | in double | the same | no scripts |
| `llSqrt`, `llSin`, `llPow` … | the double result of the float argument, rounded to a float | the double result | the same | no scripts |
| a decimal literal | the nearest float | **one unit in the last place off** the nearest double for 11 of the 56 literals (`-0.819`, `0.398`, `0.751` …): YEngine's own number parser | the nearest double | no scripts |
| a float printed by `(string)`, `llList2String`, `llDumpList2String` | rounded to **7 significant digits**, then six decimals: `123456789.0` → `123456800.000000`, `1234567.875` → `1234568.000000` | the double with six decimals: `123456789.000000` | the same | no scripts |
| a float printed by `llList2CSV` | the float's **exact** value with six decimals: `123456792.000000`, `1e30` → `1000000015047466219876688855040.000000` | the double: `1000000000000000000000000000000.000000` | the same | no scripts |
| a float past the float range (`3.4e38 * 10`), `(float)"1e39"` | `Infinity` (`inf` in `llList2CSV`) | `3.4e39`, `1e39`: a double | the same | no scripts |
| negative zero | `-0.000000` from `(string)`, `llList2CSV` and `llList2String`; `0.000000` from `llDumpList2String` | `0.000000` | the same | no scripts |
| a vector by `(string)` | five decimals (`<1.00000, 0.00000, 123456.70000>`); six inside a list | six | the same | no scripts |

### Rotations

`math/rotations` and `math/negate-rotation`. The axis order agrees: on every
grid `llEuler2Rot` turns about x, then y, then z, `a * b` applies `a` first,
and `llRotBetween`, `llAxes2Rot`, `llRot2Fwd` / `Left` / `Up`, `llRot2Axis`,
`llRot2Angle` and `llAngleBetween` give the same answers for unit input
(within the six printed decimals). They part over input that is not a unit
rotation.

| what | Second Life | OpenSim | fake grid |
| --- | --- | --- | --- |
| `llEuler2Rot(<400°, −200°, 1000°>)` | `<−0.549, 0.747, 0.153, 0.342>` | the **negation**, `<0.549, −0.747, −0.153, −0.342>` — the same rotation | no scripts |
| `llRot2Euler` back from it, and from `<30°, 45°, 60°>` | `<−140, 20, 100>`; `<30, 45.00001, 60.00001>` | `<−140.000004, 19.999999, 99.999993>`; exact | no scripts |
| the gimbal: `<10°, 90°, 20°>` and `<10°, −90°, 20°>` back | `<0, 90, 30>` and `<0, −90, 10>`: x folded into z | the same, `±90.000001` | no scripts |
| `vector * rotation` by `<0, 0, 0, 2>` | `<4, 0, 0>`: **scaled by the square of the length** — no normalising | `<1, 0, 0>`: normalised first | no scripts |
| `vector * rotation`, `vector / rotation` by `<1, 2, 3, 4>` | `<4, 28, −10>`, `<4, −20, 22>` | `<−25, 28, −10>`, `<−25, −20, 22>`: a different formula, which agrees only for unit rotations | no scripts |
| `vector * <0, 0, 0, 0>` | `ZERO_VECTOR` | the vector unchanged | no scripts |
| `llAxisAngle2Rot` of a zero axis, of a `1e-7` axis, by 90° | `<0, 0, 0, 1>`, `<0, 0, 0, 1>` | `<0, 0, 0, 0.707107>` (not a unit rotation); `<0, 0, 0.707, 0.707>`: the tiny axis normalised | no scripts |
| `llAxisAngle2Rot` of an axis of length 2; by 450° | the axis normalised; the negated rotation of 90° | the same | no scripts |
| `llVecNorm(<0, 0, 1e-7>)` | `ZERO_VECTOR`: too short | `<0, 0, 1>` | no scripts |
| `llRot2Axis`, `llRot2Angle` of the identity, its negation, `<0, 0, 0, 2>`, `<0, 0, 0, 0>` | zero axis, angle 0 | the same | no scripts |
| `llRotBetween` of opposite vectors (x and −x) | `<0, 0, 1, 0>`; z and −x `<0, −0.707, 0, 0.707>` | the same | no scripts |
| `llRotBetween` with a zero vector, or of parallel vectors of different lengths | the identity | the same | no scripts |
| `llAngleBetween` of a rotation and its negation, or `<0, 0, 0, 2>` | 0 | the same | no scripts |
| `llAxes2Rot` of x of length 2, of a left-handed set, of skewed axes | `<0, 0, 0, 1.118>`, `<0, 0, 0, 0.707>`, `<0, 0, −0.25, 1>`: not normalised | the same | no scripts |
| `llRot2Fwd` / `Left` / `Up` of `<1, 2, 3, 4>` | the axes of the normalised rotation | the same | no scripts |
| `-rotation` | `<−1, −2, −3, −4>` | YEngine the same; **XEngine does not compile it** (`CS0023: The '-' operator cannot be applied to operand of type '…Quaternion'`) | no scripts |
| `rotation + rotation`, `rotation - rotation` | component by component | the same | no scripts |

### Numbers

`math/functions`.

| what | Second Life | OpenSim | fake grid |
| --- | --- | --- | --- |
| `llRound` of 0.5, 1.5, 2.5 | 1, 2, 3 | the same | no scripts |
| `llRound` of −0.5, −1.5, −2.5 | **0, −1, −2**: half rounds **up**, `floor(x + 0.5)` | −1, −2, −3: half away from zero | no scripts |
| `llRound(0.49999997)` | **1**: the `+ 0.5` is done in float | 0 | no scripts |
| `llRound`, `llFloor`, `llCeil`, `(integer)` of 2147483648, −2147483904, ±1e10 | −2147483648 for all | the same | no scripts |
| `llAbs` and `-` of −2147483648; `int-max + 1`; `int-min - 1`; `int-max * 2` | wrap: −2147483648, −2147483648, 2147483647, −2 | the same | no scripts |
| shifts by 32, 33, −1 | the count taken modulo 32: `1 << 32` is 1, `1 << -1` is −2147483648; `>>` is arithmetic | the same | no scripts |
| `-7 % 3`, `7 % -3`, `-7 / 2` | −1, 1, −3: truncating | the same | no scripts |
| `llPow(0, 0)`, `(−8, 1/3)`, `(−8, 2)` | 1, NaN, 64 | the same | no scripts |
| `llPow(1e30, 2)`, `(1e-6, −7)` | `Infinity` | 1e60, 1e42 | no scripts |
| `llTan(PI_BY_TWO)`, `llTan(1e30)` | −22877330, **1e30** (the argument handed back) | −22877332.43, −0.009332 | no scripts |
| `llAtan2(0, 0)`, `(0, −1)`, `(−0, −1)` | 0, π, −π | the same | no scripts |
| `llModPow(-2, 3, 7)`, `(2, -1, 7)`, `(2, 3, -7)` | 1, 1, 8: every operand read as an **unsigned** 32-bit integer | −1, 0, 1 | no scripts |
| `llModPow(7, 1000000007, 1000000007)`, `(2147483647, 2147483647, 2147483646)`, `(3, 2147483647, 65537)` | 7, 1, 21846: exact | −291172004, −8, −32769: `Math.Pow` in a double, cut to a long | no scripts |
| `llFrand(1.0)`, `(−1.0)`, `(0.0)` over 2000 draws | in (0, 1), in (−1, 0), always 0; no draw of the first two was 0 | the same | no scripts |
| `llFabs(-0.0)`, `llSqrt(-0.0)` | `-0.000000` | `0.000000` | no scripts |

### Casts and conversions

`strings/conversions`. A `\t` in a string literal is four spaces on every
grid, since the compiler writes it so.

| what | Second Life | OpenSim | fake grid |
| --- | --- | --- | --- |
| `(integer)` of `"0x1F"`, `" +5"`, `"5e2"`, `"  12abc"`, `"1.9"`, `"-1.9"` | 31, 5, 5, 12, 1, −1 | the same | no scripts |
| `(integer)` of `"-0x1"`, `"+-5"`, `"abc"`, `"0x"`, `"٣"`, `"０７"` | 0 | the same | no scripts |
| `(integer)"2147483648"`, `"-2147483649"` | −2147483648, 2147483647: wraps | **−1** for both | no scripts |
| `(integer)` of `"4294967296"`, `"99999999999"`, `"0x100000000"`; `"0xFFFFFFFF"` | −1; −1 | the same | no scripts |
| `(integer)` of `"    \n7"` | 7: all leading white space skipped | **0**: only spaces | no scripts |
| `(float)` of `"1e3"`, `".5"`, `"-.5e1"`, `"5."`, `"  1.5x"`, `"1,5"`, `"1e"`, `"1.5e+2"` | 1000, 0.5, −5, 5, 1.5, 1, 1, 150 | the same | no scripts |
| `(float)` of `"0x1p4"`, `"0x10"` | 16, 16: hexadecimal floats | 0 | no scripts |
| `(float)` of `"infinity"`, `"-inf"`, `"NaN"`, `"1e39"` | `Infinity`, `-Infinity`, `NaN`, `Infinity` | 0, 0, 0, 1e39 | no scripts |
| `(float)"1e-50"`, `"e5"` | 0 | the same | no scripts |
| `(vector)` of `" <1,2,3>"` and `"1,2,3"` | `ZERO_VECTOR`: it must start with `<` | `<1, 2, 3>`: the brackets are ignored | no scripts |
| `(vector)` of `"<1,2,3> tail"`, `"<1,2,3"`, `"<1,2,3,4>"` | `<1, 2, 3>` | `"<1,2,3> tail"`: `ZERO_VECTOR`; the others `<1, 2, 3>` | no scripts |
| `(vector)"<1e1,0x10,-.5>"` | `<10, 16, −0.5>` | `ZERO_VECTOR` | no scripts |
| `(vector)` of `"<1,2>"`, `"<1,2,x>"`, `"<1 2 3>"`, `"<,,>"` | `ZERO_VECTOR` | the same | no scripts |
| `(rotation)` of a string with three components | `ZERO_ROTATION` | the same | no scripts |
| `if ((key)s)` | true only for a well-formed key that is not `NULL_KEY` (upper-case hex counts) | true for any string that is not empty | no scripts |
| `llGetListEntryType` of `(key)"k"` in a list literal | `TYPE_KEY` | **`TYPE_STRING`** | no scripts |
| `llCSV2List("…, c ,…")` | every element a string, leading spaces trimmed, `"c "` keeps its trailing one; quotes are not special | trailing space trimmed too | no scripts |
| `llList2CSV` of a vector, of strings with commas | `<1.000000, -0.000002, 123456.703125>`; the strings as they are | `123456.700000` | no scripts |

### Strings

`strings/functions`, `strings/unicode`. **Second Life counts characters in
code points; OpenSim in UTF-16 units**, so text outside the Basic
Multilingual Plane (emoji, `𝄞`) is one character there and two here, and
OpenSim can cut a surrogate pair in half (it then sends `U+FFFD`).

| what | Second Life | OpenSim | fake grid |
| --- | --- | --- | --- |
| `llStringLength("a𝄞b😀")`; `llSubStringIndex` of `b` in it | 4; 2 | 6; 3 | no scripts |
| `llGetSubString(…, 1, 1)`, `(…, -1, -1)`; `llDeleteSubString(…, 1, 1)` | `𝄞`, `😀`; `ab😀` | half a pair each (`U+FFFD`); `a`, `U+FFFD`, `b😀` | no scripts |
| `llChar` of `0x1F600`, `0x10FFFF` | one character | two (a surrogate pair) | no scripts |
| `llChar` of `0xD800`, `0xDFFF`, `0xFFFE`, `0xFFFF` | `U+FFFD` | `U+FFFD` for the surrogates, the character itself for `0xFFFE` and `0xFFFF` | no scripts |
| `llOrd("a😀b", 2)`, `(…, 3)` | 98, 0 | 128512 (the pair read from its second half), 98 | no scripts |
| `llHash` of `""`, `"a"`, `"abc"`, `"é"`, `"😀"`, a sentence | 0, 97, 807794786, 233, 128512, −1927828445 | the same | no scripts |
| `llToUpper("aäßéÿǅ")`, `llToLower("AÄÉŸǅİ")` | `AÄßÉŸǅ`, `aäéÿǅİ`: title case and `İ` left alone | `ǅ` → `Ǆ`, `İ` → `i`; `ß` kept on both | no scripts |
| `llStringTrim` | removes ASCII white space only — tab, line feed, vertical tab, form feed, carriage return and space; not a no-break or ideographic space | removes every Unicode white space | no scripts |
| `llStringTrim` with mode 7 | trims both ends | returns the string unchanged | no scripts |
| `llEscapeURL` | escapes everything but `A–Z a–z 0–9`: `~ - _ .` too | leaves `~ - _ .` alone (`Uri.EscapeDataString`) | no scripts |
| `llUnescapeURL("%zz%4%e9%C3%A9%")` | `""`: a bad escape empties the string | the text unchanged but for `%C3%A9` | no scripts |
| `llUnescapeURL("a+b%20c")`, lower-case hex | `a+b c`; decoded | the same | no scripts |
| `llBase64ToString` of a byte that is not UTF-8 (`/w==`), of a NUL (`YQBi`) | `?`, `a?b` | `U+FFFD`, a real NUL | no scripts |
| `llBase64ToString` of `"YWJjZA"` (padding left off), of `"a!b"` | `abcd`; `""` | `""` for both, each with `llBase64ToString: Error decoding string` on the debug channel | no scripts |
| `llBase64ToInteger` of `"AQ=="`, `"AQAAAAAA"` | 16777216 | the same | no scripts |
| `llBase64ToInteger("!!!!")` | **a different number each run** (15204949, then 12523626): not a defined value | 0 | no scripts |
| `llIntegerToBase64(-1)`, `llXorBase64` with a key shorter or longer than the text, or empty | `/////w==`; the key cycled, cut; the text unchanged | the same | no scripts |
| `llMD5String`, `llSHA1String` of `"abc"`, `"é"` (UTF-8) | the standard digests; MD5 of `"abc:5"` for nonce 5 | the same | no scripts |
| `llParseString2List("a--b-c", ["-"], ["--"])`; the same with the lists swapped | `[a, b, c]`; `[a, b, -, c]`: a separator wins over a spacer, and the earlier one in a list over a later one (`["b", "bc"]` on `abcabc` gives `[a, ca, c]`) | the same | no scripts |
| `llParseStringKeepNulls("", [","], [])` | `[""]` | the same | no scripts |
| `llListSort` of mixed types, ascending | not a total order: each type is sorted among the places it holds, and the places keep their types (`[3, "b", 1.5, "a", 2, <1,0,0>, "B", -1, <0,0,2>]` gives `[-1, "B", 1.5, "a", 2, <1,0,0>, "b", 3, <0,0,2>]`) | the same | no scripts |
| — descending | `[<0,0,2>, -1, "B", <1,0,0>, 2, "a", 1.5, "b", 3]`: the places do not keep their types | the ascending rule, descending: `[3, "b", 1.5, "a", 2, <0,0,2>, "B", -1, <1,0,0>]` | no scripts |
| `llListSort` of strings | by code unit: `"", "10", "9", "A", "B", "a", "b", "é"` | the same | no scripts |
| `llListSort` of vectors; of rotations | by length; rotations left in place | the same | no scripts |
| `llListSort` of keys and strings | sorted together, each keeping its type | the keys **become strings** | no scripts |
| `llListStatistics` of `[1, 2.5, "3", (key)"4", <1,2,3>, 4]` | counts the integers and floats only: sum 7.5, count 3 | parses the string and the key as numbers too: sum 14.5, count 5 | no scripts |
| the geometric mean of `[-1, 4]` | 0 | NaN | no scripts |
| `llListFindList` of `NULL_KEY` as a string against a key, and back | found: a key and a string match | not found | no scripts |
| `llListFindList` of `1` among `[1.0, 1]`, `1.0` among `[1, 1.0]`, `"1"` among `[1, "1"]` | 1, 1, 1: the type must match | the same | no scripts |
| `llSubStringIndex("", "")` | 0 | **−1** | no scripts |

### The newer string and list functions

`strings/newer`: OpenSim compiles none of `llReplaceSubString`,
`llList2ListSlice`, `llListFindStrided`, `llListSortStrided`,
`llComputeHash`, `llSHA256String`, `llHMAC`, `llLinear2sRGB` and
`llsRGB2Linear` (`undefined … function` under YEngine, `CS0103` under
XEngine); it has `llChar`, `llOrd` and `llHash` (above).

| what | Second Life | fake grid |
| --- | --- | --- |
| `llReplaceSubString("aaa-b-aaa", "a", "x", n)` for 0, 2, −2 | every match, the first two, the last two | no scripts |
| overlapping matches, `"aa"` in `aaa` | left to right, not overlapping: `ya-b-ya`; with −1 the last match from the right: `aaa-b-ay` | no scripts |
| `llList2ListSlice([1,a,2,b,3,c,4,d], 0, -1, 2, i)` | `i` 1 or −1: `[a, b, c, d]`; `i` past the stride: `[]`; a reversed range (5, 2) wraps: `[1, 2, c, d]` | no scripts |
| `llListFindStrided` | matches only at a stride's start, counting from `start` (a start of 1 never matches at even indices); negative indices count from the end | no scripts |
| `llListSortStrided` by a stride index of 1, −1, 0 | sorts by that element; past the stride, `[]`; an odd tail left at the end | no scripts |
| `llComputeHash` with `md5`, `sha1`, `sha224`, `sha256`, `sha384`, `sha512` | the hex digest; the name is not case sensitive; `md4` or `""` give `""` | no scripts |
| `llHMAC` | the Base64 HMAC, the same names; an unknown one `""` | no scripts |
| `llLinear2sRGB`, `llsRGB2Linear` | the standard curves, not clamped: 2 → 1.35326, −1 → −12.92 | no scripts |

### What a call costs

`vm/call-cost-lists`, `vm/call-cost-strings` and `vm/call-cost-newer` time
each function for a second and a half at four sizes — lists of integers,
strings of `"ab,"` repeated — reading the clock every ten calls, and an
empty loop of the same shape as the baseline. The tables give the
microseconds a call costs above that loop (an empty iteration is about
2.9 µs on Second Life, 0.05 µs under YEngine and 2.6 µs under XEngine).
Second Life's lists and strings were timed twice, an hour apart; where the
runs differ by more than a tenth both are given. The functions that build a
new list vary most between runs, and a rate under a hundred calls in the
run and a half (the slowest cells, 500 elements) is coarse by the batch of
ten.

| function | Second Life, 1, 10, 100, 500 elements | OpenSim YEngine | OpenSim XEngine |
| --- | --- | --- | --- |
| `llGetListLength` | 2.5, 2.5, 2.5, 2.5 | 0.00, 0.00, 0.00, 0.00 | 0.06, 0.05, 0.00, 0.00 |
| `llList2Integer` | 4.2–7.9, 4.2, 4.2, 4.3 | 0.01, 0.01, 0.01, 0.01 | 0.15, 0.00, 0.01, 0.13 |
| `llListSort` | 168–198, 336, 5288–6664, 27775 | 7.1, 7.4, 16, 69 | 0.14, 0.82, 9.6, 66 |
| `llList2CSV` | 100, 312, 2454, 12818 | 0.36, 1.5, 12, 56 | 0.65, 1.9, 14, 64 |
| `llDumpList2String` | 101, 370, 2904–13155, 19605–27024 | 0.21, 1.8, 43, 683 | 0.56, 2.6, 47, 902 |
| `llListFindList` | 50, 82, 436, 2485–2744 | 0.05, 0.39, 3.6, 18 | 0.22, 0.74, 4.6, 22 |
| `llList2List` | 160–183, 113–639, 855–974, 6533–9088 | 7.0, 7.0, 7.7, 9.7 | 0.19, 0.30, 0.83, 2.6 |
| `llDeleteSubList` | 86–786, 101–210, 1141–1887, 5205–7191 | 6.9, 7.0, 7.7, 9.7 | 0.07, 0.29, 0.86, 2.2 |
| `llListReplaceList` | 63–661, 132–171, 1678–1909, 8895 | 6.8, 7.2, 8.2, 12 | 0.04, 0.41, 1.3, 4.6 |
| `list + list` | 161–186, 197–261, 1334–1605, 7905 | 7.0, 7.0, 7.6, 9.6 | 0.21, 0.30, 0.80, 2.2 |
| `llListStatistics` | 29, 55, 398–513, 2700 | 0.25, 2.4, 23, 114 | 0.49, 3.0, 25, 133 |
| `llCSV2List` | 162, 404, 4383–4923, 18865 | 6.9, 9.4, 48, 622 | 0.28, 2.7, 47, 626 |
| `llListRandomize` | 171, 366–506, 3353–4648, 18865 | 8.1, 11, 38, 497 | 1.6, 4.3, 34, 582 |

| function | Second Life, 1, 10, 100, 1000 characters | OpenSim YEngine | OpenSim XEngine |
| --- | --- | --- | --- |
| `llStringLength` | 15, 30, 101, 676 | 0.00, 0.00, 0.00, 0.00 | 0.00, 0.01, 0.03, 0.02 |
| `llGetSubString` | 36, 49, 122, 766 | 0.03, 0.06, 0.06, 0.06 | 0.04, 0.01, 0.07, 0.12 |
| `llSubStringIndex` | 26, 41, 117, 778 | 0.07, 0.12, 0.75, 7.1 | 0.06, 0.30, 0.92, 7.7 |
| `llParseString2List` | 115–148, 453–611, 3934–4923, 19228–55553 | 7.4, 8.1, 14, 96 | 0.58, 1.2, 7.2, 96 |
| `llToUpper` | 14, 19–26, 71–86, 506–611 | 0.10, 0.18, 0.91, 8.3 | 0.07, 0.19, 1.0, 9.6 |
| `llStringToBase64` | 33, 51, 267, 2358 | 0.16, 0.22, 0.71, 5.0 | 0.25, 0.26, 0.78, 5.7 |
| `llMD5String` | 238, 239, 315, 975 | 7.0, 7.4, 7.6, 14 | 7.7, 8.2, 8.3, 17 |
| `llSHA1String` | 276, 294, 351, 873 | 2.0, 2.1, 2.5, 8.5 | 2.4, 2.6, 3.0, 9.8 |
| `llEscapeURL` | 72, 146, 837–1045, 7191–9256 | 0.05, 0.42, 2.2, 30 | 0.09, 0.50, 2.4, 32 |
| `string + string` | 9.4–11, 11, 34–38, 211 | 0.07, 0.08, 0.16, 0.94 | 0.20, 0.13, 0.29, 1.3 |
| `llInsertString` | 53, 99, 470, 3800 | 0.10, 0.11, 0.19, 0.96 | 0.09, 0.21, 0.37, 1.3 |
| `llDeleteSubString` | 33, 79, 460, 3958 | 0.02, 0.08, 0.16, 0.93 | 0.13, 0.11, 0.18, 1.3 |
| `llStringTrim` | 38, 76, 470, 4006 | 0.07, 0.07, 0.07, 0.07 | 0.01, 0.08, 0.08, 0.19 |

| function | Second Life, 1, 10, 100, 1000 characters | OpenSim |
| --- | --- | --- |
| `llReplaceSubString` | 60, 122, 576, 4648 | does not compile |
| `llSHA256String` | 383, 382, 434, 954 | does not compile |
| `llComputeHash` | 200, 196, 274, 1150 | does not compile |
| `llOrd` | 16, 33, 110, 742 | does not compile |
| `llHash` | 15, 33, 120, 907 | does not compile |

What the numbers say: `llGetListLength` (2.5 µs) and `llList2Integer`
(4.2 µs) are flat on every grid, so a list is indexed, not walked. On Second
Life **every string function grows with the string, `llStringLength` and
`llGetSubString` included** — about 0.65 µs a character; OpenSim answers
both in constant time. Copying a list costs 13 to 18 µs an element there,
writing one as text (`llList2CSV`) 25, sorting 55 (`llListSort` grows about
linearly between 100 and 500 elements); the hashes cost a flat 200–380 µs
before their per-character part. OpenSim is 10 to 1000 times cheaper
throughout, and its list copies are constant under YEngine at about 7 µs.

### How many scripts a region runs

`vm/region-budget` (2026-10-29) rezzes 1, 16, 64, 128 and 256 copies of an
object whose script runs an empty loop for five seconds on a word from the
probe, all at once, and collects each one's rate, in iterations a second of
its `llGetTime` and of real time (`llGetTimestamp`). The runner notes the
region's statistics meanwhile. Mean rate per script, and the total:

| scripts | Second Life | OpenSim YEngine | OpenSim XEngine |
| --- | --- | --- | --- |
| 1 | 469 680 | 28 548 380 | 436 880 |
| 16 | 476 508 (7.6 M) | 3 932 049 (63 M) | 372 509, 11 of 16 reported |
| 64 | 478 536 (30.6 M) | 1 214 680 (78 M) | 336 977, 28 of 64 |
| 128 | 477 162 (61.1 M) | 728 889 (93 M) | 284 475, 45 of 128 |
| 256 | 241 793 (61.9 M) | 471 959 (108 M), 229 of 256 | not reached in five minutes |

On Second Life every script keeps the rate one runs at alone up to 128 of
them, and 256 share what 128 used: the region runs about 130 scripts' worth
(134 in a run an hour earlier, 64.7 M against 481 455), on top of the 2 000
mostly idle scripts Mauve holds. The region's script time rises with them to
23.9 ms, one frame at 45 frames a second, and with 256 its scripts-run
percentage falls to about half (53 at the lowest). The two clocks agree
throughout (their ratio 0.99 to 1.002) while the region's time dilation dips
to 0.77: a script in a crowded region sees its loop slow, never its clock.
OpenSim divides its engine's threads among the busy scripts instead, so a
script's rate falls as soon as there are more of them than threads while the
total grows slowly; under XEngine fewer than half the workers report past
sixteen, and the probe's five minutes run out while it rezzes the 256.

## Task inventory, giving, rezzing and notecards

Measured on 2026-10-27 by the probes `inventory/listing`,
`inventory/notecards`, `inventory/rez`, `inventory/give` and
`inventory/newer`, on the same grids. The runner puts what a probe reads into
prim A before its scripts are saved: each notecard made in the owner's
inventory, saved with its text and dropped into the prim, and each object a
cube with its scripts saved in it running, taken and dropped into the prim
([The LSL probe corpus](../tools/lsl-probes.md)). A rezzed child reports on a
channel, since only the probe's own prims' owner-says are kept, and dies.
YEngine and XEngine agree but where a row says otherwise.

### Reading the inventory

| what | Second Life | OpenSim | fake grid |
| --- | --- | --- | --- |
| the order of `llGetInventoryName` | by name, case folded, **digits as numbers**: `9`, `10`, `_under`, `aaa Helper`, `alpha`, `Beta`, `Box`, `Probe`, `Zed` | by name, case folded, culture order: `_under`, `10`, `9`, `aaa Helper`, `alpha`, … | no scripts |
| a name made with an accent (`élan`, `CreateInventoryItem` over UDP) | the accented character dropped: `lan` | **a hex dump of the bytes**: `C3 A9 6C 61 6E 00 …..lan.` | no scripts |
| a name in another case (`ALPHA`) | not found: the lookup is exact | not found | no scripts |
| `llGetInventoryName` of −1, past the end, of type `INVENTORY_ALL` at 100, of a type that is none | `""` | `""`, but −1 **throws** (`ArgumentOutOfRangeException`; YEngine stops the script) | no scripts |
| a missing name's type, key, creator, masks | −1, `NULL_KEY`, `NULL_KEY`, 0 each; `No item named '…'.` for every call but the type, so six lines for one name and then the flood limit | −1, `NULL_KEY`, `""`, −1 each; one line, `llGetInventoryCreator: Can't find item ''` | no scripts |
| key, creator, masks of the owner's full-permission items | the asset key, the owner, base and owner `0x7FFFFFFF`, group and everyone 0, next owner `0x7FFFFFFF`; the object's next-owner mask `532480` (move, modify, copy, transfer) | the same | no scripts |
| `llSetInventoryPermMask` | does nothing, says nothing | does nothing, says nothing | no scripts |
| `llScriptDanger` at the prim | 1 (Mauve) | 0 | no scripts |
| a script that is not there: `llGetScriptState`, `llResetOtherScript`, `llSetScriptState` | 0, and silent here only because the missing item's six lines above had set off the object's [flood limit](#floods); alone, `Could not find script 'nope'.` ([Run-time errors](#what-is-said-and-to-whom)) | 0, and `llGetScriptState: Can't find script 'nope'` (and the like) on the debug channel | no scripts |
| a link message to a stopped script | lost | lost | no scripts |
| `llResetOtherScript` of a stopped script, then `llSetScriptState(…, TRUE)` | stays stopped through the reset; started, it **carries on without `state_entry`** | stays stopped; started, it runs `state_entry` | no scripts |
| `llResetOtherScript` of a running script with a link message queued | the message is lost with the reset | YEngine delivers it first, then the reset; XEngine loses it | no scripts |
| `llRemoveInventory` | removes, **no `changed`** in the prim; a missing name is silent | removes and raises `changed(CHANGED_INVENTORY)`; a missing name is silent | no scripts |

### Notecards

| what | Second Life | OpenSim | fake grid |
| --- | --- | --- | --- |
| `llGetNumberOfNotecardLines` of ten lines and a final newline | **11**: the newline starts an empty last line | 10 | no scripts |
| of an empty notecard | 1, line 0 `""` | 0, line 0 `EOF` | no scripts |
| line −1 | `EOF` | `""` | no scripts |
| a line's limit | **1024 bytes**, cut at a character: 1100 ASCII → 1024 characters, 600 two-byte → 512 | **255 characters** whatever their bytes | no scripts |
| a line with an emoji | counts it one character | two (UTF-16) | no scripts |
| spaces at both ends, a tab | kept | kept | no scripts |
| by the asset key (`llGetInventoryKey`) | read | read | no scripts |
| a notecard that is not there, a script, `""` | `Could not find notecard '…'.`; the key is a request that never answers | `llGetNotecardLine: Can't find notecard '…'`, `NULL_KEY`, and the call sleeps 1 s | no scripts |
| the call's forced delay, the answer | 0.1 s; the answer one frame later | 0.1 s; at once | no scripts |
| ten requests at once | answered in order | answered in order | no scripts |
| `llGetNotecardLineSync` | `NAK` before the notecard was read, its lines after; −1 `EOF`, past the end `""`; a missing notecard `NAK` and an error | does not compile | no scripts |

### Rezzing

| what | Second Life | OpenSim | fake grid |
| --- | --- | --- | --- |
| the order | the rezzer's `object_rez` **arrives first**, though the child's `on_rez` ran before it by the clock | the same | no scripts |
| the child's script | `on_rez` only, the state as it was taken | YEngine `on_rez` only; **XEngine runs `state_entry` too**, before `on_rez` (it resets the script) | no scripts |
| start parameter, `llGetStartParameter`, `OBJECT_REZZER_KEY` | as passed, both; the rezzer | the same | no scripts |
| `llRezAtRoot` of one prim | as `llRezObject` | the same | no scripts |
| a rotation that is not unit length | normalised | normalised | no scripts |
| a velocity, the child not physical | 0 | 0 | no scripts |
| `STATUS_DIE_AT_EDGE` of the child | 0 | **1** | no scripts |
| 11 m from the rezzer | nothing, silently | nothing, silently, and no forced delay | no scripts |
| a name that is not there | `Could not find object 'Nope'.`, 1.1 s | `llRez(AtRoot/Object): Can't find object 'Nope'`, 1 s | no scripts |
| a notecard's name | nothing, silently | `Can't create requested object; object is missing from database`, 1 s | no scripts |
| the forced delay | 0.1–0.2 s (frame-bound) | 0.1 s | no scripts |
| twenty in a row | all twenty rezzed, in 3.1 s; no throttle | all twenty, in 2 s | no scripts |
| `llRezObjectWithParams` | the start parameter and string arrive; a 1100-byte string arrives as 1018 characters; an unknown rule is `Error running rule #2: unknown rule id 999.` and rezzes nothing | does not compile | no scripts |
| `llDerezObject(child, DEREZ_DIE)` | 1, then 0 for the same child and for `NULL_KEY` | does not compile | no scripts |
| `llGetInventoryDesc`, `llGetInventoryAcquireTime` | the item's description (`(No Description)` for a taken object, `""` for a script); `YYYY-MM-DDThh:mm:ssZ`; a missing name `""` and an error | do not compile | no scripts |

### Giving

| what | Second Life | OpenSim | fake grid |
| --- | --- | --- | --- |
| a notecard to a prim of the same owner | arrives; `changed(CHANGED_INVENTORY)` there | the same | no scripts |
| a name the prim holds already | arrives as `Gift 1` (then `Gift 2`) | the same | no scripts |
| a script | arrives **not running** | the same | no scripts |
| a name that is not there | `Unable to give inventory: 'No item named 'Nope'.'.` | `llGiveInventory: Can't find inventory object 'Nope'`, 1 s | no scripts |
| to an avatar | an offer (`TaskInventoryOffered`, dialog 9) under the object owner's id, with the object's name, the item and the place, marked **offline**; 2 s | the same, the place as region and position; 3 s | no scripts |
| `llGiveInventoryList` to an avatar | an offer of a folder, not offline; 3 s | the same; 3 s | no scripts |
| `llGiveInventoryList` to a prim | the items arrive one by one, no folder (`Gift 2`); 3 s | the same; 0.1 s | no scripts |
| `llRemoteLoadScriptPin` with the pin | the script arrives and runs with its start parameter; 3 s | the same | no scripts |
| with a wrong pin | `Task 'Object' trying to illegally load script onto task 'Receiver'!`; 3 s | nothing, silently; 3 s | no scripts |
| not running, over a script of that name | replaces it, stopped | YEngine stopped; XEngine **reads as running**, though it prints nothing | no scripts |

### What the viewer does with it

An object's offer is an instant message of dialog 9 on both grids, sent
under the object owner's id and carrying the object's name. The viewer's
offer card takes it (`sl-viewer-people`'s `offers_invites`), whether it is
marked offline or not. A name with more than ASCII is not kept as typed
when an item is made over UDP `CreateInventoryItem`, the message the
reference viewer makes items with too; whether such a name survives a rename
(`UpdateInventoryItem`, or AIS on Second Life) is not measured.

## Forced delays and throttles

Measured on 2026-10-28 by the probes `throttle/delays`, `throttle/email`,
`throttle/rez`, `throttle/cast-ray`, `throttle/chat` and
`throttle/http-window`, on the same grids. YEngine and XEngine agree on
every row but the raycast rate, so they share a column. The local OpenSim
runs with `ScriptDelayFactor` 1.0, the default, which scales every sleep in
the OpenSim column. Each delay is the time the call itself took, by
`llGetTime` before and after, the mean of three calls; on Second Life that
clock moves in frames (about 0.022 s), so a delay there reads up to a frame
over its value.

### Forced delays

The documented value is the `sleep` of Linden Lab's keyword file
(`keywords_lsl_default.xml`), which `sl-lsl-runtime` charges after every
call today. Each call set what the prim already had, or asked about the
owner or the region; none of the land calls had the land's rights on
either grid.

| call | documented | Second Life | OpenSim | fake grid |
| --- | --- | --- | --- | --- |
| `llRegionSay`, `llSetColor`, `llSetScale`, `llSetLinkPrimitiveParamsFast` | 0 | 0 (a frame once) | 0 | no scripts |
| `llSetPos`, `llSetRot`, `llSetLocalRot` | 0.2 | 0.2–0.22 | 0.2 | no scripts |
| `llSetTexture`, `llSetLinkTexture`, `llScaleTexture`, `llOffsetTexture`, `llRotateTexture` | 0.2 | 0.2–0.22; one `llSetTexture` took 0.47 | 0.2 | no scripts |
| `llSetPrimitiveParams`, `llSetLinkPrimitiveParams` | 0.2 | 0.2–0.22 | 0.2 | no scripts |
| `llSetRemoteScriptAccessPin` | 0.2 | 0.2 | **0** | no scripts |
| `llAdjustSoundVolume`, `llRequestAgentData` | 0.1 | 0.11 | 0.1 | no scripts |
| `llXorBase64Strings` | 0.3 | 0.31 | 0.3 | no scripts |
| `llGetRegionFlags`, `llRequestUsername`, `llRequestDisplayName` | 0 | 0 | 0 | no scripts |
| `llModPow` | 0 | 0 | **1.0** | no scripts |
| `llRequestSimulatorData` | 1.0 | 1.0 | 1.0 | no scripts |
| `llGetParcelPrimOwners`, without the land's rights | 2.0 | 2.0 | 2.0 | no scripts |
| `llParcelMediaQuery`, without the land's rights | 2.0 | 2.0 | **0**: it returns before the sleep | no scripts |
| `llGetPrimMediaParams`, `llClearPrimMedia` | 1.0 | 1.0 | 1.0 | no scripts |
| `llMapDestination`, outside a touch and not worn (it does nothing) | 1.0 | 1.0 | **0**: it returns before the sleep | no scripts |
| `llInstantMessage` | 2.0 | 2.0 | 2.0 | no scripts |
| `llDialog` | 1.0 | 1.0 | 1.0 | no scripts |
| `llTextBox` | 1.0 | **0** | 1.0 | no scripts |
| `llLoadURL` | 0.1 | 0.11 | **10.0** | no scripts |
| `llEmail` | 20.0 | 20.0 | 1.0: the email module is off (below) | no scripts |
| `llRezObject` ([Rezzing](#rezzing)) | 0.1 | 0.1–0.2 | 0.1 | no scripts |
| `llGiveInventory` to an avatar ([Giving](#giving)) | 0 | 2 | 3 | no scripts |

**OpenSim sleeps a script for a second whenever a library call reports an
error.** Its `LSL_Api.Error`, which every such message goes through, ends
in `Sleep(1000)`. That is the 1 s beside each OpenSim error in the tables
above (a notecard, an object or an item that is not there), and the 1 s of
its `llEmail` here; `llGetNextEmail` costs the same second each time it is
called. Second Life's error messages cost nothing but the call's own delay.

### Email

`throttle/email`: prim A sends prim B two emails, one to
`<key>@lsl.secondlife.com` and one to the grid's own domain for objects;
prim B asks for its mail every two seconds.

| what | Second Life | OpenSim | fake grid |
| --- | --- | --- | --- |
| the forced delay | 20.0 s | 1 s, the error's (`llEmail: Email module not configured`; the module is off by default) | no scripts |
| to `<key>@lsl.aditi.lindenlab.com` | arrives about 2 s after the send, while the sender still sleeps | — | no scripts |
| to `<key>@lsl.secondlife.com` | never arrives (not within 80 s): from the beta grid that is the main grid's domain | — | no scripts |
| the `email` event | the sender `<sender's key>@lsl.aditi.lindenlab.com`; the body six lines, three of headers (`Object-Name: Object`, the region, the position), a blank line, then the message; nothing more waiting | `llGetNextEmail: Email module not configured` | no scripts |

### Rezzing without pause

`throttle/rez`: six waves of fifty `llRezObject` calls in a row, three
seconds apart, of an object that dies as it is rezzed.

| what | Second Life | OpenSim | fake grid |
| --- | --- | --- | --- |
| 300 calls | all 300 rezzed; **no throttle** | all 300 | no scripts |
| the rate | about 6.3 a second (7.8–8.0 s a wave); the slowest call 0.4 s | 10 a second, 0.1 s each | no scripts |

### Raycasts without pause

`throttle/cast-ray`: `llCastRay` in a loop for ten seconds, twice. The cheap
ray is a metre long where nothing is; the expensive one crosses the region
corner to corner at prim A's height, asking for 256 hits and phantoms.

| what | Second Life | OpenSim YEngine | OpenSim XEngine | fake grid |
| --- | --- | --- | --- | --- |
| the cheap ray | 11 552 rays, 1090 a second; **no `RCERR_*`** | 136, 14 a second | 317, 32 a second | no scripts |
| the expensive ray | 454 rays, 45 a second; no `RCERR_*` | 142, 14 a second | 283, 28 a second | no scripts |

### Chat without pause

`throttle/chat`: prim A says to prim B, five metres east, where four
listener scripts share the counting so no event queue fills. Channel 0
first, then for each of `llRegionSay`, `llSay` and `llRegionSayTo` (prim B)
a burst of 200 in a row and ten waves of 40 a second apart.

| what | Second Life | OpenSim | fake grid |
| --- | --- | --- | --- |
| 25 `llSay` on channel 0 | all heard, in 0.33 s; no call sleeps | all heard, in 4 s: **the 12th and the 24th call sleep 2 s** — more than eleven lines on channel 0 within a second (`llSay`, `llShout`) | no scripts |
| `llRegionSay`, `llSay`, `llRegionSayTo` on other channels, 600 each | all heard; **no throttle**; a call takes at most a frame | all heard; no throttle; no call sleeps | no scripts |

### HTTP requests: how the throttle lets go

[The throttle](#the-throttle) above measured the burst each grid accepts.
`throttle/http-window` measures what comes after: one request every half
second for forty seconds after the burst, then one after each of a series
of silences, and a burst after the first that is accepted.

| what | Second Life | OpenSim | fake grid |
| --- | --- | --- | --- |
| the burst | 25 accepted | 4 | no scripts |
| a request every 0.5 s for 40 s after it | **every one refused**: a refused request counts as one, so asking keeps the throttle shut | every second one accepted: one a second | no scripts |
| silence, then one request | refused after 5, 10 and 15 s of silence, **accepted after 20** | accepted after 5 s | no scripts |
| a burst after the silence | 25 again (the one and 24 more): the whole allowance | 4 | no scripts |

So Second Life's limit is 25 requests in 20 seconds, counting the refused
ones, and a script that retries at once stays shut out for as long as it
keeps trying. OpenSim's is a bucket of 3 that fills by one a second
(`PrimRequestsBurst`, `PrimRequestsPerSec`), which refusals do not drain.

### What the viewer does with it

Nothing here reaches a viewer but what the calls themselves send. Each
`llInstantMessage`, `llDialog` and `llTextBox` of `throttle/delays` reached
the owner once per call on both grids — three dialogs for three calls, none
held back — and the viewer shows a dialog or a text box as any other
(`sl-viewer-notices`' `script_dialog`), and an `llLoadURL` as its prompt
(`load_url`). The 25 channel-0 lines arrived as 25 lines of object chat on
both grids: neither grid holds a flood back, so collapsing one is the
viewer's to do (the roadmap's `viewer-anti-spam-filter`).

## Time, timers and sleep

Measured on 2026-10-28 by the probes `time/timer-floor`, `time/timer-phase`,
`time/timer-rearm`, `time/clocks`, `time/sleep-lengths`,
`time/sleep-negative` and `time/reset-mid-sleep`, on the same grids, beside
the corpus's first `time/get-time-frames`, `time/sleep-while-stopped` and
`events/timer-no-stack` ([The queue, the timer and a state
change](#the-queue-the-timer-and-a-state-change)). A probe times a timer by
`llGetTime`, which on Second Life moves in frames of 0.0219–0.0227 s, and a
sleep by the microseconds of `llGetTimestamp` as well. The local OpenSim
runs with the default `MinTimerInterval` of each engine: 0.1 s for YEngine
(`OpenSimDefaults.ini`), 0.5 s for XEngine (unset there, so `LSL_Api`'s
own).

### The timer's floor

`time/timer-floor`: each interval for three seconds, then -1.0.

| asked | Second Life | OpenSim YEngine | OpenSim XEngine | fake grid |
| --- | --- | --- | --- | --- |
| 0.001, 0.01, 0.02 s | an event **every second frame**, 0.044 s (68 in three seconds); the first a frame or two after the set | every 0.1 s | every 0.5 s | no scripts |
| 0.05 s | every two or three frames, 0.050 s on average: the rate is kept to the frame | every 0.1 s | every 0.5 s | no scripts |
| 0.1, 0.2, 0.5 s | the interval, each event on a frame (0.089–0.112 s apart for 0.1) | the interval | 0.5 s for each | no scripts |
| -1.0 s | **no event**: a negative interval stops the timer, as 0 does | **the floor**, every 0.1 s | the floor, every 0.5 s | no scripts |

### Drift and phase

`time/timer-phase`: ten events of a 1.0 s timer, then eight of a 0.5 s
timer whose handler sleeps 0.3 s, then 0.7 s.

| what | Second Life | OpenSim YEngine | OpenSim XEngine | fake grid |
| --- | --- | --- | --- | --- |
| 1.0 s, ten events | the tenth at 10.009 s: **the phase is kept**, each event on the frame nearest its due time | the tenth at 10.084 s: each gap 1.002–1.010 s, **the error adds up** — the next event is counted from when this one ran | the first at 1.077 s, then every 1.000 s: the phase of the first is kept | no scripts |
| 0.5 s, the handler sleeping 0.3 s | events at 0.5 s steps (gaps 0.489 and 0.511 by turns): **the phase is kept** | gaps of 0.501–0.516 s, adding up | gaps of 0.500 s | no scripts |
| 0.5 s, the handler sleeping 0.7 s | **0.755 s apart**: the next event two or three frames after the handler ends, not on the next 0.5 s step | 0.701–0.736 s apart: as the handler ends | 0.700 s apart: as the handler ends | no scripts |

A timer never stacks on any grid (`events/timer-no-stack`): a handler longer
than the interval delays the next event, and no event is made up for later.

### Setting the timer again

`time/timer-rearm`: when the first event comes, counted from the first set.

| case | Second Life | OpenSim (both engines) | fake grid |
| --- | --- | --- | --- |
| 2.0, and 2.0 again a second in | at 3.0: **setting it again restarts the count** | the same | no scripts |
| 2.0, and 1.5 a second in | at 2.56 | at 2.50–2.52 | no scripts |
| 1.0, and 2.0 half a second in | at 2.51 | the same | no scripts |
| 1.0, 0.0 and 1.0 at once | at 1.0 | the same | no scripts |
| 1.0, a 2 s sleep in which it falls due, then 3.0 | at 2.0: **the event already due still runs**, the new set does not drop it | the same | no scripts |
| 1.0, the same sleep, then 0.0 | **no event**: stopping the timer drops the one already due | YEngine the same; **XEngine runs it** | no scripts |

### The clocks

`time/clocks`, one reading of each clock in one event.

| clock | Second Life | OpenSim | fake grid |
| --- | --- | --- | --- |
| `llGetTimestamp` | `2026-10-28T09:27:36.407385Z`: 27 characters, **six** fraction digits | `2026-10-28T08:53:45.6802820Z`: 28 characters, **seven** (.NET's `fffffff`) | no scripts |
| `llGetDate` | the timestamp's date, UTC | the same | no scripts |
| `llGetUnixTime` | the timestamp's second | the same | no scripts |
| `llGetGMTclock` | the second of the UTC day, **a whole number** (`34056.000000`) | the same second **with its fraction** | no scripts |
| `llGetWallclock` | the second of the day in **Pacific time** (7 h behind UTC in October's daylight time), a whole number | the second of the day in **the simulator host's own time zone** (here an hour ahead of UTC), with its fraction | no scripts |
| `llGetTimeOfDay` | the UTC second of the day modulo four hours, to the millisecond (`5256.452` at `34056`) | the host's local second of the day modulo four hours | no scripts |
| `llGetAndResetTime` | the time since the last reset (0.511 s after a 0.5 s sleep), and `llGetTime` after it exactly 0 | the same (0.500–0.501 s) | no scripts |
| `llGetTime` against the timestamp, five busy seconds | 4.956 s against 5.0005 s: two frames behind, with `llGetRegionTimeDilation` 1.0 and `llGetRegionFPS` 45 | equal to the millisecond; `llGetRegionFPS` 55 | no scripts |

`llGetTime` itself counts in frames on Second Life and in milliseconds on
OpenSim (`time/get-time-frames`; OpenSim rounds it to three places).

### How long a sleep is

`time/sleep-lengths` and `time/sleep-negative`, five sleeps of each length,
by the timestamp.

| asked | Second Life | OpenSim | fake grid |
| --- | --- | --- | --- |
| 0 | no wait, mostly (0.003 ms); one of five took a frame | no wait (0.02–0.06 ms) | no scripts |
| 0.001, 0.01, 0.02 s | **one frame**, 22 ms | the time asked, plus 0.03–0.1 ms | no scripts |
| 0.05, 0.1, 0.25 s | **rounded up to whole frames**: 66.8 ms (three), 111 ms (five), 267 ms (twelve) | the time asked | no scripts |
| -1.0 s | no wait; the event goes on | YEngine no wait; **XEngine throws** an `ArgumentOutOfRangeException` from its `WaitHandle.WaitOne`, shouted on `DEBUG_CHANNEL` as a stack trace (heard by the other avatar too), which ends the event; the script runs on and throws again at the next | no scripts |

### Stopping and resetting a script that sleeps

`time/reset-mid-sleep`, with `time/sleep-while-stopped`: a five-second sleep
interrupted one second in.

| what | Second Life | OpenSim YEngine | OpenSim XEngine | fake grid |
| --- | --- | --- | --- | --- |
| `llResetOtherScript` mid-sleep | **at once**: `state_entry` within a frame of the reset, the sleep abandoned | the same | the same | no scripts |
| stopped a second in, restarted a second later, pinged a second after that | the sleep runs to its end (5 s), then the ping is answered | the same | **the sleeping event is abandoned**: it never wakes, and the ping is answered at once (3 s) | no scripts |
| reset after that | `state_entry` at once, and the next ping answered | the same | the same | no scripts |
| `llGetTime` of a script stopped three seconds and restarted | 5.0 a second after the restart: **a stopped script's clock runs on** | the same | the same | no scripts |
| a sleep stopped and restarted after it would have ended (`time/sleep-while-stopped`) | wakes within a frame of the restart | the same | never wakes | no scripts |

### What the viewer does with it

Nothing: a timer and a sleep send the viewer nothing. The one thing a viewer
sees is XEngine's negative-sleep stack trace, which arrives as an object's
`DEBUG_CHANNEL` chat like any other script error ([Run-time
errors](#run-time-errors)).
