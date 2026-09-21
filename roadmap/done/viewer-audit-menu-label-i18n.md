---
id: viewer-audit-menu-label-i18n
title: Menu and pie-menu labels cannot be translated, by type
topic: viewer
status: done
origin: static code audit (2026-08-26)
points: 8
---

Context: [context/viewer.md](../context/viewer.md).

`MenuCommand`, `MenuDef`, `MenuItemDef::DynamicSubmenu`, `PieAction`,
`PieMenuDef` and `ResolvedSlot` carry a **`label_key`** now, resolved through
`Translator` — 540 keys across fifteen files, and the English they used to
spell lives in `en/main.ftl` under one heading per surface.

## Everything a label decides is decided from the resolved text

That is the part that makes this a *type* change rather than a rename. Three
things are derived from a menu label, and all three now read the bundle's
answer:

- the **drawn line**, obviously;
- the **jump key** — `assign_jump_keys` takes the lines' resolved labels rather
  than the declarations, so a German menu's mnemonics are German letters. Its
  signature is `&[Option<String>]` (a separator is `None`), which also left it a
  pure function: the interesting property, that one menu never binds one letter
  to two lines, is checked without standing up a bundle;
- **menu search** — `key_matches_filter` matches the term against the resolved
  text, so a reader searching `Minikarte` finds the entry and `mini-map`, a
  string drawn nowhere, does not.

So a popup is built with a `Translator` in hand (`MenuBuildCtx`, fed from
`MenuNav`) rather than binding each row to its key. The pie resolves for a
second reason: `fit_pie_layout` grows the ring from the labels' **measured**
boxes, and a label that arrived a frame after it was spawned would be measured
empty and resize an open menu.

The one label that *is* bound, with `Translated`, is the bar button's — it has
no mnemonic to split and no search term to match, and binding keeps
`spawn_menu_bar` callable from a plain `Commands`, which the element registry's
fixed `fn(&mut Commands, Entity, ElementCx) -> Entity` requires. It relocalises
in place on a language switch; a popup is simply rebuilt on its next open.

## Settings descriptions: the key is declared, the text is pushed down

The audit's second companion — 210 untranslated `register_in` descriptions —
is done too, and it was not the same shape as a label. The string has two
consumers: the raw settings editor draws it, and the TOML writer puts it above
the value in the user's own `viewer-settings.toml`. A key at the call site is
right for the first and wrong for the second, because registration runs in
`FromWorld`, long before the locale folder loads, so the file would be stamped
with keys.

What resolves that is the **direction**. `sl-viewer-ui-core` depends on
`sl-viewer-settings`, so the writer cannot reach a `Translator` — but the layer
that has both can push the answer *down*: `ViewerSettings` keeps each setting's
`description_key`, registers the store's comment **empty**, and
`i18n::apply_setting_descriptions` resolves every key into the store when the
bundle loads, when the locale changes, and when a setting is registered late (a
floater registers its own geometry the first time it opens). Both consumers go
on reading one plain string, and a user running the viewer in Japanese gets a
settings file with Japanese comments.

Three registration families had the description in a **table** rather than at
the call site — `RLV_BOOL_SETTINGS`, `RLV_STRINGS` and `skin_colors`'
`COLOR_TOKENS` — so those fields are `description_key` now. `RlvStringDef`'s
was drawn in the Strings floater as well as registered, so that panel binds
`Translated` to it instead of writing the English in.

## What now catches a typo

Three things, in increasing order of how early they fire:

1. The new `i18n_keys` module holds the bundle and the **source** to each other,
   both ways: a key the source names that no English string defines would ship
   drawn as its own name, and a `setting-desc-*` / `menu-*` / `pie-*` key the
   bundle defines that nothing names is a line every translator goes on
   translating for a feature that is gone. It reads the source rather than a
   running viewer because neither half is enumerable from one — barely half the
   settings are declared by `REGISTRARS` (the rest register from a system on
   first use), and a domain's menu trees are `static`s private to its own crate.
   945 keys named, all defined, none orphaned.
2. `Translator::finish` reports an undefined key **once per key**, gated on the
   chain actually having a bundle — so the pre-load frames and every
   `install_untranslated` harness stay quiet, and a real typo is one line in the
   journal rather than a per-frame torrent.
3. Two new tests, one per widget, assert that a line's text is the *bundle's
   answer* rather than its key. That cannot be seen in the resting harness,
   where every key resolves to itself and the two are the same string, so they
   ask in the **pseudolocale** — the one locale that needs no bundle and still
   changes the answer, since `Translator` accents and fences whatever it
   resolved.

## The harnesses needed real strings, and one of them had none at all

Making a widget *require* a `Translator` turned a quiet harness gap loud. This
Bevy fork **panics** on a failed system-param validation rather than skipping
the system, so every app scheduling `MenuWidgetPlugin` or `PieMenuPlugin`
without the i18n resources died on its first frame — which is the right way
round, and it located the gaps in one run: `PieMenuPlugin` is part of the
*world* group rather than the UI one, so the string half belongs at the base of
`world_test::world_app` and in `full_stack_test::build_viewer_app`, not beside
the UI layer.

Then the layout checks failed, and they were right to. `install_untranslated`
answers every key with **itself**, which is exactly what a test naming the
strings a line is built from wants and exactly wrong for one that *measures*:
the trackball's compass label is `N` in English and `trackball-north` as a key,
so a box sized for one letter overflowed by 56 px, and an object pie whose
slices read `pie-object-take` needed a ring no viewer would draw. Worse, the
element sweep had never resolved a `Translated` label at all — it measured
**blank** ones, the failure `install_untranslated`'s own doc warns about.

So `BundlelessStrings` is a plain `key = value` table consulted **only while no
bundle has loaded**, and `install_english` fills it from the shipped
`en/main.ftl`. The bundle text is embedded in the **viewer** crate, beside
`notification_ftl_coverage`, rather than in `sl-viewer-testkit` where the
harnesses live: an `include_str!` of a viewer asset from a crate the whole
viewer dev-depends on would re-run every test above it on each string edit.
A real Fluent chain always wins, so
in a running viewer this is empty and changes nothing; a line-wise read cannot
carry a selector's multi-line value, which falls back to its key — the right
answer, since such a string needs arguments the table could not interpolate.
The world fold and the element sweep take the English; the widget tests keep
key-answers-itself, which is what their assertions are about.

## Two fixtures had to keep their text, and one baseline was already fragile

A pie fixture whose point is its labels' *size* cannot be keyed: the tiny-label
check measures one CJK glyph, the long-label check measures a translation-length
string, and under `install_untranslated` a key would measure twenty characters
of `pie-fixture-…`. Both use the drawn text as the key, which is exactly what
the harness's key-resolves-to-itself convention makes correct.

`the_fixture_pie_keeps_its_measured_angles` then failed for an unrelated reason
the longer labels merely exposed: the recorded angle was wrapped with
`rem_euclid(360)`, putting the discontinuity **on** the east compass point, so a
sub-pixel difference in where the text measured read as a 359° drift. The branch
cut is at −22.5° now — halfway between two points — which leaves every recorded
number unchanged and none of them on the seam.

## For the record

The audit's own note on the state of i18n elsewhere still stands: 16 hardcoded
`Text::new("…")` literals workspace-wide (15 now, `rlv_strings` having given one
up), and the three non-English locales cover a small fraction of the English
keys — they are locale-*mechanism* samples rather than translations, while the
pseudolocale and RTL machinery around them is complete. This task grew the
English bundle from ~3,000 keys to ~4,250, so that fraction is smaller than it
was; nothing about the mechanism changed.
