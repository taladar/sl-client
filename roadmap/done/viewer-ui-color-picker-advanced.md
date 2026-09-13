---
id: viewer-ui-color-picker-advanced
title: Color picker — SV square, hue strip, palette & eyedropper
topic: viewer
status: done
origin: user request (2026-07-24) while reviewing the RGB-slider color picker
  shipped with viewer-prim-texture-editing
blocked_by: [viewer-ui-color-picker]
refs: [viewer-prim-texture-editing, viewer-color-picker-face-pipette,
  viewer-color-picker-lsl-vector]
---

Context: [context/viewer.md](../context/viewer.md).

The colour picker shipped with the build-tool Texture tab
([[viewer-prim-texture-editing]]) is the useful core of
[[viewer-ui-color-picker]]: a reusable swatch + an `OpenColorPicker` /
`ColorPicked` floater with **R/G/B sliders, a live preview swatch, an
original-colour compare, and OK/Cancel**. This task adds the rest of the
reference's `LLFloaterColorPicker`:

- the **saturation/value square** + **hue strip** (2-D + 1-D drag pickers),
- the **hex** entry field,
- the **eyedropper** — pick a colour from anywhere on screen (the
  reference's `LLFloaterColorPicker::onColorSelect` pipette / `mPipetteBtn`,
  reading the framebuffer pixel under the cursor),
- the **saved-swatch palette** (persisted in the settings store),
- **live-apply while dragging** with revert-on-cancel (the reference applies
  continuously to the object being tuned; the current picker applies only on
  OK to avoid flooding the simulator — this task adds a throttled live path).

Reference (Firestorm, read-only): `llfloatercolorpicker.cpp`,
`floater_color_picker.xml`, `llcolorswatch.cpp`.

## Done (2026-09-13)

All five shipped, but the first one is **not** the control this task named, and
that is the interesting part.

### The square is hue × saturation, and the strip is luminance

The reference's field is not an HSV saturation/value square beside a hue strip.
`LLFloaterColorPicker::createUI` fills a 256×256 image with
`hslToRgb(x, y, 0.5)` — **hue across, saturation up, luminance fixed at a
half** — and the strip beside it is the **luminance**. It is an HSL picker, and
the difference is the one a user feels: "the same red, darker" is one drag down
the strip, where an HSV picker makes it a diagonal into a corner. So the picker
follows the reference, and the title above is the one thing in this task that
was not implemented as written.

`hsl_to_srgb` / `srgb_to_hsl` are the reference's `hslToRgb` and
`LLColor3::calcHSL`, on **sRGB** components as the reference's are. Both models
are kept side by side in the window state, as the reference keeps `curR/G/B`
beside `curH/S/L`: a grey has no hue, so a state that stored only RGB would
throw the user's hue away the moment they dragged the saturation to zero.
`the_two_models_follow_each_other` pins that the six sliders, the field and the
strip are all one state.

### Why the field is a picture and the strip is a gradient

The field is a generated 256×256 `Rgba8UnormSrgb` image, as the reference's is —
not two stacked `BackgroundGradient`s. HSL at `L = 0.5` *is* an exact linear
ramp from mid-grey to the pure hue in sRGB components, so the obvious
two-gradient stack looks right — but stacked gradient nodes are composited by
the GPU in **linear** space, so the drawn square would disagree with the colour
its own marker names, worst in the middle where people actually pick. An image
agrees by construction.

The strip *is* three gradient stops (black, the current hue and saturation at
`L = 0.5`, white) interpolated in `Srgba`, because `hslToRgb` is
piecewise-linear in `L` about the half-way colour — which is precisely what
those three stops describe. It is repainted from the state, so it tracks the
field.

Both controls sit in an outer box a marker larger than their picture, with the
picture centred in it, so a marker centred on any edge still lies inside the
control — the slider thumb's argument, in two dimensions — and the pointer is
measured against the **picture**, not the box it actually hit.
`a_field_click_aims_at_the_picture_inside_the_box` is the test for that, and it
is the one that would have caught measuring against the box: half a marker of
error everywhere, and neither end of the hue wheel quite reachable.

### The eyedropper samples the frame, and the reference's does not

This task specified "pick a colour from anywhere on screen … reading the
framebuffer pixel under the cursor", and attributed that to the reference's
pipette. The reference's pipette is `LLToolPipette`, which world-picks the face
under the cursor and reads that face's `LLTextureEntry` **tint** — an object
only, and unlit. Both are wanted and they are different tools, so the specified
one shipped and the reference's is [[viewer-color-picker-face-pipette]].

