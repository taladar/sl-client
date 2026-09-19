---
id: viewer-audit-ui-spawn-helper-consolidation
title: The same widget spawn helpers are reimplemented in five to seven crates
topic: viewer
status: done
origin: static code audit (2026-08-26)
points: 5
refs: [viewer-audit-skin-token-coverage]
---

Context: [context/viewer.md](../context/viewer.md).

Near-identical helpers, each diverging only in hardcoded padding, border width
and colour:

- `spawn_action_button` — **7 copies**: `sl-viewer-notices/src/load_url.rs`,
  `sl-viewer-asset-editors/src/edit_wearable.rs`,
  `sl-viewer-edit/src/edit_material.rs`, `sl-viewer-edit/src/edit_params.rs`,
  `sl-viewer-notices/src/experience_permission.rs`, plus two more. Padding is
  10/5, 10/3, 8/2 and 10/2 across them; borders 1px vs 2px; some attach
  `ClassList` and some attach none, so those buttons are not skinnable at all;
- `spawn_text_button` (5 crates), `spawn_labeled_row` (5), `spawn_label` (5),
  `spawn_button` (5);
- `set_text` exists **six times in three incompatible shapes** —
  `(&mut Text, &str)` at `groups.rs:995`, `people.rs:2523`, `inventory.rs:3104`
  (identical, same doc comment); `(&mut Query<&mut Text>, Entity, &str)` at
  `conversations.rs:2643`; `(&mut Query<&mut Text>, Option<Entity>, &str)` at
  `about_landmark.rs:886` and `edit_material_asset.rs:969`. The last one **drops
  the `text.0 != value` guard** the other five have (documented at
  `inventory.rs:3102` as "so a re-bind of an unchanged row does not needlessly
  re-measure it"), so it dirties `Text` on every status refresh;
- `set_value_node` (4 copies), `despawn_children` (2, byte-identical),
  `short_id` (3).

Every one of these crates already depends on `sl-viewer-ui-core` and
`sl-viewer-ui-widgets`.

Scope: one set of helpers in `sl-viewer-ui-core::ui` / `ui_text`, taking the
padding and class as parameters. Pair with
[[viewer-audit-skin-token-coverage]] so the consolidated versions attach skin
classes rather than inline colours.

## Done

The count was worse than the audit's sample: `spawn_action_button` was **20**
copies, not 7, across nine crates; `spawn_labeled_row` 8; `set_text` 12 in five
shapes.

### The shapes, once

New `sl-viewer-ui-core::ui_spawn` — a module rather than more of `ui`, which is
already 2,400 lines of scaffold and owns a different subject. It holds the three
shapes a panel assembles its chrome from:

- `spawn_button(commands, parent, ButtonSpec) -> SpawnedButton { button, label }`
  — both entities, because a panel needs either: the box is what its action
  component and observer go on, the label is what a panel that retitles or greys
  its button writes to. `ButtonSpec::bordered()` / `::flat()` are the two shapes
  actually in use (a bordered push button; the People / Groups / My Environments
  action columns), narrowed by `#[must_use]` setters, so a call site names only
  what differs — its padding, its colours, its class, its font size;
- `spawn_labeled_row(commands, parent, LabeledRowSpec) -> LabeledRow`, covering
  the fixed / minimum label column, the wrap and the margin the eight copies
  varied over;
- `spawn_label(commands, parent, UiLabel, colour, size)`.

`UiLabel::Key` vs `UiLabel::Literal` is a distinction the copies made by hand:
a key re-resolves on a locale switch, a literal must not — its text came from
the grid (a script dialog's own captions, a group notice's subject).

`ButtonKind` is the one thing that could not be defaulted: `bevy_ui::Button`
(`Interaction`, what a `Pointer<Press>` observer wants), `bevy_ui_widgets`'
headless button (`Activate`), or neither for the flat columns. Sixteen of the
call sites wanted the headless one, and getting this wrong silently stops a
button emitting anything — `teleport_progress.rs` caught it first.

### Text, and the missing guard

`ui_text::set_text(&mut Text, &str)` and
`set_node_text(&mut Query<&mut Text, F>, impl Into<Option<Entity>>, &str)` —
one guarded implementation, generic over the query filter and over
`Entity` / `Option<Entity>`, which is what the five shapes actually differed
over. `edit_material_asset.rs`'s copy, the one that had **lost** the
`text.0 != value` guard and re-measured its status line on every refresh, is
gone with them. The three `set_value_node`s over a
`Query<(&mut Text, &mut TextColor)>` stay as three-line local adapters — the
query shape is theirs, the guard is no longer.

`inspector_popup.rs`'s `set_text` is deliberately kept: it is a deferred
`Commands` insert into a node that is not yet queryable, not the same operation.

### The two that did not need a helper at all

- `despawn_children` (2 byte-identical copies plus their `Query<&Children>`
  system params) is `commands.entity(parent).despawn_related::<Children>()`;
- `short_id` had four copies against `sl_viewer_social::short_id`, which was
  already `pub` and which both crates already depend on.
  `sl-crosscheck`'s `short_id(&str)` is a different function on a different
  input and stays.

### One behaviour decided rather than preserved

A helper-spawned label always carries `Pickable::IGNORE`. Most copies had it;
the ones that did not were not choosing differently — without it the text node
blocks the pointer and the box under it never sees the hover, so a button does
not light up when the pointer is over its own caption. Dropping the explicit
`Pickable::default()` several boxes carried is a no-op: `bevy_picking` treats an
absent `Pickable` as exactly that value.

### What was deliberately left to [[viewer-audit-skin-token-coverage]]

The colours and classes are **parameters**, not defaults inherited from
`ui_spawn`, and every call site passes what it passed before. Attaching
`sk-button` to the buttons that carry no class would restyle them — `.sk-button`
declares its own `padding: 5px 10px` and `border-width: 2px` — and deciding
which panels are skinned is that task's 8 points, not this one's. What this task
changes is that the class is now one spec field instead of a line missing from
fifteen copies.
