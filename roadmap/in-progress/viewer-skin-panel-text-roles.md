---
id: viewer-skin-panel-text-roles
title: Panel text does not follow the skin, only the widgets it is built from
topic: viewer
status: in-progress
origin: viewer-audit-skin-token-coverage (2026-09-20)
points: 5
refs: [viewer-audit-skin-token-coverage]
---

Context: [context/viewer.md](../context/viewer.md).

`viewer-audit-skin-token-coverage` made the shared **widget set** skinnable —
floaters, tabs, tables, fields, combos, radios, scrollbars, menus and the pie
menu all take their colours from the skin now, through a `.sk-*` class or the
`SkinPalette` role palette. What it did *not* do is the ~40 **panels** built
out of those widgets: their labels, headings, row text and table specs still
name a colour at spawn time.

They no longer *drift* — all 64 copies of the label colours now read
`SkinPalette::FALLBACK.text_primary` / `.text_muted` / `.text_disabled`, so
there is one value and one place to change it. But `FALLBACK` is the unskinned
constant: wearing Azure recolours the floater a panel lives in and leaves the
text inside it Graphite's.

Two obstacles, and they are why this was not folded into that task:

1. **No world access at the spawn site.** A panel's chrome is built by free
   functions taking `&mut Commands` (`spawn_label`, `spawn_labeled_row`,
   `ButtonSpec`), several layers below any system that could hold a
   `SkinColors`. Threading the palette down means either a parameter on each
   helper (hundreds of call sites) or a new seam.
2. **`const` table specs.** Several panels declare their table as
   `const SPEC: TableSpec = TableSpec { header_color: …, cell_color: … }`,
   which cannot read a resource at all.

Two candidate approaches, both worth costing before picking:

- **Classes on the spawn helpers.** `ui_spawn::spawn_text` already takes an
  `Option<&'static str>` class and every panel label goes through it — so
  defaulting `spawn_label` / `spawn_labeled_row` to `.sk-text` / `.sk-title`
  (with the passed colour kept as the unskinned fallback) would cover most
  panel text with no call-site change at all. The trap is the one documented
  in `sl-client-skin-tokens-bevy-flair`: a class `color` beats a Rust-painted
  `TextColor`, so any panel that *recolours* a label per state (a greyed
  action column, a filter-dimmed row) needs a state class or has to opt out.
  Find those first.
- **A `TableSpec` that names roles rather than colours** — store a role enum
  and resolve it when the table is spawned, which is inside a system.

## What the state sweep left you (2026-09-21)

`viewer-skin-panel-state-classes` went first precisely so the trap above is
gone: a class on the spawn helpers would have beaten a Rust-painted
`TextColor` and flattened a greyed action column or a disabled check glyph into
one colour, silently and only in the live viewer. Those paints are classes now.

Two things it hands over:

- **Resting greys.** `about_land.rs` and `about_region.rs` each spawn a value
  node with `TextColor(DISABLED_COLOR)` — a colour at birth, not a state, so it
  stayed. Their `DISABLED_COLOR` constant survives for those and for the check
  painters.
- **An argument-count side effect.** Splitting a combined
  `(&mut Text, &mut TextColor)` query into content-plus-state adds a parameter,
  and several of these systems already sit near the workspace's seven-argument
  clippy limit — `experience_picker.rs` went over and needed a `PickerChrome`
  bundle. Expect the same in the larger `sl-viewer-places` and
  `sl-viewer-people` systems.

`ButtonSpec::class` / `::label_class` are the seam the greying used, and they
work: several panels now pass both. `ui_spawn::spawn_text` already takes an
`Option<&'static str>` class, so the "classes on the spawn helpers" route is
the one with evidence behind it.

## The seam, chosen and built (2026-09-22)

**"Classes on the spawn helpers", with the role read back out of the colour.**

`ui_spawn::spawn_text` — which `spawn_label`, `spawn_labeled_row` and
`spawn_button` all end in — now derives the class from the colour it was
handed: `FALLBACK.text_primary` → `.sk-text`, `text_muted` → `.sk-title`,
`text_heading` → `.sk-heading`, `text_disabled` → `.sk-disabled-text`. A colour
that matches no role gets no class and keeps painting itself.

That works only because `viewer-audit-skin-token-coverage` had already
collapsed all 64 label-colour copies onto those four constants: a panel asking
for `text_muted` is **naming a role**, not picking a grey, so the equality is
exact rather than approximate. Reading the role back out is what covers ~230
call sites without touching one of them — the alternative, a role parameter on
each helper, is the same information spelled 230 times.

The `TextColor` stays beside the class: it is the unskinned value a headless
world, and a skin that omits the token, fall back to. `spawn_text`'s explicit
`class` argument still wins where a caller passed one.

`a_label_takes_the_class_of_the_role_its_colour_names` holds both halves,
including that an off-role colour is left alone.

### What is left

- **The ~449 direct `TextColor(…)` spawns** that do not go through a helper.
  These are the panel-specific ones; each needs the same judgement the helper
  now makes automatically, and a good number will turn out to be a role
  constant that can simply move to a class.
- **`const TableSpec`.** The second obstacle is untouched: several panels
  declare `const SPEC: TableSpec = TableSpec { header_color: …, cell_color: … }`
  and a `const` cannot read a resource. The candidate remains a role enum in
  the spec, resolved when the table is spawned (which is inside a system).
- **A live look.** The change is invisible under the built-in fallback by
  construction and only shows once a skin's own token differs, so the gallery's
  live switcher (or a viewer run) is the check that it actually took.

Done when a skin switch recolours a panel's text the way it already recolours
its floater, and the gallery's live switcher shows it.
