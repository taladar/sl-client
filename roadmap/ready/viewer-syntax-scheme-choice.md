---
id: viewer-syntax-scheme-choice
title: Choose a code editor syntax scheme apart from the skin
topic: viewer
status: ready
origin: viewer-lsl-editor-highlight (2026-10-29)
refs: [viewer-lsl-editor-highlight, viewer-ui-skin-tokens,
  viewer-i18n-colorblind-accessibility]
---

Context: [context/viewer.md](../context/viewer.md).

The code editor's colours are the `--syntax-*` token group
([[viewer-lsl-editor-highlight]]): the fallback sheet's dark scheme, or the
skin's own (Vintage ships a light one). A resident who wants another scheme in
the script editor — and not another chat window — has no way to pick one.

Ship a few schemes as small sheets that define only `--syntax-*` (a dark, a
light, a high-contrast one), add a preference that picks *Skin default* or one
of them, and layer the chosen sheet over the skin so it wins only for that
group. The colour-blind overlay ([[viewer-i18n-colorblind-accessibility]]) is
the same mechanism and should be able to reach these tokens.
