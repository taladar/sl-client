---
id: viewer-skin-image-backed-widgets
title: Nine-sliced image surfaces, so a skin can change a widget's shape
topic: viewer
status: ready
origin: Vintage skin fidelity audit (2026-09-20)
points: 8
refs: [viewer-ui-skin-tokens, viewer-vintage-skin, viewer-skin-bevel-border-policy]
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
