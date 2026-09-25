---
id: viewer-i18n-floater-literal-english
title: Several floaters still draw literal English instead of translation keys
topic: viewer
status: ready
origin: viewer-gallery-floaters-are-mostly-stubs — found while extracting their content builders (2026-09-25)
points: 3
refs: [viewer-gallery-floaters-are-mostly-stubs]
---

Context: [context/viewer.md](../context/viewer.md).

## Observation

Strings drawn as English literals, so they neither translate nor
pseudolocalise (and the layout sweep's long-translation cells cannot test
them):

- **Search** (`sl-viewer-search/src/search.rs`): "N members", "Born …", "Open
  enrollment", "No results", "Showing …", the events' day labels,
  "Auction" / "Sale", `LAND_SALE_LABELS`, `LAND_SORT_LABELS` and the category
  labels.
- **Material editor and wearable editor**: every label, the "Edit: …" titles
  and the status messages.
- **Web browser**: "Loading… {n}%".
- **Texture picker**: the "— refine search" count text.

## What to do

Give each a Fluent key in `assets/locales/en/main.ftl` (with a plural selector
where it counts), bind it with `Translated` or `Translator::format`, and let
the specimens pick the keys up — the sweep then measures the real strings.

## Done when

None of the windows above draws a word that is not from the bundle.
