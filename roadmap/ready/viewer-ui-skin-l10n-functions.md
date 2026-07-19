---
id: viewer-ui-skin-l10n-functions
title: Skin CSS l10n/i18n functions (theme-authored labels & numbers)
topic: viewer
status: ready
origin: raised during viewer-ui-skin-tokens (2026-07)
blocked_by: [viewer-ui-skin-tokens]
---

Context: [context/viewer.md](../context/viewer.md).

A follow-up split out of [[viewer-ui-skin-tokens]]: let a **theme author their
own localized labels / numbers inside the skin CSS**, resolved through the
existing i18n / l10n stack, so a theme that ships a decorative label or a
formatted value renders it per the active locale.

**Why this is not in the base skin task.** `bevy_flair`'s property-parser hook
is a bare `fn(&mut Parser) -> Result<PropertyValue, CssError>` with **no access
to the ECS world**, so a `l10n()` function cannot read the active locale at
parse time — there is no drop-in path. It needs a self-contained preprocessor
layer that is orthogonal to the core token mechanism, so it ships on its own.

**Approach (validated against bevy_flair internals during the base task):** a
**preprocess-then-parse** layer that needs no fork.

- A theme ships its **own `.ftl` bundle** and writes custom functions in CSS,
  e.g. `content: -sl-l10n("theme-credit");` or `-sl-number(3)` /
  `-sl-datetime(...)`.
- Before the `.css` source reaches `bevy_flair`, a preprocessor resolves those
  custom functions through the existing `Translator` / `sl-l10n` formatters
  (the same number / currency / date/time formatting the viewer already wires)
  for the current locale, emitting plain resolved CSS that `bevy_flair` parses
  normally.
- On locale change, re-run the preprocessor and reuse `bevy_flair`'s existing
  reload path — it already listens for and even emits
  `AssetEvent::Modified { StyleSheet }` (`bevy_flair_style` systems) — so the
  content re-resolves live, the same machinery hot-reload uses.

This keeps the single-source-of-truth intact: translation *data* stays in
`.ftl`; the CSS only references keys. Strings never live in the stylesheet.

Composes with the culture-colour ([[viewer-i18n-cultural-color-meanings]]) and
CVD ([[viewer-i18n-colorblind-accessibility]]) overlays, which resolve through
the same locale/culture root attributes the base skin task installs.

The base task ([[viewer-ui-skin-tokens]]) builds the skin **loader** with a
preprocess seam so this drops in without reworking it.

Reference (Firestorm, read-only): none — its skins carry no localized content;
this is a feature beyond the reference.
