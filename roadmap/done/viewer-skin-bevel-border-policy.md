---
id: viewer-skin-bevel-border-policy
title: A bevel's light source has a handedness the logical-property ban denies
topic: viewer
status: done
origin: Vintage skin fidelity audit (2026-09-20)
points: 2
refs: [viewer-ui-skin-tokens, viewer-skin-image-backed-widgets]
---

Context: [context/viewer.md](../context/viewer.md),
[context/vintage-skin.md](../context/vintage-skin.md).

`BANNED_PHYSICAL_PROPERTIES` (`sl-viewer-ui-core/src/skin.rs`) rejects
`border-left-color` and `border-right-color`, and `logical_replacement` gives
the reason:

```text
"border-left-color" | "border-right-color" => "border-color (a colour has no
handedness)"
```

That is true of a *frame*. It is false of a **bevel**, which is the one thing
a colour border is used for in every classic skin: a light edge on the side
the light comes from and a dark edge opposite. The light source is a property
of the rendering, not of the writing direction — a mirrored bevel under an RTL
locale would look lit from the wrong side, which is why no desktop toolkit
mirrors one. So the ban has no logical spelling to offer, and the escape hatch
it suggests (`border-color`, one colour on all four sides) is precisely the
flat look a bevel is not.

`border-top-color` and `border-bottom-color` are *not* banned, so half a bevel
is already legal today — which makes the current state incoherent rather than
restrictive.

Worth knowing before choosing: `bevy_ui` 0.19 has **no inset box shadow**
(`ShadowStyle` is colour / offset / spread / blur only), so per-side border
colours and a nine-sliced image are the only two ways to draw a bevel at all.

## The decision

Three options, in the order they appeal:

1. **Carve out the pair as physical-on-purpose.** Drop the two colours from
   the ban list and say why in the doc comment: a bevel's light source is
   physical and must not mirror. Cheapest, and it makes the half-legal state
   whole.
2. **Add a `--bevel-light` / `--bevel-shadow` token pair and a `.sk-bevel`
   rule in `common.css`**, so skins express the intent once and no skin writes
   a physical property itself. The ban stays exactly as it is; one structural
   rule owns the handedness. Slightly more machinery, better story.
3. **Refuse bevels in CSS entirely** and let them come only from nine-sliced
   art ([[viewer-skin-image-backed-widgets]]). Coherent, but it makes the
   cheapest possible classic look require authored PNG art.

Option 2 reads best: the intent — "this edge is lit" — is genuinely a role,
and roles are what this token system is for.

Whichever is chosen, the doc comment on `logical_replacement` needs to stop
claiming a colour has no handedness, because the reason it gives is the part
that is wrong.

## Done when

The policy is decided and recorded in `skin.rs`, the scanner and its tests
agree with it, and a scratch rule draws a Vintage-style bevel — dark top-left,
light bottom-right, note the **inverted** convention measured in the context
file — that stays lit from the same corner when the root is flipped to
`dir="rtl"`.

## Done (2026-09-24)

**Option 2, with the tokens named for edges rather than for light.** The
suggested `--bevel-light` / `--bevel-shadow` pair with one `.sk-bevel` rule
cannot say what Vintage says: its push button *and* its text field are both
dark at the top-left (the widget-art table in the context file), while the
Windows convention raises a button and sinks a field — dark on opposite
corners. Which corner is lit is therefore a per-widget choice the skin makes,
not one light direction, so the tokens are:

| Token | Paints |
| --- | --- |
| `--button-bevel-top-left` / `--button-bevel-bottom-right` | `.sk-button` |
| `--field-bevel-top-left` / `--field-bevel-bottom-right` | `.sk-field`, `.sk-search-field` |

The ban is unchanged for skins: `border-left-color` / `border-right-color` stay
on `BANNED_PHYSICAL_PROPERTIES`, and `common.css` is the one sheet that writes
per-side colours. The policy is the doc comment on that constant; the
replacement hint no longer claims a colour has no handedness, and points a
skin author at the tokens. Both flat skins and the fallback give all four
tokens their `--control-border`, so nothing moves visibly. `.sk-field:disabled`
still sets a one-colour `border-color`, so a refused well is flat, as the
reference's disabled art is.

Tests:

- `skin::tests::common_css_writes_side_colours_only_from_bevel_tokens` — the
  licence is narrow: every side colour in `common.css` reads a `--*-bevel-*`
  token, and no other banned property appears there. It replaces the
  `common.css` half of `the_embedded_sheets_use_no_banned_property`, which
  now scans `fallback.css` only.
- `skin_palette_resolves::a_bevel_stays_lit_from_the_same_corner_under_rtl` —
  a fixture (`tests/assets/vintage-bevel.css`) sets Vintage's inverted bevel;
  the button, field and search box resolve it edge by edge, again after the
  root flips to `dir="rtl"`, and the refused field's `border-color` beats the
  bevel longhands.
