---
id: viewer-i18n-floater-literal-english
title: Several floaters still draw literal English instead of translation keys
topic: viewer
status: done
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

## Done (2026-11-27)

Every string the five windows draw that is not the grid's own text, a number
or a unit symbol now comes from `assets/locales/en/main.ftl`.

- **Search**: the five filter combos bind their option keys
  (`translate_labels`). The count line, the Events day label and the details
  pane's lines are typed values (`CountLine`, `EventsDay`, `DetailText`)
  computed by the same pure functions as before and worded by the bundle at
  the one place holding a `Translator`, re-worded on a locale switch. The
  member count and the far day offsets are plural selectors; "+3 days" and
  "-3 days" became "+3 days" and "3 days ago". A classified's category is now
  named as the Classifieds filter names it, where it drew `sl-types`'
  lower-case `Display` ("land rental"). A land row's "Unknown" type comes from
  the bundle too.
- **Material editor**: every label, the alpha-mode names, the Save / Revert
  buttons (node names keep the English locator), the loading line, the
  notecard note and the four status messages are keys; a status line is bound
  with `Translated`, so a locale switch re-words it. The "Edit: …" title is
  `material-editor-title`, re-formatted on a locale switch
  (`title_material_windows`).
- **Wearable editor**: the same for its buttons, statuses, title, Height,
  Gender (Female / Male radio), Color/Tint, and the layer texture slots, now
  worded with the reference's `panel_edit_*.xml` labels ("Upper body",
  "Fabric", "Left Arm Tattoo") instead of prettified layer names. The slider
  labels go through the bundle as the reference's `LLTrans::getString` does:
  `avatar-param-<slug of the avatar_lad label>`, 220 strings generated from
  the vendored `avatar_lad.xml`. A definitions file that names a parameter
  the bundle lacks keeps its own text, through a new miss-tolerant
  `Translator::lookup` (no missing-key report). The specimen's sample sliders
  are now real shirt parameters (they had invented ids and labels).
- **Web browser**: "Loading… {n}%" is `web-browser-loading`, with the percent
  rounded and clamped (`loading_percent`).
- **Texture picker**: the capped count is `texture-picker-count-capped`, the
  read-out a `TreeCount` component worded by `label_tree_counts` (the total
  alone is a locale-formatted integer).

The specimens use the live paths: keys bind through `Translated`, and the
lines with arguments are written by a queued one-shot system with the
translator, as the radar specimen's counts line already was.

Tests: the status / count / day / details tests now assert the typed lines or
the bound keys; new ones cover the day label, the details pane, a classified's
category, the tree count, the loading percentage, the parameter-label slug,
and that every texture slot and every editable `avatar_lad` parameter of
every slot and sex has an English string.
