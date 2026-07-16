---
id: viewer-input-focus-contexts
title: Input focus / modal context state machine
topic: viewer
status: done
origin: reference-viewer feature-cluster survey (2026-07); split from viewer-input-system
blocked_by: [viewer-ui-widget-scaffold]
---

Context: [context/viewer.md](../context/viewer.md).

The "input focus" spine: a modal input-context state machine plus
focus-ownership routing via `bevy_input_focus`, deciding who receives input each
frame — the world, a UI widget, or a text field. Includes
**cursor-grab toggling** (the viewer today is unconditionally
`CursorGrabMode::Locked`, which is why no UI mouse interaction is possible); a
UI/text context releases the grab, a world/mouselook context takes it.

The context set covers **both** UI-focus states (Chat / TextEntry / Edit)
**and** the movement/camera modes (third-person / mouselook / sitting), which
mirror Firestorm's `keys.xml` **modes** — because a key binding is scoped to
whichever context is active ([[viewer-input-action-map]] holds the per-context
profiles). So this task owns *which context is active and who has focus*; the
action map owns *what a key does in that context*.

Reference (Firestorm, read-only): `indra/llwindow/llkeyboard` (focus/mode),
`llagentcamera` mode transitions, `llviewerwindow` focus handling.

Builds on: the current always-grabbed cursor in `main.rs`.

## Done

`src/input_context.rs`. Brought forward and done **with**
[[viewer-ui-widget-scaffold]] rather than after it: the scaffold made focus
reachable, which immediately made the bug visible — every key typed into a
focused text field *also* drove the avatar, because all fourteen keyboard
readers consult `ButtonInput<KeyCode>` directly and nothing had ever told them
not to.

`InputContext` (`World` / `UiWidget` / `TextEntry`) is **derived** from
`InputFocus` each frame, never assigned: focus is the state, and a second copy
would drift. A focused non-`Node` entity — the primary window, which is what
`bevy_input_focus` parks focus on — reads as `World` for free, so no "is the UI
open" flag is needed. `world_has_keyboard` is a run condition on every system
that reads a key the UI could want; that is all of them except the
`F3`/`F4`/`F5` overlay toggles, per the reference's rule in
`LLViewerWindow::handleKey` (a focused control takes the keystroke unless it is
an accelerator or a non-character key). **The arrows are in the gated set** —
they move a caret in a field and turn the avatar in the world, and ours did both
at once. `Escape` releases focus, deliberately not a letter: any letter binding
is unusable as an escape hatch, because a text field rightly eats it.

The cursor grab is now `allowed && world && !hud_cursor`, one system owning the
write; `hud_pick`'s `H` toggle only flips its own flag now. `CursorGrabAllowed`
is false in screenshot mode — without it the context would helpfully re-grab an
unattended run's pointer, which is exactly what the old "write the grab only on
the toggle frame" hack existed to avoid.

**Deliberately not modelled: the `keys.xml` modes** (`MODE_FIRST_PERSON` /
`THIRD_PERSON` / `EDIT_AVATAR` / `SITTING`). They scope what a key does when the
world *does* have the keyboard, and mouselook ([[viewer-camera-mouselook]]) and
sit/stand ([[viewer-sit-stand-actions]]) are the tasks that would give them a
second value to take; an enum with one inhabitant is not a state machine.
`InputContext::World` is the seam they subdivide.

**The cursor half is a waypoint, not the destination.** Second Life grabs the
cursor in **mouselook and nowhere else** — in third person the pointer is free
and clicks the world. So the grab properly keys off the *camera mode*, not off
the input context. `World` takes it today only because the sole camera we have
is the debug fly-camera, which is permanently mouselook-shaped (it steers from
raw motion and cannot work with a free pointer). When
[[viewer-camera-mouselook]] / [[viewer-camera-third-person-orbit]] land, the
condition becomes "the camera is in mouselook", third person joins the UI on the
free-cursor side, and `hud_pick`'s `H` toggle should be
**deleted rather than ported** — it exists only to escape a grab third person
will not have.
