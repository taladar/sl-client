---
id: viewer-skin-text-shadow-role
title: A text-shadow role — the trait that makes classic UI text look classic
topic: viewer
status: done
origin: Vintage skin fidelity audit (2026-09-20)
points: 3
refs: [viewer-ui-skin-tokens, viewer-vintage-skin, viewer-ui-text-foundation,
  viewer-sliced-art-seam-at-fractional-ui-scale]
---

Context: [context/viewer.md](../context/viewer.md),
[context/vintage-skin.md](../context/vintage-skin.md).

The single widest-reaching change the Vintage skin makes to its widget
defaults is one attribute:

```text
<!-- vintage/xui/en/widgets/text.xml, against the default skin's "none" -->
<text font_shadow="soft" … />
```

Every static label in the viewer gets a soft drop shadow. Buttons and tab
labels get the same through `label_shadow="true"`, and floater titles through
`header_font_shadow="soft"` (which the default skin also sets — so titles are
the one place we already differ from *both* reference skins). It costs one
attribute there and it is most of why the skin reads as period-correct: pale
steel-blue labels (`#93a9d5`) on mid grey would otherwise sit flat against
their background.

We render no shadow on any text anywhere.

## What to do

`bevy_flair` already parses `text-shadow` and maps it onto bevy's
`TextShadow`, so this is a token plus the rules that consume it:

- `--text-shadow` in each skin's `:root` (the flat skins: `none`);
- `common.css` rules applying it to the text roles that carry a label —
  `.sk-text`, `.sk-title`, button and tab labels, the floater title, status
  read-outs;
- Vintage-style value: a 1 px offset in a near-black at partial alpha, matched
  against the reference by eye rather than by guessing at what "soft" meant in
  `llfontgl`'s three-level enum (`none` / `hard` / `soft`).

Two things to check while wiring it, both of which would quietly undo it:

- **`bevy_ui` has no style inheritance.** A shadow set on a container reaches
  no text; every text node needs the rule to match it directly, the same trap
  the `.sk-disabled-surface` / `.sk-disabled-text` pair exists for.
- **Shadowed text must not change layout.** `TextShadow` draws outside the
  glyph box, so it should not reflow anything — confirm on a row that is
  already tight, because a shadow that nudges a baseline would show up as a
  regression in an unrelated panel.

## Done when

`--text-shadow` exists in every shipped skin, the label roles consume it, the
flat skins are pixel-unchanged (value `none`), and a skin that sets it shows
the shadow on labels, button labels, tab labels and floater titles with no
layout movement.

## Done (2026-09-24)

- `--text-shadow` is a whole `text-shadow` value, defined in `fallback.css`
  and both shipped skins as `none`. Graphite's **Relief** theme — the one
  shipped look with classic shapes — sets `1px 1px #000000a6`, so there is a
  shipped skin to look at.
- `common.css` reads it on every **chrome** text rule: `.sk-text`, `.sk-title`,
  `.sk-heading`, `.sk-tab-label`, `.sk-floater-title-text`,
  `.sk-status-readout`, `.sk-toolbar-label`, `.sk-toast-text`,
  `.sk-build-label` / `.sk-build-value`, `.sk-teleport-title`. A button's
  caption is an `.sk-text`, so it comes with the first.
- **Data text states `none`**, which the task did not list and the reference
  settled: it shadows static labels, button and tab captions and floater
  headers, and never a scroll-list cell, a line / text editor or a menu row
  (`NO_SHADOW` in `llscrolllistcell.cpp`, `lllineeditor.cpp`, `llmenugl.cpp`).
  So `.sk-list-surface` / `.sk-list-row` text, `.sk-text-field` and a combo
  option take `none` back, a button inside a list or row keeps its caption's
  shadow, and a menu label names none. It is legibility as well as fidelity:
  the reference even drops a shadow under text darker than 35% luminance,
  which a classic skin's black list text is.
- **Layout:** `TextShadow` is read only by `bevy_ui_render`'s extractor; no
  measure or layout system sees it, so a shadow cannot move a baseline.
- **The cost of `none`.** A stylesheet's `text-shadow: none` is a
  `TextShadow` with a transparent colour, not an absent component — and bevy
  extracted a second copy of every glyph for it all the same, which would
  have doubled the UI's glyph extraction in both flat skins. Fixed in the
  bevy fork (`f8e83bc`, skip a fully transparent shadow in
  `extract_text_shadows`); the rev is bumped on all 65 crates.

Tests (`skin_palette_resolves.rs`):
`a_skin_shadows_its_chrome_text_and_never_its_data` against the real Relief
sheet (22 specimens, three outcomes), and `a_flat_skin_draws_no_text_shadow`
over Graphite and Azure. The book's token table names `--text-shadow`, and its
chrome / data section says where the shadow stops.

Not reproduced: the reference's "soft" shadow is five offset copies at 30%
each (a halo weighted downwards); bevy draws one offset copy, so a Vintage
value is matched by eye.

### The live look (2026-09-24)

Gallery, Graphite → Relief, at a UI scale of 1.5. The shadow is there and
correct, and **very subtle**: black at about two-thirds under light text on
Graphite's near-black chrome shows only when zoomed in. That is the value, not
the mechanism. A skin with mid-grey chrome (Vintage's `#3e3e3e`) is where it
will read; retune it by eye when [[viewer-vintage-skin]] is built.

The same look found an unrelated defect in Relief's nine-sliced button art:
its frame changes thickness along the quad's diagonal at a fractional scale.
Filed as [[viewer-sliced-art-seam-at-fractional-ui-scale]].