Two things about it are deliberate:

- The frame is read back **once**, when the eyedropper is armed, and sampled
  from there as the pointer moves. A read-back per frame would copy the whole
  window every frame of the gesture, and the screen does not change underneath a
  pointer that is only hunting for a pixel.
- Arming puts a screen-wide transparent catcher up, spawned *after* the capture
  so it is never in the frame being sampled. It absorbs the click that finishes
  the gesture — otherwise hunting for a pixel over a button would press it.
  `Escape` abandons the gesture and puts back the colour the picker held.

`sample_frame` is pure, so the two tests that matter (the texel under the
pointer, after the window's scale factor; and nothing at all off the edge) need
no renderer.

### The palette is the reference's, gesture and all

32 entries in two rows of sixteen, seeded from the reference's own
`ColorPaletteEntry01..32` (`colors.xml`), persisted to the **global** settings
scope under those same names. A click loads an entry. **Dragging the current
colour swatch onto a cell saves it there** — which is the reference's gesture
(`handleMouseUp`'s `mMouseDownInSwatch` branch, and the floater's own "(Drag
below to save.)" label), not an invention.

### Live-apply, and what turned out to already be there

This task's last bullet describes a picker that "applies only on OK" and asks
for a throttled live path. That was true when it was written; the live stream
landed with [[viewer-key-color-picker]]'s keying work, and the consumers already
preview locally and commit to the simulator only on OK — which is better than
the reference, whose own live-apply-during-drag is commented out with "generates
too much traffic and results in sporadic updates".

So what was missing was the reference's **switch**: the "Apply now" checkbox,
`ApplyColorImmediately`, registered here and defaulting on. Turned off, the
picker says nothing at all until OK. No throttle was added, because there is
nothing to throttle: a gesture that previews into a local material does not
reach the grid.

The flag is a **resource** the settings store merely persists, not a read of the
store — which is the shape the snapshot floater's toggles already use, and the
first version got it wrong. Reading the store directly left the checkbox inert
in every host that has no store (the gallery, a test fold): it drew itself
permanently ticked and swallowed every click, which is worse than not being
there at all, and no test could reach it.
`apply_now_toggles_and_gates_the_live_stream` runs in exactly that storeless
world.

The seven controls that can now change a colour — the field, the strip, six
sliders, the hex field, a palette cell and the eyedropper — do **not** each emit
the preview. `emit_live_preview` watches `Changed<ColorPickerState>` and speaks
once, which is how the eighth control will not be the one that silently does
not.

### Also

- The **hex** field commits on `Enter` or focus loss, as every other committed
  field in the viewer does, and is re-seeded from the state whenever the user is
  not in it. Anything that is not six hex digits is simply not a colour; the
  sync puts the current one back.
- The registry entry stopped being a `Stub` and became a real
  `FloaterContent::Specimen`, so the gallery and the floater sweep measure the
  actual field, strip, sliders, palette and reply row.

### The specimen needed the plugin, and a green sweep did not say so

Promoting the registry entry turned up a trap worth writing down. `floater_app`
(the chrome sweep) and `gallery::run` build a registry entry's content but add
no *feature* plugin — `ui_contract::install_element_hosting` is the **element**
sweep's set and deliberately has no floater manager, so a widget whose systems
open keyed windows cannot live in it. With all 60 chrome tests green the gallery
still drew the field as a flat grey box (the plugin is what *builds* its
picture), and the first press on Cancel killed the whole binary on an
unregistered `ColorPicked`. The sweep presses title bars, grips and close
clusters — never a specimen's own buttons — so it could not see either. The
plugin is now scheduled in both places.

With the plugin in, the specimen drew correctly and still answered **no click**,
which turned out to be a second, sharper version of the same blind spot. Every
handler found its state with `host_floater` — walk up to the enclosing window —
and that is right only because the live picker happens to put its state on the
floater root. A picker that is not a window (the specimen: a body root inside a
window that knows nothing about colours) is found, has no state, and every
gesture returns silently. `picker_window` now walks up to the nearest ancestor
carrying a `ColorPickerState`, which cannot miss that way; OK / Cancel keep the
old walk *as well*, because "which state do I answer with" and "which window do
I close" are two questions with two answers.
`a_picker_that_is_not_a_floater_still_answers_its_controls` pins it, and is the
only test here that is not driving a floater.

Deliberately not done, and split out rather than dropped:
[[viewer-color-picker-face-pipette]] and
[[viewer-color-picker-lsl-vector]] (the reference's LSL float triple and its
Copy LSL button).
