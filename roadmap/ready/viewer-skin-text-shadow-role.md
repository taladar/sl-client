---
id: viewer-skin-text-shadow-role
title: A text-shadow role — the trait that makes classic UI text look classic
topic: viewer
status: ready
origin: Vintage skin fidelity audit (2026-09-20)
points: 3
refs: [viewer-ui-skin-tokens, viewer-vintage-skin, viewer-ui-text-foundation]
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
