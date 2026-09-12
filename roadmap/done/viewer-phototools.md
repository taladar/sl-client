---
id: viewer-phototools
title: Phototools — a photographer's environment & graphics control panel
topic: viewer
status: done
origin: user request (2026-07)
blocked_by: [viewer-ui-widget-scaffold]
refs: [viewer-preferences-floater, viewer-quick-preferences, viewer-snapshot-floater, viewer-camera-third-person-orbit, viewer-depth-of-field, viewer-glow-bloom, viewer-screen-space-reflections, viewer-realtime-mirrors, viewer-projector-lights-textured, viewer-pbr-terrain, viewer-antialiasing-post, viewer-avatar-impostors-billboard, viewer-ambient-occlusion, viewer-tonemap-auto-exposure]
---

Context: [context/viewer.md](../context/viewer.md).

A single control panel that gathers everything an SL photographer tweaks to get
a shot — **environment** (time of day, sky / water look) and **graphics
quality** (shadows, depth of field, exposure, ambient occlusion, lighting) — so
they can dial in the image without diving through Preferences or the environment
editor between every frame. Firestorm's Phototools floater is exactly this, and
it is telling that it is the **largest single XUI layout in the whole viewer**
(~5000 lines): photographers live in it. It is a floater
([[viewer-ui-widget-scaffold]]).

Two halves:

- **Personal environment override.** Force a local time of day / sky / water —
  midday, sunset, a custom preset — regardless of what the region sends, and
  scrub the sun freely. The environment *rendering* already exists (`sky.rs`,
  the P22 day-cycle and EEP ingest); the **local override** layer is now its
  own task, [[viewer-environment-personal-lighting]] — this floater's
  environment half is quick access to it (full EEP asset authoring is
  [[viewer-environment-fixed-editor]] and siblings). The reference's exodus
  **vignette** post effect belongs in the graphics half here.
- **Graphics quick-toggles.** The render knobs that change the *look*, surfaced
  live: shadows (P24), the reflection probes (P33), exposure / the tone mapper
  (P33.3), point-light limits, draw distance, and the render-quality tiers —
  each bound to the same settings store everything else uses, so a change here
  is the same change Preferences would make. Several of the knobs Phototools
  exposes are render features **we have not built yet** — this floater is
  effectively Firestorm's own catalogue of them, and the gap analysis (2026-07)
  turned each into its own task: [[viewer-depth-of-field]],
  [[viewer-glow-bloom]], [[viewer-screen-space-reflections]],
  [[viewer-realtime-mirrors]], [[viewer-projector-lights-textured]],
  [[viewer-pbr-terrain]], [[viewer-antialiasing-post]],
  [[viewer-avatar-impostors-billboard]], [[viewer-ambient-occlusion]] and
  [[viewer-tonemap-auto-exposure]]. Phototools *surfaces* them; it does not
  block on them (it exposes whatever exists).

This is deliberately a
**sibling of [[viewer-quick-preferences]], not a duplicate**: quick-prefs is the
general "settings I reach for often" panel; Phototools is the
*photography preset* of that idea, plus the environment override, plus a layout
tuned for composing a shot. Build it as a curated view over the typed settings
store ([[viewer-preferences-floater]]) rather than a parallel pile of controls,
so the two share plumbing. It pairs naturally with the snapshot floater
([[viewer-snapshot-floater]]) — set the look here, capture there — and with
[[viewer-camera-third-person-orbit]] for the framing.

Reference (Firestorm, read-only): `floater_phototools.xml` (the layout),
`fsfloaterphototools`, and the environment / EEP panels
(`llfloatereditextdaycycle`, `llpanelenvironment`).

## Done

`sl-viewer-preferences::phototools` is the window: four tabs — **Environment**,
**Shadows**, **Look**, **General** — on a block-start strip, narrow and tall
(400×620) so it lives down one side of the screen while the shot is composed in
the rest of it. World ▸ **Photo and Video** ▸ **Phototools…** opens it, on the
reference's own `Alt+P`; the submenu is where the reference keeps the
photographer's windows, and the camera window
([[viewer-camera-controls-window]]) and the depth-of-field toggles join it there
once they exist.

**The reference says out loud what this task guessed.** Firestorm's Phototools
is not a class of its own: `LLFloaterReg::add("phototools", …)` builds a
`FloaterQuickPrefs`, the *same* C++ class as Quick Preferences, which asks
`getName() == "phototools"` in eight places. So the architecture the task
prescribed — a second curated view over the store, not a second implementation —
is the reference's, not an improvement on it.

**Every graphics row is one row of one static table**: a Fluent label key, the
element id, the setting, the scope an edit writes to, and which control draws
it. The build walks it, and so do nine tests — a row naming a setting the store
does not declare, or drawing a checkbox over a number, fails a test rather than
quietly rendering a control that does nothing. The runtime guard is still there
behind the test, because the mistake it catches (a setting renamed in its own
crate) can arrive from outside this file.

**The labels and the option lists are the graphics tab's own.** *Shadow detail*
is one setting with one name and one set of levels, whichever window it was
reached through, so the rows reuse the `preferences-row-*` keys and
`preferences_graphics` grew seven `…_options()` functions that both surfaces
call. A second spelling would be a second place for a level to be added to, and
the window that missed it would bind a value nothing matches. Only the window's
own chrome — four tab labels and four environment strings — is new Fluent.

**The quality tier is not re-implemented either**: the combo's anchor carries
`QualityTierControl`, the same marker the graphics tab puts on its own, so one
applier writes the tier from either surface.

