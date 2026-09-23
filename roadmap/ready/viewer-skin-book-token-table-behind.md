---
id: viewer-skin-book-token-table-behind
title: The skin chapter's token table is four tasks behind the vocabulary
topic: viewer
status: ready
origin: viewer-skin-list-row-striping (2026-09-23)
points: 2
refs: [viewer-ui-skin-tokens, viewer-skin-light-surface-roles,
  viewer-skin-checkbox-radio-shape]
---

Context: [context/viewer.md](../context/viewer.md).

`book/src/authoring/skins.md` is the document a skin author reads, and its two
tables — "The role tokens we ship" and "The widget classes" — say they are the
whole vocabulary. They are not: the shipped-skin test
`every_token_common_css_reads_is_defined_by_every_skin` walks **67** tokens and
the chapter lists roughly two thirds of them.

Missing, by the task that added them:

- **The field / list family** ([[viewer-skin-light-surface-roles]]):
  `--field-bg-focused`, `--field-bg-readonly`, `--field-text-disabled`,
  `--field-text-readonly`, `--field-placeholder`, `--list-bg`, `--menu-bg`,
  and the `.sk-list-surface` class with the text re-rooting that is the whole
  point of it.
- **The checkbox and the radio**
  ([[viewer-skin-checkbox-radio-shape]]): `--check-bg`, `--check-border`,
  `--check-bg-checked`, `--check-border-checked`, `--check-tick`, the five
  `--radio-*`, and the `.sk-checkbox` / `.sk-radio-*` classes.
- Assorted singles: `--drop-target`, `--tile-bg`, `--tile-hover`,
  `--folder-label`, `--inline-item-bg{,-hover}`, `--console-*`, `--text-error`
  / `--text-warn` / `--text-note`, `--experience-accent`, `--presence-*`,
  `--notice-*`, `--marker-selected`.

The row family was added to both tables by the task that noticed this, so the
gap is now a known, bounded list rather than an unknown one.

## Why it matters more than a doc nit

A token the chapter omits is a token a third-party skin never defines, and the
failure mode is the silent one the test suite exists for: a `var()` that
resolves to nothing paints `bevy_flair`'s default, which is a colour nobody
chose. The test catches it for the **shipped** skins only.

## What to do

- Fill both tables from `common.css` rather than by hand, so the next omission
  is visible: the token list is already computable (the shipped-skins test
  computes it), and a doc test could assert the chapter names every token that
  test finds.
- That assertion is the actual deliverable. A one-off catch-up would be behind
  again after the next widget.

## Done when

Every token `common.css` reads appears in the chapter, and a test fails when
one does not.
