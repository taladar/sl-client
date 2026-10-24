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
