---
id: viewer-skin-checkbox-radio-shape
title: Checkboxes are hand-rolled per panel, so no skin can shape one
topic: viewer
status: in-progress
origin: Vintage skin fidelity audit (2026-09-20)
points: 5
refs: [viewer-ui-radio-widget, viewer-ui-settings-binding, viewer-vintage-skin,
  viewer-bevy-empty-text-measures-at-parley-defaults]
---

Context: [context/viewer.md](../context/viewer.md),
[context/vintage-skin.md](../context/vintage-skin.md).

There is no checkbox widget. `sl-viewer-ui-widgets` has `ui_radio`, `ui_combo`,
`ui_slider`, `ui_table`, `ui_tab`, `ui_text_input` — and every checkbox in the
viewer is a square `Node` spawned by whichever panel needed one, with that
panel's own constants. `settings_binding.rs` has `bound_checkbox`;
`preferences/phototools.rs` declares its own `PhotoCheckboxBox` and a fill
colour pair; `environment/my_environments.rs` declares `CHECK_SIZE`,
`CHECK_ON`, `CHECK_OFF`. They do not agree with each other, and none of them
can be reached by a skin.

The reference draws a checkbox as a **light box with a dark frame and a tick**
— Vintage's is a `#d8d8d8` face inside a 1 px `#0a0a0a` frame with a soft drop
shadow, and a radio button is a white disc in a dark ring, both 15×15. Ours
are filled squares that go blue when checked. The shapes are different in
kind, not in tint: one is a container you look into, the other is a swatch.

## Decided (2026-09-21)

The shared-widget approach below is **agreed with the user**, not a proposal —
do not re-open it.

Two things the `viewer-skin-panel-state-classes` sweep measured while working
around this:

- **19 files declare their own `CHECKED_GLYPH`**, 30 glyph constants between
  them. `settings_binding::bound_checkbox` is a binding helper, not a widget,
  and the panels that do not go through it each draw their own.
- Three **three-state check painters** were left un-converted for this task to
  absorb — `about_land.rs`, `about_region.rs` and `land_environment.rs`, each
  resolving `!enabled` / `on` / `off` to a glyph colour by hand. They were the
  only sites in that sweep with nothing to convert *to*: a class vocabulary
  invented for them now would have to be undone when the widget lands. They
  are the natural first call sites.

## What to do

- A shared `ui_checkbox` in `sl-viewer-ui-widgets`, on the same model as
  `ui_radio`: box, tick, label, the interaction contract the rest of the set
  follows, and a class the cascade can select. `ui_radio` is the template —
  including its two-node problem, since `bevy_ui` has no style inheritance and
  the box and its label must each carry their own class.
- Role tokens for both: box surface, frame, tick, and the disabled variants;
  the tick's colour is not the label's.
- Migrate the hand-rolled call sites — `bound_checkbox` first, since the
  settings panels are most of them — and delete their local constants.
- Leave room for the image-backed form ([[viewer-skin-image-backed-widgets]]):
  the reference expresses checked / unchecked / pressed / disabled as six
  separate textures, so the state model must be the selector one from
  [[viewer-skin-widget-state-classes]], not a per-frame fill.

This is as much a consistency fix as a skinning one — three spellings of a
checkbox is three chances for one of them to drift, and two of them already
have.

## Done when

One checkbox widget exists, every panel uses it, its shape and colours come
from the skin, and a scratch skin can make it a light box with a dark frame
and a tick without touching Rust.

## Progress (2026-09-22)

The widget exists and the skin owns all four looks: `ui_checkbox` in
`sl-viewer-ui-widgets` spawns row / box / tick / caption, carries
`CHECKBOX_CLASS`, `CHECKBOX_BOX_CLASS` and `CHECKBOX_TICK_CLASS`, and **paints
nothing** — `:checked` and `:disabled` reach it from `common.css`, and the tick
is that stylesheet's `content` glyph on a `::before` rather than a constant in
Rust. `ui_radio`'s glyph-swapping half is gone the same way. Two upstream fixes
were needed to get there, both in the `taladar/bevy_flair` fork: a text
`::before` spawning without `StyleData` (`dbaaa48`), and one not inheriting its
host's font (`946f8a2`).

**Call sites converted so far:** the whole of `sl-viewer-preferences` —
`preferences.rs` (`spawn_pref_checkbox`, so every Preferences tab),
`preferences_alerts.rs`, `debug_settings.rs`, `quick_preferences.rs` and
`phototools.rs`, plus the Preferences and Photo Tools gallery specimens. Their
`CHECK_ON` / `CHECK_OFF` / `CHECK_DISABLED` constants and the three per-panel
`drive_*_checkbox_visual` systems are deleted.

