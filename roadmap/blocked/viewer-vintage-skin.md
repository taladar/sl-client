---
id: viewer-vintage-skin
title: Ship a Vintage-alike skin
topic: viewer
status: blocked
origin: Vintage skin fidelity audit (2026-09-20)
points: 8
refs: [viewer-ui-skin-tokens, viewer-vintage-bottom-bar,
       viewer-preferences-colors-skins-tab, viewer-skin-text-shadow-role,
       viewer-skin-list-row-striping, viewer-skin-icon-set,
       viewer-vintage-ui-chrome-crosscheck]
blocked_by: [viewer-skin-light-surface-roles, viewer-skin-image-backed-widgets]
---

Context: [context/viewer.md](../context/viewer.md),
[context/vintage-skin.md](../context/vintage-skin.md).

The capstone: a third shipped skin whose palette and widget shapes are the
reference's Vintage, rather than a third set of dark-on-dark values. The
viewer's feature set has been audited against Vintage twice — the
coverage audit of 2026-07-22 and the full-parity audit of 2026-08-19 — and
between them they produced the classic bottom bar, the utility cluster, the
radar and the contact sets. The one thing that has never been Vintage is how
it *looks*.

Everything it needs to say is already measured, in
[context/vintage-skin.md](../context/vintage-skin.md): the resolved palette
(merged over the reference's default skin, not the 100-entry override file
alone), the measured widget art, and the widget-default deltas. This task does
not re-derive any of it.

## What shipping it means

- `assets/skins/vintage/skin.css` — the role tokens set to the measured
  palette, square corners throughout (`--surface-radius: 0`,
  `--control-radius: 0`), the light field / list family from
  [[viewer-skin-light-surface-roles]], and `--text-shadow` set
  ([[viewer-skin-text-shadow-role]]).
- Authored nine-slice art for the widget surfaces that carry a shape — push
  button (idle / pressed / disabled), text field (idle / focused / disabled),
  tab, scrollbar parts, slider, progress bar, floater frame and header, the
  tooltip plate — to the measured geometry, drawn here and not copied
  ([[viewer-skin-image-backed-widgets]] and the licensing note in the context
  file).
- Registration: `SKINS` in `sl-viewer-ui-core/src/skin.rs`, the `--skin` flag's
  help, and the preferences Colors & Skins tab
  ([[viewer-preferences-colors-skins-tab]]) pick it up from there.
- The user-tunable palette (`skin_colors.rs`) gets Vintage's values as its
  skin-supplied defaults: the chat family, the name tags — note Vintage
  collapses match and mismatch onto one orange — and the minimap dots, which
  are already these values in both existing skins.

## Two deliberate deviations to record in the skin's own header comment

- **We do not reproduce `UIControlBGSelectedCompensate`.** It exists in the
  reference only to pre-compensate a hard-coded 0.7 alpha applied in code. We
  have no such multiplier; one selection colour does both jobs.
- **We do not fork floater layouts.** Vintage ships whole-file XUI forks of
  `floater_tools.xml` and friends; [[viewer-ui-skin-tokens]] ruled that out on
  purpose, and it is the reason a reference skin breaks every release. Shape
  and colour only.

## Done when

`--skin vintage` gives a viewer that reads as Vintage at a glance — dark grey
chrome, light black-texted fields and lists, periwinkle selection, bevelled
buttons, shadowed steel-blue labels, square corners — the chrome cross-check
([[viewer-vintage-ui-chrome-crosscheck]]) puts it beside the real thing, and
switching back to `graphite` leaves nothing behind.
