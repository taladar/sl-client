---
id: viewer-build-numeric-field-spinners
title: Spinner widget — up/down arrow numeric fields (build window and every reference spinner)
topic: viewer
status: done
origin: user request (2026-07-24) while reviewing the build-tool numeric fields
refs: [viewer-prim-parameter-editing, viewer-prim-texture-editing,
  viewer-object-edit-floater-shell, viewer-automation-semantic-custom-widgets]
---

## Done (2026-10-03)

The scope grew on request (2026-10-02). Every control that is an
`LLSpinCtrl` in the reference is now one here, wherever we have the control.
The spinner is skinnable through CSS and drivable from the automation and
accessibility layers.

**The widget** is `sl_viewer_ui_widgets::ui_spinner`. `spawn_spinner` wraps
`spawn_text_input` and puts a column of up / down arrows at the inline start,
as the reference does. A `SpinStep` holds the increment, min, max and
decimals.

- A press steps once, and holding repeats (`HoldToRepeat`).
- Up / Down step while the field is focused.
- Modifiers scale the increment: Alt ×10, Ctrl ×0.1, Shift ×0.01, the
  `<FS:KC>` order.
- A step is rounded to the decimals, then clamped. A field holding no number
  does not step.
- The arrows mirror the field's `InteractionDisabled` / `ReadOnlyField`.
- A step announces `SpinnerStepped`. `FieldCommits` is the commit input every
  consumer now shares (Enter, blur or step); it replaced six hand-copied
  focus trackers.
- A settings-bound spinner writes its setting on the step itself.
- `Spinner::set_step` re-ranges a field: the Object tab's twist, taper and
  hole ranges per prim type.

**Settings binding**: a text field bound to an F32 / I32 / U32 setting
parses and writes in that type. Before this, every edit was written as a
string, so the radar age-days preference never saved.

**Automation and accessibility**:

- The new `spinbutton` role is on the **field** (the focus stop), valued by
  `NodeValue::Number`. `fill` types into it; `NodeValue::holds` confirms a
  numeric fill.
- A text wait or `in_app::text` on a spin button reads its number.
- The arrows are buttons `{element}:up` / `:down`, named by
  `spinner-increase` / `spinner-decrease`, inside the `{element}:spinner`
  group.
- AccessKit gets `SpinButton` with min / max / step from the field's
  `AccessibilityNode`.

**Skins**:

- Classes: `.sk-spinner`, `.sk-spinner-arrows`, `.sk-spinner-arrow` (with
  `:hover`, `:active` and `:disabled`), `-up` / `-down` and
  `.sk-spinner-arrow-glyph`.
- Tokens: `--spinner-arrow-bg` (plus `-hover`, `-pressed`, `-disabled`),
  `--spinner-arrow`, `--spinner-arrow-disabled` and
  `--spinner-arrow-border`, in every skin.
- Vintage dresses the arrows in its step-button art.

**Converted**:

- Build floater: the transform rows and grid unit; the Object and Features
  fields; the Texture, legacy-material and PBR transform / factor fields.
- About Land: pass price and hours.
- About Region: agent limit, object bonus, restart delay, water height,
  terrain limits and the eight corner elevations.
- Item price and inventory filter hours / days.
- Group enrollment fee and classified price.
- Media size and world-map coordinates.
- The Debug Settings numeric editors.
- Preferences: chat max lines (was a slider) and radar age days.
- Phototools: the reference's eight `S_*` spinners now sit beside their
  sliders, replacing the readouts.
- The material editor's factors (were sliders).
- The colour picker's R / G / B / H / S / L (were sliders).

**Deliberate divergences**:

- The wheel does not step. Over a scrolling page it would change values the
  user only scrolled past.
- LOD factor keeps three decimals; the reference's zero would round 1.125.
- World-map coordinates clamp to what a location carries (255 / 4095).
- Chat max lines floors at 1, as the overlay does.

The reference spinners whose controls we lack are left to those features'
own tasks: the joystick, particle editor, mesh upload, poser, snapshot size,
Build Options, and the build floater's physics / probe / sale-price
controls.

Context: [context/viewer.md](../context/viewer.md).

Every numeric field in the Build Tools floater (the reference's `LLSpinner`)
carries a pair of small **up / down arrow buttons** that step the value by a
per-field increment, holding to repeat. Ours are plain text inputs today — a
value only changes by typing. Add spinner arrows to each numeric build field:

- **Where**: the transform rows (position / rotation / size X-Y-Z), the grid
  unit, every Object-tab shape spinner (cut / hollow / twist / taper / shear /
  radius / revolutions / skew …), the Features-tab flexi / light / spot fields,
  and the Texture-tab transparency / glow / repeats / offset / rotation fields.
- **Step**: the reference's per-field increment (`LLSpinner` `increment`), e.g.
  0.01 m for position / size, 1° for rotation, the shape fields' own steps.
  Shift / Ctrl modifiers step by a coarser / finer amount as the reference does.
- **Behaviour**: click steps once; press-and-hold auto-repeats after an initial
  delay. Each step commits exactly as an Enter would (the same
  `MultipleObjectUpdate` / `ObjectImage` / feature send), and clamps to the
  field's min / max. The arrows grey / disable with their field (they share the
  field's gate — see the no-selection disabling already wired for the transform
  and Texture-tab controls).

Best done as a **reusable spinner widget** wrapping `spawn_text_input`
(mirroring the reusable combo / radio / colour-picker widgets), so every build
field and any future numeric field gets arrows for free.

Reference (Firestorm, read-only): `llspinctrl.cpp` / `llspinctrl.h`
(`LLSpinCtrl` — the arrow buttons, increment, hold-to-repeat, clamp).

**Automation semantics** (deferred here from
[[viewer-automation-semantic-custom-widgets]], which found no spinner to give a
role): the widget adds a `spinbutton` role to `sl-automation-proto`'s `Role`,
puts `Semantic::new(Role::SpinButton)` on its root with the field's number as
its value (a `NodeValue::Number`), and names its two arrows ("Increase" /
"Decrease" keys) so the focus-stop guard
(`ui_contract::every_focus_stop_has_a_contract_row`) passes. A teeth test in
`automation_model.rs`: an arrow click changes the reported value.
