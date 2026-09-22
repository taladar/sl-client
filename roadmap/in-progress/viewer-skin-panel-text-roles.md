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

**The invariant it rests on** is that the mapping is *injective*: each of the
four values must belong to exactly one role, or a label would silently take
another role's class. That holds today — the nearest neighbours are
`pie_label_sub_pie` against `text_heading` and `title_text_inactive` against
`text_muted`, both distinct — and
`the_text_roles_are_the_only_roles_with_their_values` now enforces it by
reflection over every palette field, so a role added or retuned later is
covered without anyone remembering. Only the **fallback** has to satisfy it: a
skin may give two roles the same value freely, since the class is chosen at
spawn from the fallback and the skin paints per class. If the fallback ever
*needs* two equal text roles, the derivation has to be replaced by an explicit
role at the call sites — which is the option this seam was chosen over, and
the failure message says so.

## The tables (2026-09-22)

The second obstacle turned out not to need the role enum the plan proposed.
`role_class` answers the same question for a `const TableSpec`'s
`header_color` / `cell_color` as it does for a label's, so the ~34 specs are
covered where they stand — the spec keeps naming a colour and the widget reads
the role out of it.

A cell's role is **not** fixed by its column, though, which is what makes this
more than the header case: the radar colours its range column by distance band
and its name column by whether the avatar is muted, so the role has to follow
each bind. `set_table_cell` therefore carries the class beside the colour
(`skin::set_role_class`, which makes exactly one of the four current), and its
query widened to `(&mut Text, &mut TextColor, Option<&mut ClassList>)` across
19 consumer files.

The table widget also stopped painting one state of its own: header greying
was a `SkinColors`-reading system writing `disabled_text_color`, and is now
`.sk-disabled-text` over whatever role the spec's header colour names.

### B0001 is the hazard here, and the compiler cannot see it

Widening that query gives a system a second `&mut ClassList` access whenever it
already had one for its **rows** — Bevy's B0001, which is a panic on the
system's first run rather than a compile error. Six systems had it
(`settings_picker`, `asset_blacklist`, `avatar_render_floater`,
`group_profile`, `blocked`, `contact_sets_panel`, `my_environments`). A row
carries no `Text`, so `Without<Text>` on the row query makes the two provably
disjoint and Bevy accepts them.

Three of those were found by a **static scan** for "a widened cell query beside
an unfiltered `&mut ClassList` in one signature", not by the suite. Anything
touching this again should run that scan rather than trust a green test run:
whether a given panel's bind system is scheduled by any test is not obvious,
and the failure only exists at runtime.

## The direct spawns (2026-09-22)

`skin::text_role(color) -> (TextColor, ClassList)` is the spawn-site form of
the same derivation: `text_role(LABEL_COLOR)` in place of
`TextColor(LABEL_COLOR)` is the whole edit, and a panel that builds its own
text nodes becomes skinnable without restating which role it meant.

**Found and rewritten with `ast-grep`**, which matches the syntax tree rather
than the text — so a spawn tuple is distinguishable from a test assertion, and
formatting is irrelevant. Two rules did it:

- `TextColor($C)` `inside` a `spawn` / `with_child` / `insert` call, which
  excludes the assertions, and `constraints: {C: {kind: identifier}}`, which
  excludes a literal `Color::srgba(…)` — a literal names no role by
  definition. 200 sites in 54 files.
- The same pattern is the census tool: `--json` plus a counter gives the
  distribution of *what* the 449 sites pass, which is what turned up the
  second tier of drift below.

Only files that never take `&mut TextColor` were rewritten (287 of the 449
sites are in such files). A class `color` beats a Rust-painted `TextColor`, so
a node something still repaints has to keep painting; `chat.rs`'s age fade is
the standing example.

### The second tier of drift

The census showed constants that are *almost* a role, which
`viewer-audit-skin-token-coverage` missed because it collapsed by **name**:
`TEXT_COLOR` was `srgb(0.90, 0.93, 0.97)` against `text_primary`'s
`srgb(0.90, 0.92, 0.96)`, `DIM_TEXT_COLOR` `srgb(0.64, 0.68, 0.76)` against
`text_muted`'s `srgb(0.62, 0.66, 0.74)`. 28 such constants now read
`SkinPalette::FALLBACK.*` and skin themselves, since `text_role` reads the
value.