**What the rows must not do is join the Preferences search.** The shell's
`spawn_pref_*` helpers look like exactly the right thing to call, and calling
them would have been a bug: `apply_preferences_filter` queries **every**
`PrefSearchRow` in the world and writes its `Node::display`, so a term typed in
Preferences would have hidden rows in this window. Phototools spawns its own
rows, as quick-prefs does.

**The environment tab is not settings.** It drives the live `EnvironmentState`
the World ▸ Environment menu drives — a preset-library combo (region day cycle /
legacy WindLight / modern EEP), the four times of day as buttons, **Shared
Environment**, and **Personal Lighting…** — so a pin made here is the pin the
menu shows a check mark for, down to honouring
`EnvironmentRepeatedTogglesShared`. The time button whose environment is in
force is lit, and a local *settings asset* — a sky from inventory, which this
tab cannot describe — lights none of them, which is the honest answer rather
than a wrong one. The whole group carries the same `@setenv` restriction the
menu entries do, refused in the observers because Bevy's `InteractionDisabled`
only talks to the a11y tree.

**The three settings-asset combos are here too, and that took a refactor.**
`quick_prefs_environment` held its three anchors in a singleton `PresetCombos`
resource, which could only ever describe one window. They are now addressed by a
`PresetCombo` component on each anchor — the track it drives and the host window
it is in — and a window declares a `PresetHost` carrying its own element ids and
scope tag. The row *lists* deliberately stayed a resource: what settings assets
inventory holds is one answer to one question, so both windows are views of it
and agree by construction rather than by being kept in step. A prev / next press
steps only its own window's combo, because the step is a gesture on one control;
the other window follows when the pick lands, the same way it would follow any
other change to the environment.

**The sun and moon can be scrubbed here.** Two trackballs over their four angle
sliders, from the shared `sl-viewer-environment::rows` controls, so this window
never learns what a sky is. Three things the obvious version gets wrong:

- *Opening the window must not change the shot.* The capture fills a buffer and
  stops there; only a drag marks it dirty and reaches
  `EnvironmentState::set_local_instant`. A window that pinned a copy of the
  region's sky just by being opened would be the one behaviour a photographer
  cannot forgive.
- *The sliders have to stay honest.* They re-capture whenever the environment
  changes under them — a time button here, the World ▸ Environment menu, a
  region change — so they show where the sun **is**, not where it was when the
  window opened.
- *And that honesty must not eat the drag.* A push marks the change as this
  window's own, and the capture in the same frame consumes the mark rather than
  reading it back: the round trip from two angles to a rotation and back loses
  the azimuth of a body at a pole, so a recapture mid-drag would move a compass
  the hand is holding still. The systems are chained push → capture → reseed for
  exactly that reason.

Scrubbing the sun un-pins a fixed sky, because `install_local` takes the sky
track back from the menu's pin — so the time buttons stop showing one lit, which
is the honest answer rather than a stale one.

**Four settings became reachable to a test.** Probes, mirrors, avatar complexity
and the derender filter declare their settings from a `Startup` system rather
than the registrar list (their plugins may run with no store at all), so nothing
outside those crates could ask what they declare. Each now exposes its
declarations as a plain `declare_*(&mut ViewerSettings)` that the startup system
calls, which is what lets this window's test build the store the viewer will
have and check its own rows against it.

## Not done — and why

- **No *Aids* or *Cam* tab.** The reference's *Aids* tab is the Advanced-menu
  render toggles (wireframe, bounding boxes, [[viewer-highlight-transparent]])
  plus a statistics strip, and its *Cam* tab is the camera / joystick window
  ([[viewer-camera-controls-window]]). Both are their own tasks; surfacing a
  toggle for a feature that does not exist is the one thing "exposes whatever
  exists" rules out.
- **The knobs this viewer has not got.** Depth of field, screen-space
  reflections, the vignette, antialiasing, the point-light budget
  ([[viewer-local-light-count-setting]]), ambient occlusion, the terrain and LOD
  scalars — the reference has a slider for each and this viewer has no setting
  behind any of them. Each is its own task; the table gains a row the day the
  setting is declared.
- **No *whole* sky editor here.** The scrubber is the two bodies' four angles
  and nothing else; the colours, the atmosphere, the clouds and the water are
  [[viewer-environment-personal-lighting]]'s, one button away. That boundary is
  the reference's own and is what keeps this from becoming a second copy of that
  window.

## Verified

`cargo test --release -p sl-viewer-preferences --lib` — 105 green. Fifteen are
this window's: every row names a declared setting; every row draws a control its
setting's type can drive; a checkbox over `RenderFarClip` is refused and the
gallery's storeless app still admits it; element ids are unique and namespaced;
no setting is on two rows; a combo offers at least two options; a slider's range
is non-empty and its step fits inside it; every tab has content; the
library/time pair round-trips through all three groups; an out-of-range group
falls back to the region day cycle; the four time buttons are four distinct
skies; opening captures without installing; a dragged angle is pushed once and
consumes its own latch; the scrubber carries only the four aim knobs.

Three more are the multi-host refactor's, in `quick_prefs_environment`: a step
moves only its own host's combo, every host's combo is filled from the one list,
and a pick on the second host applies.

`cargo test --release -p sl-client-bevy-viewer` — the floater and element
registries carry `phototools`, so the whole matrix swept it: every script, a
long translation at every scale, both directions at every UI scale, it actually
opens, it fits a laptop window, and the chrome sweep dragged, resized, minimised
and closed it. The accelerator pin gained `toggle-phototools`, which is the
deliberate edit that test exists to force.

Driven live against the local OpenSim grid (2026-09-12): the window opens from
the menu, every tab lays out, and the run logged no `phototools:` warning —
which is the runtime guard saying every row found its setting declared, with a
type its control can drive, in the store the viewer actually builds.