**The caption belongs to the widget.** Two of those panels first kept their own
label beside a caption-less box; a label that is not part of the checkbox
cannot be clicked to toggle it, and the reference puts the box *first*
(`<check_box label="…" left="3">`), which is also what keeps a column of boxes
aligned. Both now pass their label into `CheckboxSpec`. The two sites that stay
caption-less are the ones whose text is genuinely elsewhere: the alerts table
(the text is a different column) and the debug-settings bool editor.

**Two defects the contract sweep caught on the way**, both now fixed here:

- A checkbox with no `SettingBinding` behind it **never ticked**.
  `bevy_ui_widgets`' headless `Checkbox` only announces a toggle as a
  `ValueChange<bool>`; nothing
  adds or removes `Checked` unless something observes it, and only the binding
  path did. `spawn_checkbox` now attaches `checkbox_self_update`, so the widget
  is self-updating and a bound one still converges on what the store holds.
- Both glyph hosts were measured at parley's defaults because they held no
  characters — see
  [[viewer-bevy-empty-text-measures-at-parley-defaults]]. The radio's indicator
  also had no reserved box at all, so a column of options aligned their captions
  differently depending on whether a stylesheet was loaded; it now occupies the
  same 14 px square the checkbox's box does.

**Every call site is converted (2026-09-22).** The nineteen `CHECKED_GLYPH`
declarations are gone, and so are the four colour-box painters
(`drive_search_checkbox_visual`, `paint_notify_checkbox`,
`sync_radar_limit_checkbox`, the my-environments fill loop): About Land, About
Region, the land-environment override, the six Build Tools editors, the avatar
and group profiles, contact sets, the radar, the inventory filters and item
properties, the script editor, the material-asset editor, the snapshot floater,
search, experiences, the notification toast's ignore box and the colour
picker's apply-now toggle.

Three shapes came out of it, and they are the thing to reuse when the next
panel grows a checkbox:

- **The panel owns the value.** A settings-bound box needs nothing at all:
  `bound_checkbox` on the widget's checkbox entity, and the binding moves
  `Checked` both ways.
- **The grid owns the value.** About Land / Region, the Build Tools faces, the
  parcel-override confirmation: the observer writes the draft or sends the
  command, and a refusal *puts the tick back*, because the widget flips its own
  marker before anyone asks whether the change is allowed. The sync pass is the
  only other writer.
- **Read-only is `InteractionDisabled`**, not a dim colour: the "You can" rows
  of item properties, a group flag the agent cannot change, a script editor
  opened on a no-modify item. `.sk-checkbox:disabled` greys box and caption
  together.

A gallery element (`checkbox-states`) shows all four looks side by side, and
the contract table pins that the live two toggle on a click, `Enter` and
`Space` while the refused two answer nothing.

## The radio's disc (2026-09-22)

The other half of the title, and the last mechanism gap. The indicator was a
**character** — `◯` / `◉` through `content` — and a glyph carries exactly one
colour, so a skin could recolour the ring but never draw the reference's *pale
disc inside a dark ring*, which is two. It is a round box now
(`border-radius: 50%`, asserted, since a `%` the parser rejected would leave it
square with nothing else to show for it) with the lit mark as a `content` pip
inside it — the checkbox's structure exactly:

- `.sk-radio-indicator` — the disc: `--radio-bg` / `--radio-border`, and the
  `:checked` pair. Five new tokens, defined in the fallback sheet and both
  shipped skins.
- `.sk-radio-pip::before` — the mark, `--radio-pip`, matched only under
  `:checked`, so an unlit option has no rule and nothing to hide.
- `.sk-radio-group:disabled` greys the disc, the pip and now the caption, the
  way a refused checkbox does.

**A guard came out of it.** Nothing checked that a token a *class* rule reads
is defined at all — the shipped-skin tests only covered the `-sk-color-*`
palette roles, so `--check-*` had no coverage either and a missing `--radio-*`
would have painted bevy_flair's default silently.
`every_token_common_css_reads_is_defined_by_every_skin` now walks all 67 tokens
`common.css` names across the fallback and both skins.

## Still open

- `.sk-checkbox:disabled .sk-text` was added for the greyed caption; the
  **image-backed** form ([[viewer-skin-image-backed-widgets]]) is still the
  nine-slice swap this was built to allow.
- ~~The tick and ring hosts carry a zero-width space~~ — fixed upstream in the
  bevy fork (`302316a`) and both hosts are `Text::default()` again; see
  [[viewer-bevy-empty-text-measures-at-parley-defaults]].