Deliberately **not** collapsed, and why:

- `NAME_TAG_MISMATCH` has its own user-tunable `--name-tag-mismatch` token and
  is world-space name-tag data, not panel chrome.
- `DISABLED_MARKER` is the trackball's, already recorded as the case that
  resists conversion.
- `GHOST_COLOR` is the inventory drag ghost — an overlay, not panel text.
- `BAR_LABEL_DIM` and `CHROME_COLOR` each sit equidistant between two roles,
  so picking one is a judgement rather than a collapse.
- `HEADER_COLOR`, `SECTION_COLOR`, `VALUE_COLOR` are further than 0.06 from
  any role (`HEADER_COLOR` is *gold* in one crate). Their own shades, not
  drift.

### The files that *do* write a colour (2026-09-22)

Of the 27 files holding a `&mut TextColor`, **seventeen never actually write
one** — their only such query is `set_table_cell`'s widened cell tuple, which
carries the class itself. Those 75 sites converted with the rest.

One of them was a defect this audit found in the greying commit
(`791bd913`): `phototools`' button captions had been given `.sk-text` while
`drive_photo_button_labels` still wrote their colour every frame — two writers
on one component. The system was redundant by then (the button carries
`.sk-action-button` and `InteractionDisabled`, so
`.sk-action-button:disabled .sk-text` greys the caption), so it is deleted
along with its `PhotoButtonLabel` marker.

**Ten files have a real writer**, and their ~61 sites need per-site judgement
— which node the writer targets, and whether *this* spawn is that node:

| file | what writes a colour | state |
| --- | --- | --- |
| `inventory.rs` | the row arrow and the label, per bind, by row kind | **done** |
| `rlv_console.rs` | a line's colour, by what the line is | **done** |
| `about_land.rs`, `about_region.rs` | `set_check_visual`'s three-state glyph | checkbox task |
| `land_environment.rs`, `snapshot_floater.rs` | the same hand-rolled check | checkbox task |
| `group_profile.rs` | `set_toggle_glyph` | checkbox task |
| `ui_text_input.rs` | the field text, disabled / read-only vs `field_text` | widget state |
| `name_tag_billboard.rs` | world-space name tags — data, not panel chrome | not this task |

**Two converted, and both turned out to be roles rather than repaints.**
`inventory.rs` wrote its row arrow the *same* colour on every bind — a write
that could never change anything — and its label gold-or-plain by whether the
row is a folder, which is the reference's own distinction and the fastest way
to read a long tree. That is `--folder-label` and a class now.
`rlv_console.rs` wrote a line's colour from a four-way `line_color(kind)`; a
refused command staying distinguishable is exactly what a colour-blind overlay
retunes, so the kinds are `--console-info` / `--console-error` over `.sk-text`,
with a typed command carrying no class at all. `line_color` and the constants
behind it are gone.

Five of the remaining six are the hand-rolled check painters
[[viewer-skin-checkbox-radio-shape]] owns — converting them now would invent a
class vocabulary that task will replace. `ui_text_input`'s is a widget state
wanting a class rather than a palette read, and `name_tag_billboard`'s is
world-space data.

### What is left, and it is not this task's

Nothing here is still panel-text work:

- **64 sites over 22 constants that resolve to no role**, plus the 32
  `Color::WHITE` literals — each a judgement rather than a sweep, and every
  one already calling `text_role`, so each fix is one line in a *constant*.
  Their own task now: [[viewer-skin-text-colours-without-a-role]].
- **Five hand-rolled three-state check painters** (`about_land`,
  `about_region`, `land_environment`, `snapshot_floater`, `group_profile`) —
  [[viewer-skin-checkbox-radio-shape]]'s, and converting them here would
  invent a vocabulary that task will replace.
- **`name_tag_billboard`** is not residue at all: `NAME_TAG_MISMATCH` is
  `--name-tag-mismatch`'s fallback, and name tags are already skinnable
  through the user-tunable palette.

### The live look, deferred on purpose

A half-converted UI cannot be judged by eye — nothing distinguishes a colour
that is already skin-driven from one that is not. Now that panel text, tables,
widget state and row selection all go through the cascade, the check is
meaningful and simple: **does a skin switch in the gallery recolour
everything?** Two defects this work turned up (the cascade ordering, the
phototools double-writer) were invisible headlessly, so it is worth doing
before this is called finished.
