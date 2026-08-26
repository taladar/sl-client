---
id: viewer-audit-ui-spawn-helper-consolidation
title: The same widget spawn helpers are reimplemented in five to seven crates
topic: viewer
status: ready
origin: static code audit (2026-08-26)
points: 5
refs: [viewer-audit-skin-token-coverage]
---

Context: [context/viewer.md](../context/viewer.md).

Near-identical helpers, each diverging only in hardcoded padding, border width
and colour:

- `spawn_action_button` — **7 copies**: `sl-client-bevy-viewer/src/load_url.rs`,
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
