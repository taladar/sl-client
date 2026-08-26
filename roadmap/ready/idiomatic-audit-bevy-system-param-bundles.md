---
id: idiomatic-audit-bevy-system-param-bundles
title: 128+ too_many_arguments suppressions are Bevy systems that want a SystemParam bundle
topic: idiomatic
status: ready
origin: static code audit (2026-08-26)
points: 8
---

Context: [context/idiomatic.md](../context/idiomatic.md).

The workspace carries **293** `#[expect(clippy::too_many_arguments)]`
attributes, of which 128 are in the seven group-A feature crates alone
(`sl-viewer-edit` 73, `sl-viewer-people` 39, `sl-viewer-inventory` 29) and 18 in
the UI crates.

Every one carries an honest, bespoke reason about Bevy system parameters, so
these are **not** defects — the lint genuinely cannot tell a system signature
from a bad API. But the idiomatic fix is already in the codebase's vocabulary:
`#[derive(SystemParam)]` is used at `edit_create.rs:774`, `:798`,
`radar.rs:613`, `:631`, `edit_material.rs:1179`, `edit_texture.rs:1110`,
`:1739` — eight times, against 128 suppressions.

Two places where the parameters are already being bundled by hand, which is the
clearest signal:

- `sl-viewer-inventory/src/inventory_drag.rs:571-620` groups its parameters into
  `session` / `geometry` / `occlusion` / `targets` / `world` / `resolve` /
  `outputs` tuples and then immediately destructures all seven;
- `sl-viewer-ui-widgets/src/menu.rs` carries **13** suppressions (`:800`,
  `:832`, `:907`, `:1117`, `:1209`, `:1718`, `:1778`, `:1853`, `:2255`, `:2302`,
  `:2384`, `:2461`, `:2522`) threading the *same* bundle by hand —
  the same ten-parameter bundle by hand:

  ```text
  &child_of, &conditions, &slots, &entries, &free,
  &mut hosts, &mut branches, direction, &filter, &mut commands
  ```

  One `#[derive(SystemParam)] struct MenuNav<'w, 's>` collapses all thirteen.

Scope: sweep the largest clusters into `SystemParam` bundles, in a codebase
whose stated convention is no `#[expect]`. Start with `menu.rs`, then
`sl-viewer-edit`.

Not in scope, and worth recording so it is not re-litigated: the remaining cast
suppressions (152 `as_conversions`, 105 `cast_possible_truncation`, 63
`cast_sign_loss`) are overwhelmingly load-bearing numeric conversions with
checkable reasons, and the 134 `module_name_repetitions` ones all say
"re-exported at the crate root" — if that repetition is the house style, turning
the lint off workspace-wide beats 134 local suppressions.
