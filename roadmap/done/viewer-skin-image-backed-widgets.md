---
id: viewer-skin-image-backed-widgets
title: Nine-sliced image surfaces, so a skin can change a widget's shape
topic: viewer
status: done
origin: Vintage skin fidelity audit (2026-09-20)
points: 8
refs: [viewer-ui-skin-tokens, viewer-vintage-skin,
  viewer-skin-bevel-border-policy]
blocked_by: [viewer-skin-widget-state-classes]
---

Context: [context/viewer.md](../context/viewer.md),
[context/vintage-skin.md](../context/vintage-skin.md).

Today a skin can change a widget's **colour** and its **corner radius**, and
that is the whole of its power over shape. Every widget is a flat rounded
rectangle: a `Node` with a `BackgroundColor`, a uniform `BorderColor` and a
radius. The reference's classic skins are not built that way — a Vintage push
button is a raised bevel, a text field is sunken, a tab is a shouldered
trapezoid, a slider track is a groove. All of them are **nine-sliced images**,
with the slice insets declared beside the file:

```text
<texture name="PushButton_Off" file_name="widgets/PushButton_Off.png"
         scale.left="12" scale.top="12" scale.right="18" scale.bottom="13" />
```

## The good news: the engine already does this

`bevy_flair` maps `-bevy-image` onto `ImageNode.image` and
`-bevy-image-mode: sliced(t r b l)` onto `ImageNode.image_mode`, and registers
`ImageNode` with `auto_insert_remove` — so a CSS rule can **insert** an image
background onto a node that has none, and remove it again when the rule stops
matching. The reference's four `scale.*` insets map straight onto `sliced()`.
Nothing needs adding to the fork.

```css
.sk-button {
  -bevy-image: url("skins/vintage/widgets/push-button.png");
  -bevy-image-mode: sliced(12px 18px 13px 12px);
}
```

## What is actually missing

1. **Widgets that carry a class to target.** `.sk-button` exists; the tab,
   text field, combo, slider, scrollbar, checkbox and floater frame do not
   have one yet (that is [[viewer-audit-skin-token-coverage]]'s surface, and
   this task consumes its result rather than redoing it).
2. **State as a selector, not a paint** — [[viewer-skin-widget-state-classes]].
   Three textures per button is the reference's whole state model; a rule can
   only swap one if the state is selectable.
3. **Art to point at.** See the licensing note in the context file: the
   reference's PNG files are not ours to copy, so a classic-look skin ships art
   authored here to the measured geometry.
4. **A decision about `BackgroundColor` underneath.** An `ImageNode` draws over
   the node's background colour. A skin that supplies art wants the colour out
   of the way; one that does not wants it kept. Simplest rule that works:
   image-carrying rules set the background transparent in the same block, and
   the shipped flat skins set no image at all.

## Watch for

- **Sampler and filtering.** A nine-sliced 1–2 px bevel edge is exactly the
  case where a linear sampler smears the crisp line the whole look depends on.
  Check it on the first button, not after authoring a whole set.
- **UI scale.** The reference's slice insets are in unscaled pixels and its UI
  scales the whole image; ours lay out in logical pixels. A button that looks
  right at 1.0 and wrong at 1.5 is the failure mode.
- **The gallery is the check.** `sl-client-bevy-viewer-gallery` needs no login
  and is the fastest way to see every widget in a skin at once.

## Done when

The shipped widget set can be given an image surface per state from CSS alone,
one widget (the push button) is proven end to end with authored art in a
scratch skin, and the flat skins are visually unchanged.

## Done (2026-09-23)

The capability is proven end to end and shipped: **`themes/relief.css`**, a
Graphite overlay whose push button is a raised bevel drawn from nine-sliced
art, with a separate file per state. No Rust in the viewer knows the theme
exists — the rules do all of it, because `bevy_flair` registers `ImageNode`
with `auto_insert_remove` and so can *add* a surface to a node that has none
and take it away again.

```css
.sk-button {
  background-color: #00000000;
  border-width: 0;
  -bevy-image: url("skins/graphite/widgets/push-button.png");
  -bevy-image-mode: sliced(8px 8px 8px 8px);
}

.sk-button:hover {
  -bevy-image: url("skins/graphite/widgets/push-button-hover.png");
}
```

Answering the four things this task said were missing:

1. **Classes to target** — done by the work since the audit rather than here:
   47 classes now paint a surface (button, checkbox and its box, combo, field,
   floater and its title bar, tab, tab panel, both scrollbar parts, radio and
   its disc, menu, toolbar, the four toast kinds, swatch, trackball, list and
   table rows).
2. **State as a selector** — [[viewer-skin-widget-state-classes]], done; the
   `:hover` / `:active` / `:disabled` rules above are the proof it carries
   through to art.
3. **Art** — `assets/skins/graphite/widgets/*.png`, drawn by the
   **`sl-viewer-skin-art`** binary, a new workspace member. Ours: only the
   *geometry* is borrowed (a raised face, a lit edge where the light is and a
   shaded one opposite, over a hairline frame), and geometry is not what a
   licence covers. A generator rather than four committed blobs because a diff
   of its state table says what changed where a diff of four image blobs says
   nothing — and in Rust rather than the Python it started as, a tool that may
   not run again for a year should be pinned by `Cargo.lock` rather than by
   whatever interpreter and imaging library the machine happens to have.
4. **`BackgroundColor` underneath** — the rule that brings an image clears the
   background and the border **in the same block**, which is what keeps the
   flat skins untouched: they simply never set an image. A test asserts that
   Graphite, Azure and the dark theme insert no `ImageNode` at all.

### The sampler, checked on the first button as this task asked

`ImagePlugin`'s default sampler is **linear**, which is right for the world's
textures and wrong for a 2 px bevel: at any UI scale but 1.0 it blurs the line
the whole look rests on. Each PNG ships a `.meta` asking for nearest, and a
test asserts the meta is honoured — get one letter of that RON wrong and it is
ignored in silence.

Writing it turned up a trap worth knowing: `ImagePlugin` only **preregisters**
the image loader's extensions. The real `ImageLoader` is registered by
`bevy_render`'s wrapper, which wants a GPU, so a headless test that loads a PNG
must register the loader itself or the load sits pending for ever.

### Still to eyeball, and why it is not a test

**UI scale.** The slice insets are texture pixels and `max_corner_scale`
defaults to 1.0, so at scale 1.5 the corners keep their own size while the
button grows around them — a bevel that reads correctly at 1.0 may read thin at
1.5. Nothing headless can answer that; it belongs to the gallery pass, where
the skin switcher and the scale control are both to hand.

A **complete** classic-look skin is [[viewer-vintage-skin]], which this
unblocks: the mechanism is no longer the missing piece, only the art for the
other 46 surfaces.
