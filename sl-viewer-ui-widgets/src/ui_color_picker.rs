//! The **colour-picker floater + swatch widget** (`viewer-ui-color-picker`,
//! `viewer-ui-color-picker-advanced`): a reusable colour swatch any panel can
//! host, and the picker floater it opens — the reference's `LLColorSwatchCtrl` +
//! `LLFloaterColorPicker`.
//!
//! # Model
//!
//! - [`spawn_color_swatch`] drops a bordered button whose fill is its current
//!   colour ([`ColorSwatchValue`]); clicking it emits [`OpenColorPicker`] tagged
//!   with the swatch entity as the **requester**. A consumer keeps the swatch's
//!   [`ColorSwatchValue`] up to date (this module paints the fill from it) and
//!   reads [`ColorPicked`] filtered to its own swatch.
//! - The picker window carries the reference's whole surface: the **hue ×
//!   saturation field** and the **luminance strip** beside it, R/G/B and H/S/L
//!   sliders, a **hex** field, an **eyedropper**, the 32-entry **palette**, a
//!   live preview swatch to compare against the original, the **Apply now**
//!   toggle, and OK / Cancel.
//!
//! # The colour model is HSL, not HSV
//!
//! The reference's field is **hue across, saturation up, at a fixed luminance of
//! a half** (`LLFloaterColorPicker::createUI` fills a 256×256 image with
//! `hslToRgb(x, y, 0.5)`), and the strip beside it is **luminance**. That is not
//! the HSV square-and-hue-strip most modern pickers draw, and the difference is
//! visible: a viewer user reaching for "the same red, darker" moves down the
//! strip, not into a corner of the square. So this follows the reference —
//! [`hsl_to_srgb`] / [`srgb_to_hsl`] are its `hslToRgb` and `LLColor3::calcHSL`,
//! working on **sRGB** components, as the reference's do.
//!
//! Both models are kept side by side in one window's state, exactly as the
//! reference keeps `curR/curG/curB` beside `curH/curS/curL`: hue is undefined for
//! a grey, so a state that only stored RGB would lose the user's hue the moment
//! they dragged the saturation to zero.
//!
//! # How the field and the strip are drawn
//!
//! The **field** is a generated 256×256 image, as the reference's is — not a
//! stack of two `BackgroundGradient`s. HSL at `L = 0.5` is an exact linear ramp
//! from mid-grey to the pure hue *in sRGB components*, but two stacked gradient
//! nodes are composited by the GPU in **linear** space, so a saturation overlay
//! would draw a square that disagreed with the colour the marker on it names.
//! An image agrees by construction.
//!
//! The **strip** is a three-stop [`LinearGradient`] — black, the current hue and
//! saturation at `L = 0.5`, white — interpolated in
//! [`Srgba`](InterpolationColorSpace::Srgba). That one *is* exact: `hslToRgb` is
//! piecewise-linear in `L` about the half-way colour, which is precisely what
//! those three stops describe.
//!
//! # One window per swatch, of the window that opened it
//!
//! The picker is a **keyed** floater, keyed by the opening window and the
//! swatch's field together ([`picker_identity`]). It used to be one shared
//! window with a single `requester` slot, so a second swatch clicked while the
//! first was still being answered simply took the picker over and left the
//! first swatch's consumer holding a live preview nobody would ever commit or
//! revert. Two instances of one window — About Land is one per parcel — hit
//! that with the *same* swatch.
//!
//! # Divergences from the reference, and why
//!
//! - **The eyedropper samples the rendered frame, not a prim's tint.** The
//!   reference's pipette is `LLToolPipette`: it world-picks the face under the
//!   cursor and reads that face's `LLTextureEntry` colour, so it can only sample
//!   an object, and samples the tint *unlit*. This one reads back the window's
//!   own framebuffer, so it can sample anything on screen — the sky, a
//!   texture's own colour, another panel — at the cost of sampling the shaded
//!   pixel rather than the stored tint. Both are wanted; the face-tint pipette
//!   is [[viewer-color-picker-face-pipette]].
//! - **The frame is captured once, when the eyedropper is armed**, and sampled
//!   from there as the pointer moves. A read-back per frame would cost a full
//!   window copy per frame for the whole gesture, and the screen does not change
//!   underneath a pointer that is only hunting for a pixel.
//! - **No LSL tab.** The reference's RGB / LSL / Hex tab strip also carries a
//!   `<r, g, b>` float triple and a Copy LSL button; that is
//!   [[viewer-color-picker-lsl-vector]].
//!
//! Reference (Firestorm, read-only): `llfloatercolorpicker`, `llcolorswatch`,
//! `floater_color_picker.xml`, `v3color.cpp` (`LLColor3::calcHSL`).

use bevy::asset::RenderAssetUsages;
use bevy::input_focus::InputFocus;
use bevy::input_focus::tab_navigation::TabIndex;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use bevy::render::view::screenshot::{Screenshot, ScreenshotCaptured};
use bevy::text::EditableText;
use bevy::ui_widgets::{
    Button, Slider, SliderRange, SliderStep, SliderThumb, SliderValue, ValueChange,
};
use bevy::window::PrimaryWindow;
use bevy_flair::style::components::ClassList;

use crate::floater::{
    Floater, FloaterCaps, FloaterCommand, FloaterHandle, FloaterOp, FloaterOwner, FloaterSpec,
    FloaterSystems, KeyedFloaterOpen, KeyedFloaters, host_floater, picker_identity,
};
use crate::ui_text_input::{TextInputKind, TextInputSpec, spawn_text_input};
use sl_settings::{Scope, SettingValue};
use sl_viewer_settings::ViewerSettings;
use sl_viewer_ui_core::i18n::Translated;
use sl_viewer_ui_core::ui::{LogicalInset, LogicalRect, UiRoot, UiScaffoldSystems, column, row};
use sl_viewer_ui_core::ui_font::UiFont;

/// The picker's numeric-channel maximum (an sRGB byte).
const CHANNEL_MAX: f32 = 255.0;

/// The hue slider's maximum, in degrees.
const HUE_MAX: f32 = 360.0;

/// The saturation and luminance sliders' maximum, in percent.
const PERCENT_MAX: f32 = 100.0;

/// The RGB slider track width, in logical pixels.
const TRACK_WIDTH: f32 = 160.0;

/// The RGB slider track height.
const TRACK_HEIGHT: f32 = 14.0;

/// The slider thumb width.
const THUMB_WIDTH: f32 = 10.0;

/// A preview / original swatch's side length.
const SWATCH_SIZE: f32 = 40.0;

/// The hue × saturation field's side, in logical pixels — the reference's
/// `mRGBViewerImageWidth` at two thirds, which is what fits beside the sliders
/// without making the window taller than the screen's short edge.
const FIELD_SIZE: f32 = 176.0;

/// A field / strip marker's side, in logical pixels.
const MARKER_SIZE: f32 = 9.0;

/// The luminance strip's width, in logical pixels (the reference's
/// `mLumRegionWidth`).
const STRIP_WIDTH: f32 = 18.0;

/// The generated hue × saturation image's side, in texels — the reference's
/// `mRGBViewerImageWidth`. A `u16` so the loop's counter converts to `f32`
/// losslessly, without a cast.
const FIELD_TEXELS: u16 = 256;

/// How many palette entries there are — the reference's
/// `numPaletteColumns * numPaletteRows`.
const PALETTE_SIZE: usize = 32;

/// How many palette columns there are (the reference's `numPaletteColumns`).
const PALETTE_COLUMNS: usize = 16;

/// A palette cell's width, in logical pixels.
const PALETTE_CELL_WIDTH: f32 = 24.0;

/// A palette cell's height, in logical pixels.
const PALETTE_CELL_HEIGHT: f32 = 18.0;

/// The picker font size.
const PICKER_FONT: f32 = 13.0;

/// The hint under the current-colour swatch, and the drag-to-save target's
/// highlight: both are small, so they share the dimmed text colour.
const HINT_COLOR: Color = Color::srgb(0.62, 0.66, 0.74);

/// A bordered control's border colour.
const CONTROL_BORDER: Color = Color::srgba(0.4, 0.4, 0.45, 1.0);

/// A [disabled](bevy::ui::InteractionDisabled) swatch's border — dimmed so a
/// swatch the consumer cannot change reads as disabled.
const DISABLED_BORDER: Color = Color::srgba(0.28, 0.28, 0.32, 1.0);

/// A palette cell's border while the current colour is dragged over it — the
/// reference's complementary-colour highlight, as a single bright rim.
const DROP_BORDER: Color = Color::srgb(1.0, 0.85, 0.35);

/// A slider track's fill.
const TRACK_FILL: Color = Color::srgba(0.12, 0.12, 0.14, 1.0);

/// A slider thumb's fill.
const THUMB_FILL: Color = Color::srgb(0.75, 0.78, 0.85);

/// A button's background.
const BUTTON_BACKGROUND: Color = Color::srgba(0.18, 0.18, 0.2, 1.0);

/// A latched button's background — the eyedropper while it is armed.
const BUTTON_LATCHED: Color = Color::srgba(0.32, 0.38, 0.5, 1.0);

/// The text colour.
const TEXT_COLOR: Color = Color::srgb(0.9, 0.92, 0.96);

/// A field / strip marker's outline — dark, so a pale marker reads against a
/// pale corner of the field.
const MARKER_OUTLINE: Color = Color::srgb(0.05, 0.05, 0.07);

/// A field / strip marker's fill.
const MARKER_FILL: Color = Color::srgb(0.98, 0.98, 1.0);

/// The skin class for value text.
const VALUE_CLASS: &str = "sk-build-value";

/// The settings section the palette and the apply-immediately toggle live under.
const PICKER_SECTION: &[&str] = &["colorpicker"];

/// Whether a pick is handed to the requester **while** it is being made, or only
/// when OK is pressed — the reference's `ApplyColorImmediately`, drawn as its
/// "Apply now" checkbox.
pub const SETTING_APPLY_IMMEDIATELY: &str = "ApplyColorImmediately";

// ---------------------------------------------------------------------------
// The HSL model — the reference's, on sRGB components.

/// One channel of `hslToRgb`: the reference's `LLFloaterColorPicker::hueToRgb`,
/// verbatim. `hue` is in turns and may sit outside `0..1`; it is wrapped.
fn hue_channel(low: f32, high: f32, hue: f32) -> f32 {
    let hue = if hue < 0.0 {
        hue + 1.0
    } else if hue > 1.0 {
        hue - 1.0
    } else {
        hue
    };
    if hue * 6.0 < 1.0 {
        return low + (high - low) * 6.0 * hue;
    }
    if hue * 2.0 < 1.0 {
        return high;
    }
    if hue * 3.0 < 2.0 {
        return low + (high - low) * ((2.0 / 3.0) - hue) * 6.0;
    }
    low
}

/// Hue / saturation / luminance (each `0..1`) to sRGB components (each `0..1`) —
/// the reference's `LLFloaterColorPicker::hslToRgb`.
#[must_use]
pub fn hsl_to_srgb(hsl: [f32; 3]) -> [f32; 3] {
    let hue = hsl.first().copied().unwrap_or(0.0);
    let saturation = hsl.get(1).copied().unwrap_or(0.0);
    let luminance = hsl.get(2).copied().unwrap_or(0.0);
    if saturation < 0.000_01 {
        return [luminance, luminance, luminance];
    }
    let high = if luminance < 0.5 {
        luminance * (1.0 + saturation)
    } else {
        (luminance + saturation) - (saturation * luminance)
    };
    let low = 2.0 * luminance - high;
    [
        hue_channel(low, high, hue + (1.0 / 3.0)),
        hue_channel(low, high, hue),
        hue_channel(low, high, hue - (1.0 / 3.0)),
    ]
}

/// sRGB components (each `0..1`) to hue / saturation / luminance (each `0..1`) —
/// the reference's `LLColor3::calcHSL`. A grey has no hue, and gets `0.0`, as the
/// reference's does.
#[must_use]
pub fn srgb_to_hsl(rgb: [f32; 3]) -> [f32; 3] {
    let red = rgb.first().copied().unwrap_or(0.0);
    let green = rgb.get(1).copied().unwrap_or(0.0);
    let blue = rgb.get(2).copied().unwrap_or(0.0);
    let low = red.min(green).min(blue);
    let high = red.max(green).max(blue);
    let span = high - low;
    let luminance = f32::midpoint(high, low);
    if span <= 0.0 {
        return [0.0, 0.0, luminance];
    }
    let saturation = if luminance < 0.5 {
        span / (high + low)
    } else {
        span / (2.0 - high - low)
    };
    let delta = |channel: f32| (((high - channel) / 6.0) + (span / 2.0)) / span;
    let mut hue = if red >= high {
        delta(blue) - delta(green)
    } else if green >= high {
        (1.0 / 3.0) + delta(red) - delta(blue)
    } else {
        (2.0 / 3.0) + delta(green) - delta(red)
    };
    if hue < 0.0 {
        hue += 1.0;
    }
    if hue > 1.0 {
        hue -= 1.0;
    }
    [hue, saturation, luminance]
}

// ---------------------------------------------------------------------------
// The swatch widget.

/// A reusable colour swatch's current value; this module paints the swatch fill
/// from it, and a consumer reads / writes it.
#[derive(Component, Debug, Clone, Copy)]
pub struct ColorSwatchValue(pub Color);

/// A swatch's **field** name — which control it is, and so half of the identity
/// of the picker window it opens (see [`OpenColorPicker::field`]). The texture
/// picker's `TextureSwatchField`, for colours.
#[derive(Component, Debug, Clone)]
struct ColorSwatchField(Box<str>);

/// Spawn a colour swatch under `parent`: a bordered button filled with `initial`
/// that opens the picker on click, tagged with `element` for its [`Name`]. The
/// returned entity is the **requester** a [`ColorPicked`] reply is matched by.
pub fn spawn_color_swatch(
    commands: &mut Commands,
    parent: Entity,
    element: &str,
    tab_index: i32,
    initial: Color,
) -> Entity {
    commands
        .spawn((
            Button,
            TabIndex(tab_index),
            Node {
                width: Val::Px(SWATCH_SIZE),
                height: Val::Px(SWATCH_SIZE),
                border: UiRect::all(Val::Px(1.0)),
                ..Default::default()
            },
            BorderColor::all(CONTROL_BORDER),
            BackgroundColor(initial),
            ColorSwatchValue(initial),
            ColorSwatchField(Box::from(element)),
            Pickable::default(),
            Name::new(format!("{element}:color-swatch")),
            ChildOf(parent),
        ))
        .observe(open_picker_from_swatch)
        .id()
}

/// Request the picker for the clicked swatch, seeding it with the swatch's colour.
fn open_picker_from_swatch(
    press: On<Pointer<Press>>,
    swatches: Query<(&ColorSwatchValue, &ColorSwatchField)>,
    disabled: Query<(), With<bevy::ui::InteractionDisabled>>,
    mut opens: MessageWriter<OpenColorPicker>,
) {
    if press.button != PointerButton::Primary {
        return;
    }
    // A disabled swatch does not open the picker (scroll past it still works).
    if disabled.contains(press.entity) {
        return;
    }
    if let Ok((value, field)) = swatches.get(press.entity) {
        opens.write(OpenColorPicker {
            requester: press.entity,
            field: field.0.clone(),
            current: value.0,
        });
    }
}

/// Dim a colour swatch's border while it is
/// [disabled](bevy::ui::InteractionDisabled), restoring it when enabled.
fn reflect_color_swatch_disabled(
    mut swatches: Query<
        (&mut BorderColor, Has<bevy::ui::InteractionDisabled>),
        With<ColorSwatchValue>,
    >,
) {
    for (mut border, disabled) in &mut swatches {
        let wanted = BorderColor::all(if disabled {
            DISABLED_BORDER
        } else {
            CONTROL_BORDER
        });
        if *border != wanted {
            *border = wanted;
        }
    }
}

// ---------------------------------------------------------------------------
// Messages.

/// Open the colour picker for `requester`, seeded with `current`.
#[derive(Message, Debug, Clone)]
pub struct OpenColorPicker {
    /// The swatch (or other widget) the reply is tagged back to.
    pub requester: Entity,
    /// **Which** of the opening window's pickers this is — the swatch's element
    /// id, or a name the opener chooses. Two swatches of one window are two
    /// picker windows, and so is the same swatch in two instances of that
    /// window; see `picker_identity`.
    pub field: Box<str>,
    /// The colour to open on.
    pub current: Color,
}

/// The chosen colour, tagged back to the [`requester`](Self::requester) that
/// opened the picker. Emitted **continuously** while the colour is being chosen
/// (with [`final_pick`](Self::final_pick) `false`) so a consumer can
/// live-preview, and once on **OK** with `final_pick` `true`; **Cancel** emits
/// the original colour with `final_pick` `false` so the consumer reverts its
/// preview.
///
/// The live stream is the reference's `ApplyColorImmediately` (its "Apply now"
/// checkbox, [`SETTING_APPLY_IMMEDIATELY`]): turned off, the picker says nothing
/// at all until OK.
#[derive(Message, Debug, Clone, Copy)]
pub struct ColorPicked {
    /// The widget that opened the picker.
    pub requester: Entity,
    /// The chosen colour.
    pub color: Color,
    /// Whether this is the committed choice (**OK**) rather than a live-preview
    /// or revert update.
    pub final_pick: bool,
}

// ---------------------------------------------------------------------------
// The saved palette.

/// The 32 saved swatches under the picker — the reference's `mPalette`, whose
/// entries it keeps in the UI colour table as `ColorPaletteEntry01..32` and
/// rewrites when a colour is dragged onto one.
#[derive(Resource, Debug, Clone)]
pub struct ColorPalette([Color; PALETTE_SIZE]);

impl Default for ColorPalette {
    /// The reference's shipped palette (`colors.xml`), entry for entry.
    fn default() -> Self {
        Self([
            Color::srgb(0.0, 0.0, 0.0),
            Color::srgb(0.5, 0.5, 0.5),
            Color::srgb(0.5, 0.0, 0.0),
            Color::srgb(0.5, 0.5, 0.0),
            Color::srgb(0.0, 0.5, 0.0),
            Color::srgb(0.0, 0.5, 0.5),
            Color::srgb(0.0, 0.0, 0.5),
            Color::srgb(0.5, 0.0, 0.5),
            Color::srgb(0.5, 0.5, 0.0),
            Color::srgb(0.0, 0.25, 0.25),
            Color::srgb(0.0, 0.5, 1.0),
            Color::srgb(0.0, 0.25, 0.5),
            Color::srgb(0.5, 0.0, 1.0),
            Color::srgb(0.5, 0.25, 0.0),
            Color::srgb(1.0, 1.0, 1.0),
            Color::srgb(1.0, 1.0, 1.0),
            Color::srgb(1.0, 1.0, 1.0),
            Color::srgb(0.75, 0.75, 0.75),
            Color::srgb(1.0, 0.0, 0.0),
            Color::srgb(1.0, 1.0, 0.0),
            Color::srgb(0.0, 1.0, 0.0),
            Color::srgb(0.0, 1.0, 1.0),
            Color::srgb(0.0, 0.0, 1.0),
            Color::srgb(1.0, 0.0, 1.0),
            Color::srgb(1.0, 1.0, 0.5),
            Color::srgb(0.0, 1.0, 0.5),
            Color::srgb(0.5, 1.0, 1.0),
            Color::srgb(0.5, 0.5, 1.0),
            Color::srgb(1.0, 0.0, 0.5),
            Color::srgb(1.0, 0.5, 0.0),
            Color::srgb(1.0, 1.0, 1.0),
            Color::srgb(1.0, 1.0, 1.0),
        ])
    }
}

impl ColorPalette {
    /// The colour in `index`, or black for an index past the end.
    #[must_use]
    pub fn entry(&self, index: usize) -> Color {
        self.0.get(index).copied().unwrap_or(Color::BLACK)
    }
}

/// The settings name of palette entry `index` — the reference's
/// `ColorPaletteEntry%02d`, one-based.
fn palette_setting_name(index: usize) -> String {
    format!("ColorPaletteEntry{:02}", index.saturating_add(1))
}

/// Register the picker's persisted settings: the 32 palette entries and the
/// apply-immediately toggle.
fn register_color_picker_settings(settings: Option<ResMut<ViewerSettings>>) {
    let Some(mut settings) = settings else {
        return;
    };
    let defaults = ColorPalette::default();
    for index in 0..PALETTE_SIZE {
        let srgba = defaults.entry(index).to_srgba();
        settings.register_in(
            PICKER_SECTION,
            &palette_setting_name(index),
            SettingValue::Color4(srgba.to_f32_array()),
            "A saved colour-picker palette swatch",
        );
    }
    settings.register_in(
        PICKER_SECTION,
        SETTING_APPLY_IMMEDIATELY,
        SettingValue::Bool(true),
        "Hand a colour to whatever is being tinted while it is being picked",
    );
}

/// Whether the picker hands a colour over **while** it is being chosen, rather
/// than only on OK — the reference's `ApplyColorImmediately`, drawn as its
/// "Apply now" checkbox.
///
/// A resource rather than a read of the settings store, and that is the whole
/// point: the store is *persistence*, not the live value. Reading it directly
/// made the checkbox inert in every host that has no store — the gallery, a test
/// fold — where it drew itself permanently ticked and swallowed every click,
/// which is worse than not being there. It is seeded from the store at startup
/// and written back when toggled; without a store it is simply this session's
/// value, and the control still works.
#[derive(Resource, Debug, Clone, Copy)]
pub struct ApplyColorImmediately(pub bool);

impl Default for ApplyColorImmediately {
    /// On, as the reference's setting is.
    fn default() -> Self {
        Self(true)
    }
}

/// Seed the in-memory [`ColorPalette`] and [`ApplyColorImmediately`] from the
/// store, once the settings resource exists.
fn load_color_picker_settings(
    settings: Option<Res<ViewerSettings>>,
    mut palette: ResMut<ColorPalette>,
    mut apply_now: ResMut<ApplyColorImmediately>,
) {
    let Some(settings) = settings else {
        return;
    };
    for index in 0..PALETTE_SIZE {
        let Ok([red, green, blue, alpha]) =
            settings.store().get_color4(&palette_setting_name(index))
        else {
            continue;
        };
        if let Some(slot) = palette.0.get_mut(index) {
            *slot = Color::srgba(red, green, blue, alpha);
        }
    }
    if let Ok(stored) = settings.store().get_bool(SETTING_APPLY_IMMEDIATELY) {
        apply_now.0 = stored;
    }
}

// ---------------------------------------------------------------------------
// Per-window state.

/// One picker window's live state — a component on the window root, so it dies
/// with the instance.
#[derive(Component, Debug, Default)]
struct ColorPickerState {
    /// The widget that opened it, or `None` when closed.
    requester: Option<Entity>,
    /// The colour it opened on (for Cancel / the original swatch).
    original: Color,
    /// The three sRGB channel values, 0..255 — the reference's `curR/G/B`.
    channels: [f32; 3],
    /// Hue, saturation and luminance, each 0..1 — the reference's `curH/S/L`,
    /// kept beside the channels because a grey has no hue to recover.
    hsl: [f32; 3],
    /// The last colour handed to the requester as a live preview, so an
    /// unchanged state says nothing twice.
    previewed: Color,
}

impl ColorPickerState {
    /// The current colour built from the channel bytes.
    fn current(&self) -> Color {
        let channel = |index: usize| byte(self.channels.get(index).copied().unwrap_or(0.0));
        Color::srgb_u8(channel(0), channel(1), channel(2))
    }

    /// A state opened on `color`, answering `requester`.
    fn opened_on(requester: Entity, color: Color) -> Self {
        let mut state = Self {
            requester: Some(requester),
            original: color,
            channels: [0.0; 3],
            hsl: [0.0; 3],
            previewed: color,
        };
        state.set_color(color);
        state.previewed = state.current();
        state
    }

    /// Drive the state from sRGB channel bytes, recomputing the HSL model — the
    /// reference's `setCurRgb`.
    fn set_channels(&mut self, channels: [f32; 3]) {
        self.channels = channels;
        let unit = |index: usize| channels.get(index).copied().unwrap_or(0.0) / CHANNEL_MAX;
        self.hsl = srgb_to_hsl([unit(0), unit(1), unit(2)]);
    }

    /// Drive the state from the HSL model, recomputing the channel bytes — the
    /// reference's `setCurHsl`.
    fn set_hsl(&mut self, hsl: [f32; 3]) {
        self.hsl = hsl;
        let rgb = hsl_to_srgb(hsl);
        let channel = |index: usize| (rgb.get(index).copied().unwrap_or(0.0) * CHANNEL_MAX).round();
        self.channels = [channel(0), channel(1), channel(2)];
    }

    /// Drive the state from a whole colour.
    fn set_color(&mut self, color: Color) {
        let srgba = color.to_srgba();
        self.set_channels([
            (srgba.red * CHANNEL_MAX).round(),
            (srgba.green * CHANNEL_MAX).round(),
            (srgba.blue * CHANNEL_MAX).round(),
        ]);
    }

    /// The colour the field and the strip are drawn around: the current hue and
    /// saturation at the mid luminance, which is the strip's middle stop and the
    /// field's own colour under the marker.
    fn mid_luminance_color(&self) -> Color {
        let hue = self.hsl.first().copied().unwrap_or(0.0);
        let saturation = self.hsl.get(1).copied().unwrap_or(0.0);
        let [red, green, blue] = hsl_to_srgb([hue, saturation, 0.5]);
        Color::srgb(red, green, blue)
    }
}

/// One picker window's entities.
#[derive(Component, Debug)]
struct ColorPickerUi {
    /// The live preview swatch — also the handle dragged onto a palette cell.
    preview: Entity,
    /// The original-colour swatch.
    original: Entity,
    /// The marker on the hue × saturation field. The field itself is not held:
    /// its picture never changes, so nothing syncs it — the pointer reaches it
    /// through its own observers.
    field_marker: Entity,
    /// The luminance strip's inner (gradient-painted) box.
    strip: Entity,
    /// The marker on the strip.
    strip_marker: Entity,
    /// The six sliders, R/G/B then H/S/L.
    sliders: [Entity; 6],
    /// The six channel value labels, in the same order.
    labels: [Entity; 6],
    /// The hex field.
    hex: Entity,
    /// The 32 palette cells.
    palette: [Entity; PALETTE_SIZE],
    /// The eyedropper button, latched while it is armed.
    pipette: Entity,
    /// The Apply-now checkbox's glyph node.
    apply_glyph: Entity,
}

/// Which value a slider drives.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
enum ChannelAxis {
    /// The sRGB red byte.
    Red,
    /// The sRGB green byte.
    Green,
    /// The sRGB blue byte.
    Blue,
    /// The hue, in degrees.
    Hue,
    /// The saturation, in percent.
    Saturation,
    /// The luminance, in percent.
    Luminance,
}

impl ChannelAxis {
    /// Every axis, in the order the picker stacks them.
    const ALL: [Self; 6] = [
        Self::Red,
        Self::Green,
        Self::Blue,
        Self::Hue,
        Self::Saturation,
        Self::Luminance,
    ];

    /// The one-letter label the slider row carries.
    const fn label(self) -> &'static str {
        match self {
            Self::Red => "R",
            Self::Green => "G",
            Self::Blue => "B",
            Self::Hue => "H",
            Self::Saturation => "S",
            Self::Luminance => "L",
        }
    }

    /// The slider's upper bound: a byte for the channels, degrees for hue, and
    /// percent for saturation and luminance — the reference's spinner ranges.
    const fn maximum(self) -> f32 {
        match self {
            Self::Red | Self::Green | Self::Blue => CHANNEL_MAX,
            Self::Hue => HUE_MAX,
            Self::Saturation | Self::Luminance => PERCENT_MAX,
        }
    }

    /// Where this axis sits in [`ChannelAxis::ALL`], and so in the UI's arrays.
    const fn slot(self) -> usize {
        match self {
            Self::Red => 0,
            Self::Green => 1,
            Self::Blue => 2,
            Self::Hue => 3,
            Self::Saturation => 4,
            Self::Luminance => 5,
        }
    }

    /// This axis's current value in the picker's own units.
    fn read(self, state: &ColorPickerState) -> f32 {
        let hsl = |index: usize, scale: f32| state.hsl.get(index).copied().unwrap_or(0.0) * scale;
        match self {
            Self::Red => state.channels.first().copied().unwrap_or(0.0),
            Self::Green => state.channels.get(1).copied().unwrap_or(0.0),
            Self::Blue => state.channels.get(2).copied().unwrap_or(0.0),
            Self::Hue => hsl(0, HUE_MAX),
            Self::Saturation => hsl(1, PERCENT_MAX),
            Self::Luminance => hsl(2, PERCENT_MAX),
        }
    }

    /// Write this axis into the state, recomputing the model it is not part of.
    fn write(self, state: &mut ColorPickerState, value: f32) {
        match self {
            Self::Red | Self::Green | Self::Blue => {
                let mut channels = state.channels;
                if let Some(slot) = channels.get_mut(self.slot()) {
                    *slot = value;
                }
                state.set_channels(channels);
            }
            Self::Hue | Self::Saturation | Self::Luminance => {
                let mut hsl = state.hsl;
                // The HSL slots trail the three channel slots.
                if let Some(slot) = hsl.get_mut(self.slot().saturating_sub(3)) {
                    *slot = value / self.maximum();
                }
                state.set_hsl(hsl);
            }
        }
    }
}

/// A picker action button.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
enum PickerButton {
    /// Accept the current colour.
    Ok,
    /// Discard and close.
    Cancel,
}

/// The picker's live preview swatch — the reference's "Current color" box, and
/// the handle its drag-to-save gesture starts from.
#[derive(Component, Debug, Clone, Copy)]
struct PreviewSwatch;

/// The hue × saturation field's outer box: what the pointer aims at.
#[derive(Component, Debug, Clone, Copy)]
struct HueSaturationField;

/// The luminance strip's outer box.
#[derive(Component, Debug, Clone, Copy)]
struct LuminanceStrip;

/// A palette cell, and which entry it shows.
#[derive(Component, Debug, Clone, Copy)]
struct PaletteCell(usize);

/// The eyedropper button.
#[derive(Component, Debug, Clone, Copy)]
struct PipetteButton;

/// The Apply-now checkbox.
#[derive(Component, Debug, Clone, Copy)]
struct ApplyNowToggle;

/// The picker's hex field.
#[derive(Component, Debug, Clone, Copy)]
struct HexField;

/// Which picker window a stray node (the eyedropper's screen-wide shade) belongs
/// to, since it does not live under that window.
#[derive(Component, Debug, Clone, Copy)]
struct PipetteShade(Entity);

/// A gesture in progress on the field or the strip, so a drag that wanders off
/// the control keeps driving it.
#[derive(Component, Debug, Clone, Copy, Default)]
struct PickerDrag(bool);

/// One picker window's armed eyedropper.
#[derive(Component, Debug)]
struct Eyedropper {
    /// The window frame captured when the eyedropper was armed — `None` until the
    /// read-back lands.
    frame: Option<Image>,
    /// The colour the picker held when it was armed, restored if the gesture is
    /// abandoned with `Escape`.
    before: Color,
    /// The screen-wide click-catcher, once the frame is in.
    shade: Option<Entity>,
}

/// The shared hue × saturation picture, built once and pointed at by every
/// picker window's field.
#[derive(Resource, Debug, Default)]
struct FieldImage(Option<Handle<Image>>);

// ---------------------------------------------------------------------------
// The plugin.

/// The plugin wiring the colour picker into the viewer.
#[derive(Debug, Clone, Copy, Default)]
pub struct ColorPickerPlugin;

impl Plugin for ColorPickerPlugin {
    /// Register the messages, state, floater, and systems.
    fn build(&self, app: &mut App) {
        app.add_message::<OpenColorPicker>()
            .add_message::<ColorPicked>()
            .init_resource::<ColorPalette>()
            .init_resource::<ApplyColorImmediately>()
            .init_resource::<FieldImage>()
            .init_resource::<HexFieldFocus>()
            .add_systems(
                Startup,
                (
                    register_color_picker_settings,
                    load_color_picker_settings,
                    build_field_image,
                )
                    .chain(),
            )
            .add_systems(
                Update,
                // Ordered, not a bare tuple: the visual sync reads the slider
                // values the open handler seeds (and needs its commands applied
                // to see them), so an unordered pair would leave the thumbs a
                // frame behind the colour the picker opened on.
                (
                    // After the manager's command pass — see `FloaterSystems`:
                    // the click on a swatch also raises the window it landed
                    // in, and the later raise wins the z-order.
                    handle_open_color_picker
                        .after(FloaterSystems::Commands)
                        .after(UiScaffoldSystems::SpawnRoot),
                    attach_field_image,
                    commit_hex_field,
                    drive_eyedropper,
                    emit_live_preview,
                    sync_color_picker_visual,
                    apply_color_swatch_fill,
                    reflect_color_swatch_disabled,
                )
                    .chain(),
            );
    }
}

/// The color picker floater's [`FloaterSpec`] — shared with the `FLOATERS`
/// registry, so the swept window is the one the viewer spawns.
#[must_use]
pub fn color_picker_floater_spec() -> FloaterSpec {
    FloaterSpec {
        id: "color-picker",
        title: String::from("Color Picker"),
        // Clear of the Build Tools floater (which spans the upper-left), so
        // the picker is never hidden behind it.
        position: Vec2::new(520.0, 220.0),
        default_size: None,
        min_size: None,
        dock_host: None,
        caps: FloaterCaps {
            resizable: false,
            minimizable: false,
            closable: true,
            dockable: false,
        },
    }
}

// ---------------------------------------------------------------------------
// The generated hue × saturation picture.

/// Build the hue × saturation picture: hue across, saturation **up**, at the mid
/// luminance — the reference's `createUI` image, texel for texel.
#[must_use]
fn hue_saturation_image() -> Image {
    let side = FIELD_TEXELS;
    let last = f32::from(side.saturating_sub(1)).max(1.0);
    let mut data: Vec<u8> = Vec::with_capacity(
        usize::from(side)
            .saturating_mul(usize::from(side))
            .saturating_mul(4),
    );
    for row in 0..side {
        // Row zero is the picture's **top**, and the top is full saturation: the
        // reference's image is addressed bottom-up, so its row zero (saturation
        // zero) is the bottom one.
        let saturation = 1.0 - (f32::from(row) / last);
        for column in 0..side {
            let hue = f32::from(column) / last;
            let [red, green, blue] = hsl_to_srgb([hue, saturation, 0.5]);
            data.extend_from_slice(&[
                byte(red * CHANNEL_MAX),
                byte(green * CHANNEL_MAX),
                byte(blue * CHANNEL_MAX),
                u8::MAX,
            ]);
        }
    }
    Image::new(
        Extent3d {
            width: u32::from(side),
            height: u32::from(side),
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        data,
        // sRGB-encoded, so what the shader shows is exactly the byte written —
        // which is the whole point of drawing the field as a picture.
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::RENDER_WORLD,
    )
}

/// Build the shared field picture once, if this app has an image store at all
/// (a headless test app does not, and the picker still works without it).
fn build_field_image(images: Option<ResMut<Assets<Image>>>, mut field: ResMut<FieldImage>) {
    let Some(mut images) = images else {
        return;
    };
    field.0 = Some(images.add(hue_saturation_image()));
}

/// Point a freshly-spawned field at the shared picture. Separate from the
/// content build because the build runs from `Commands` and the handle may not
/// exist yet in an app whose asset store arrives late.
fn attach_field_image(
    field: Res<FieldImage>,
    fields: Query<Entity, (With<FieldPicture>, Without<ImageNode>)>,
    mut commands: Commands,
) {
    let Some(handle) = field.0.clone() else {
        return;
    };
    for entity in &fields {
        commands
            .entity(entity)
            .insert(ImageNode::new(handle.clone()));
    }
}

/// The inner node of a hue × saturation field — the one the picture is drawn on.
#[derive(Component, Debug, Clone, Copy)]
struct FieldPicture;

// ---------------------------------------------------------------------------
// Content.

/// Build one colour-picker window's content, titling the window with it.
fn build_color_picker_content(handle: &FloaterHandle, commands: &mut Commands) -> ColorPickerUi {
    commands
        .entity(handle.title_text)
        .insert(Translated::new("color-picker-title"));
    build_color_picker_body(commands, handle.content).1
}

/// Build the picker's body under `parent`: the field and strip beside the
/// sliders, the hex row, the compare swatches, the palette and the reply row.
/// Returns the body's own root — what a host that is not a floater window hangs
/// the state on — and the entities the sync writes.
fn build_color_picker_body(commands: &mut Commands, parent: Entity) -> (Entity, ColorPickerUi) {
    let content = commands
        .spawn((
            Node {
                padding: UiRect::all(Val::Px(8.0)),
                ..column(Val::Px(8.0))
            },
            Name::new("color-picker-content"),
            ChildOf(parent),
        ))
        .id();

    // The aiming row: the field, the luminance strip, and the slider stack.
    let aiming = commands
        .spawn((
            Node {
                align_items: AlignItems::Start,
                ..row(Val::Px(10.0))
            },
            ChildOf(content),
        ))
        .id();
    let field_marker = spawn_hue_saturation_field(commands, aiming);
    let (strip, strip_marker) = spawn_luminance_strip(commands, aiming);

    let stack = commands
        .spawn((
            Node {
                ..column(Val::Px(4.0))
            },
            ChildOf(aiming),
        ))
        .id();
    let mut sliders = [Entity::PLACEHOLDER; 6];
    let mut labels = [Entity::PLACEHOLDER; 6];
    for axis in ChannelAxis::ALL {
        let (slider, label) = spawn_channel_row(commands, stack, axis);
        if let Some(slot) = sliders.get_mut(axis.slot()) {
            *slot = slider;
        }
        if let Some(slot) = labels.get_mut(axis.slot()) {
            *slot = label;
        }
    }
    let hex = spawn_hex_row(commands, stack);

    // The comparison row: the current colour (the drag handle), the original,
    // the eyedropper and the drag-to-save hint.
    let compare = commands
        .spawn((
            Node {
                align_items: AlignItems::Center,
                ..row(Val::Px(10.0))
            },
            ChildOf(content),
        ))
        .id();
    let preview = spawn_compare_swatch(commands, compare, "color-picker-preview", true);
    let original = spawn_compare_swatch(commands, compare, "color-picker-original", false);
    let pipette = spawn_pipette_button(commands, compare);
    commands.spawn((
        Text::default(),
        Translated::new("color-picker-drag-hint"),
        UiFont::Sans.at(PICKER_FONT),
        TextColor(HINT_COLOR),
        ClassList::new_with_classes([VALUE_CLASS]),
        ChildOf(compare),
    ));

    let palette = spawn_palette(commands, content);

    // The reply row: Apply now, then OK / Cancel.
    let buttons = commands
        .spawn((
            Node {
                align_items: AlignItems::Center,
                justify_content: JustifyContent::SpaceBetween,
                ..row(Val::Px(8.0))
            },
            ChildOf(content),
        ))
        .id();
    let apply_glyph = spawn_apply_now_toggle(commands, buttons);
    let replies = commands
        .spawn((
            Node {
                align_items: AlignItems::Center,
                ..row(Val::Px(8.0))
            },
            ChildOf(buttons),
        ))
        .id();
    spawn_picker_button(commands, replies, PickerButton::Ok, "color-picker-ok");
    spawn_picker_button(
        commands,
        replies,
        PickerButton::Cancel,
        "color-picker-cancel",
    );

    (
        content,
        ColorPickerUi {
            preview,
            original,
            field_marker,
            strip,
            strip_marker,
            sliders,
            labels,
            hex,
            palette,
            pipette,
            apply_glyph,
        },
    )
}

/// Spawn the hue × saturation field: an outer box a marker's width larger than
/// the picture, so a marker centred on any edge of the picture still lies inside
/// the control (the slider thumb's argument, in two dimensions). Returns the
/// marker; the outer box the pointer aims at carries its own observers.
fn spawn_hue_saturation_field(commands: &mut Commands, parent: Entity) -> Entity {
    let outer = commands
        .spawn((
            Node {
                width: Val::Px(FIELD_SIZE + MARKER_SIZE),
                height: Val::Px(FIELD_SIZE + MARKER_SIZE),
                ..Default::default()
            },
            HueSaturationField,
            PickerDrag::default(),
            Pickable::default(),
            Name::new("color-picker-field"),
            ChildOf(parent),
        ))
        .observe(on_aim_press)
        .observe(on_aim_drag)
        .observe(on_aim_drag_end)
        .observe(on_aim_release)
        .observe(on_aim_cancel)
        .id();
    commands.spawn((
        Node {
            position_type: PositionType::Absolute,
            left: Val::Px(MARKER_SIZE / 2.0),
            top: Val::Px(MARKER_SIZE / 2.0),
            width: Val::Px(FIELD_SIZE),
            height: Val::Px(FIELD_SIZE),
            border: UiRect::all(Val::Px(1.0)),
            ..Default::default()
        },
        BorderColor::all(CONTROL_BORDER),
        BackgroundColor(Color::srgb(0.5, 0.5, 0.5)),
        FieldPicture,
        Pickable::IGNORE,
        Name::new("color-picker-field-picture"),
        ChildOf(outer),
    ));
    spawn_marker(commands, outer, "color-picker-field-marker")
}

/// Spawn the luminance strip beside the field, inset by the same half marker so
/// the two line up. Returns the **inner** (gradient-painted) box and the marker;
/// the outer box is what the pointer aims at and is the marker's parent.
fn spawn_luminance_strip(commands: &mut Commands, parent: Entity) -> (Entity, Entity) {
    let outer = commands
        .spawn((
            Node {
                width: Val::Px(STRIP_WIDTH),
                height: Val::Px(FIELD_SIZE + MARKER_SIZE),
                ..Default::default()
            },
            LuminanceStrip,
            PickerDrag::default(),
            Pickable::default(),
            Name::new("color-picker-strip"),
            ChildOf(parent),
        ))
        .observe(on_aim_press)
        .observe(on_aim_drag)
        .observe(on_aim_drag_end)
        .observe(on_aim_release)
        .observe(on_aim_cancel)
        .id();
    let inner = commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(0.0),
                top: Val::Px(MARKER_SIZE / 2.0),
                width: Val::Px(STRIP_WIDTH),
                height: Val::Px(FIELD_SIZE),
                border: UiRect::all(Val::Px(1.0)),
                ..Default::default()
            },
            BorderColor::all(CONTROL_BORDER),
            luminance_gradient(Color::srgb(0.5, 0.5, 0.5)),
            Pickable::IGNORE,
            Name::new("color-picker-strip-picture"),
            ChildOf(outer),
        ))
        .id();
    let marker = spawn_marker(commands, outer, "color-picker-strip-marker");
    commands.entity(marker).insert(Node {
        position_type: PositionType::Absolute,
        left: Val::Px(0.0),
        top: Val::Px(0.0),
        width: Val::Px(STRIP_WIDTH),
        height: Val::Px(MARKER_SIZE),
        border: UiRect::all(Val::Px(2.0)),
        ..Default::default()
    });
    (inner, marker)
}

/// The luminance strip's gradient for a hue and saturation: black, that colour at
/// the mid luminance, white — which is exactly what `hslToRgb` traces as `L`
/// runs 0 → 1, interpolated in the space it is linear in.
fn luminance_gradient(mid: Color) -> BackgroundGradient {
    BackgroundGradient(vec![Gradient::Linear(LinearGradient {
        color_space: InterpolationColorSpace::Srgba,
        angle: LinearGradient::TO_TOP,
        stops: vec![
            ColorStop::percent(Color::BLACK, 0.0),
            ColorStop::percent(mid, 50.0),
            ColorStop::percent(Color::WHITE, 100.0),
        ],
    })])
}

/// Spawn an aiming marker: a pale box with a dark rim, so it reads against both
/// ends of the field.
fn spawn_marker(commands: &mut Commands, parent: Entity, name: &'static str) -> Entity {
    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(0.0),
                top: Val::Px(0.0),
                width: Val::Px(MARKER_SIZE),
                height: Val::Px(MARKER_SIZE),
                border: UiRect::all(Val::Px(2.0)),
                ..Default::default()
            },
            BorderColor::all(MARKER_OUTLINE),
            BackgroundColor(Color::NONE),
            Outline {
                width: Val::Px(1.0),
                offset: Val::Px(0.0),
                color: MARKER_FILL,
            },
            Pickable::IGNORE,
            Name::new(name),
            ChildOf(parent),
        ))
        .id()
}

/// Spawn a comparison swatch (preview / original). The preview is the drag
/// handle of the reference's drag-to-save gesture, so it is pickable.
fn spawn_compare_swatch(
    commands: &mut Commands,
    parent: Entity,
    name: &'static str,
    is_preview: bool,
) -> Entity {
    let mut swatch = commands.spawn((
        Node {
            width: Val::Px(SWATCH_SIZE),
            height: Val::Px(SWATCH_SIZE),
            border: UiRect::all(Val::Px(1.0)),
            ..Default::default()
        },
        BorderColor::all(CONTROL_BORDER),
        BackgroundColor(Color::BLACK),
        Name::new(name),
        ChildOf(parent),
    ));
    if is_preview {
        swatch.insert((PreviewSwatch, Pickable::default()));
    }
    swatch.id()
}

/// Spawn one channel row: a name label, a slider track + thumb, and a value
/// label. Returns the slider and value-label entities.
fn spawn_channel_row(
    commands: &mut Commands,
    parent: Entity,
    axis: ChannelAxis,
) -> (Entity, Entity) {
    let name = axis.label();
    let channel_row = commands
        .spawn((
            Node {
                align_items: AlignItems::Center,
                ..row(Val::Px(6.0))
            },
            ChildOf(parent),
        ))
        .id();
    commands.spawn((
        Text::new(name),
        UiFont::Sans.at(PICKER_FONT),
        TextColor(TEXT_COLOR),
        Node {
            min_width: Val::Px(14.0),
            ..Default::default()
        },
        ChildOf(channel_row),
    ));
    let slider = commands
        .spawn((
            Slider::default(),
            SliderValue(0.0),
            SliderRange::new(0.0, axis.maximum()),
            SliderStep(1.0),
            axis,
            Node {
                width: Val::Px(TRACK_WIDTH),
                height: Val::Px(TRACK_HEIGHT),
                border: UiRect::all(Val::Px(1.0)),
                ..Default::default()
            },
            BorderColor::all(CONTROL_BORDER),
            BackgroundColor(TRACK_FILL),
            TabIndex(0),
            Name::new(format!("color-picker-slider:{name}")),
            ChildOf(channel_row),
        ))
        .observe(on_color_slider_change)
        .with_child((
            SliderThumb,
            Node {
                position_type: PositionType::Absolute,
                width: Val::Px(THUMB_WIDTH),
                height: Val::Px(TRACK_HEIGHT),
                ..Default::default()
            },
            LogicalInset(LogicalRect {
                inline_start: Val::Px(0.0),
                ..LogicalRect::ZERO
            }),
            BackgroundColor(THUMB_FILL),
        ))
        .id();
    let label = commands
        .spawn((
            Text::new("0"),
            UiFont::Sans.at(PICKER_FONT),
            TextColor(TEXT_COLOR),
            ClassList::new_with_classes([VALUE_CLASS]),
            Node {
                min_width: Val::Px(30.0),
                ..Default::default()
            },
            ChildOf(channel_row),
        ))
        .id();
    (slider, label)
}

/// Spawn the hex row — a `#` prefix and a six-character field, the reference's
/// `hex_hash_prefix` + `hex_value`.
fn spawn_hex_row(commands: &mut Commands, parent: Entity) -> Entity {
    let hex_row = commands
        .spawn((
            Node {
                align_items: AlignItems::Center,
                ..row(Val::Px(6.0))
            },
            ChildOf(parent),
        ))
        .id();
    commands.spawn((
        Text::new("#"),
        UiFont::Mono.at(PICKER_FONT),
        TextColor(TEXT_COLOR),
        Node {
            min_width: Val::Px(14.0),
            ..Default::default()
        },
        ChildOf(hex_row),
    ));
    let field = spawn_text_input(
        commands,
        hex_row,
        &TextInputSpec {
            initial: String::from("000000"),
            tab_index: 0,
            font_size: PICKER_FONT,
            width_glyphs: 7.0,
            max_characters: Some(6),
            ..TextInputSpec::new("color-picker-hex", TextInputKind::Line)
        },
    );
    commands.entity(field).insert(HexField);
    field
}

/// Spawn the eyedropper button.
fn spawn_pipette_button(commands: &mut Commands, parent: Entity) -> Entity {
    let button = commands
        .spawn((
            Button,
            TabIndex(0),
            Node {
                padding: UiRect::axes(Val::Px(8.0), Val::Px(3.0)),
                border: UiRect::all(Val::Px(1.0)),
                ..Default::default()
            },
            BorderColor::all(CONTROL_BORDER),
            BackgroundColor(BUTTON_BACKGROUND),
            PipetteButton,
            Pickable::default(),
            Name::new("color-picker-pipette"),
            ChildOf(parent),
        ))
        .observe(on_pipette_press)
        .id();
    commands.spawn((
        Text::default(),
        Translated::new("color-picker-pipette"),
        UiFont::Sans.at(PICKER_FONT),
        TextColor(TEXT_COLOR),
        ClassList::new_with_classes([VALUE_CLASS]),
        Pickable::IGNORE,
        ChildOf(button),
    ));
    button
}

/// Spawn the Apply-now checkbox, returning its glyph node (which the sync
/// repaints from the setting).
fn spawn_apply_now_toggle(commands: &mut Commands, parent: Entity) -> Entity {
    let toggle = commands
        .spawn((
            Button,
            TabIndex(0),
            Node {
                align_items: AlignItems::Center,
                column_gap: Val::Px(6.0),
                ..row(Val::Px(0.0))
            },
            ApplyNowToggle,
            Pickable::default(),
            Name::new("color-picker-apply-now"),
            ChildOf(parent),
        ))
        .observe(on_apply_now_press)
        .id();
    let glyph = commands
        .spawn((
            Text::new(String::from(CHECKED_GLYPH)),
            UiFont::Sans.at(PICKER_FONT),
            TextColor(HINT_COLOR),
            Pickable::IGNORE,
            ChildOf(toggle),
        ))
        .id();
    commands.spawn((
        Text::default(),
        Translated::new("color-picker-apply-now"),
        UiFont::Sans.at(PICKER_FONT),
        TextColor(TEXT_COLOR),
        ClassList::new_with_classes([VALUE_CLASS]),
        Pickable::IGNORE,
        ChildOf(toggle),
    ));
    glyph
}

/// The checked box glyph.
const CHECKED_GLYPH: &str = "\u{2611}";

/// The unchecked box glyph.
const UNCHECKED_GLYPH: &str = "\u{2610}";

/// Spawn the 32-cell palette under `parent`, two rows of sixteen as the
/// reference draws it.
fn spawn_palette(commands: &mut Commands, parent: Entity) -> [Entity; PALETTE_SIZE] {
    let grid = commands
        .spawn((
            Node {
                ..column(Val::Px(2.0))
            },
            Name::new("color-picker-palette"),
            ChildOf(parent),
        ))
        .id();
    let mut cells = [Entity::PLACEHOLDER; PALETTE_SIZE];
    let mut index = 0_usize;
    while index < PALETTE_SIZE {
        let palette_row = commands
            .spawn((
                Node {
                    ..row(Val::Px(2.0))
                },
                ChildOf(grid),
            ))
            .id();
        let mut column_index = 0_usize;
        while column_index < PALETTE_COLUMNS && index < PALETTE_SIZE {
            let cell = commands
                .spawn((
                    Button,
                    Node {
                        width: Val::Px(PALETTE_CELL_WIDTH),
                        height: Val::Px(PALETTE_CELL_HEIGHT),
                        border: UiRect::all(Val::Px(1.0)),
                        ..Default::default()
                    },
                    BorderColor::all(CONTROL_BORDER),
                    BackgroundColor(Color::BLACK),
                    PaletteCell(index),
                    Pickable::default(),
                    Name::new(format!("color-picker-palette-cell:{index}")),
                    ChildOf(palette_row),
                ))
                .observe(on_palette_press)
                .observe(on_palette_drag_enter)
                .observe(on_palette_drag_leave)
                .observe(on_palette_drop)
                .id();
            if let Some(slot) = cells.get_mut(index) {
                *slot = cell;
            }
            index = index.saturating_add(1);
            column_index = column_index.saturating_add(1);
        }
    }
    cells
}

/// Spawn an OK / Cancel button.
fn spawn_picker_button(
    commands: &mut Commands,
    parent: Entity,
    which: PickerButton,
    label_key: &'static str,
) {
    let button = commands
        .spawn((
            Button,
            TabIndex(0),
            Node {
                padding: UiRect::axes(Val::Px(12.0), Val::Px(3.0)),
                border: UiRect::all(Val::Px(1.0)),
                ..Default::default()
            },
            BorderColor::all(CONTROL_BORDER),
            BackgroundColor(BUTTON_BACKGROUND),
            which,
            Pickable::default(),
            Name::new(format!("color-picker-button:{label_key}")),
            ChildOf(parent),
        ))
        .observe(on_picker_button)
        .id();
    commands.spawn((
        Text::default(),
        Translated::new(label_key),
        UiFont::Sans.at(PICKER_FONT),
        TextColor(TEXT_COLOR),
        ClassList::new_with_classes([VALUE_CLASS]),
        Pickable::IGNORE,
        ChildOf(button),
    ));
}

// ---------------------------------------------------------------------------
// Aiming: the field and the strip.

/// What a pointer handler needs to turn a screen position into a fraction of a
/// control.
type AimQuery<'world, 'state> = Query<
    'world,
    'state,
    (
        &'static ComputedNode,
        &'static UiGlobalTransform,
        &'static bevy::ui::ComputedUiRenderTargetInfo,
    ),
>;

/// Where a pointer sits inside `entity`'s **picture**, as a fraction of it in
/// each axis, clamped to it — so a drag that wanders off the control keeps
/// aiming at its nearest edge, as the reference's `llclamp`ed hover does.
///
/// The outer box is a marker larger than the picture in the axes the marker
/// travels, and the picture is centred in it, so the fraction is measured
/// against the picture rather than the box the pointer actually hit.
fn pointer_fraction(
    entity: Entity,
    position: Vec2,
    aims: &AimQuery,
    ui_scale: &UiScale,
    picture: Vec2,
    outer: Vec2,
) -> Option<Vec2> {
    let Ok((node, transform, target)) = aims.get(entity) else {
        return None;
    };
    // Component-wise `f32` throughout: the workspace's arithmetic lint fires on
    // `glam`'s overloaded operators, and every scalar here is finite by
    // construction.
    let physical = target.scale_factor() / ui_scale.0;
    let normalized = node.normalize_point(
        *transform,
        Vec2::new(position.x * physical, position.y * physical),
    )?;
    let axis = |normal: f32, picture: f32, outer: f32| {
        if picture <= 0.0 {
            return 0.0;
        }
        // `normalize_point` spans the whole outer box, centred; re-express that
        // as a fraction of the centred picture inside it.
        ((normal * outer / picture) + 0.5).clamp(0.0, 1.0)
    };
    Some(Vec2::new(
        axis(normalized.x, picture.x, outer.x),
        axis(normalized.y, picture.y, outer.y),
    ))
}

/// A control's picture size and its outer box: the field's picture is inset by
/// half a marker on every side, the strip's only top and bottom (its marker
/// spans the full width, so it needs no horizontal room).
fn control_boxes(is_field: bool) -> (Vec2, Vec2) {
    let outer_height = FIELD_SIZE + MARKER_SIZE;
    if is_field {
        (
            Vec2::new(FIELD_SIZE, FIELD_SIZE),
            Vec2::new(FIELD_SIZE + MARKER_SIZE, outer_height),
        )
    } else {
        (
            Vec2::new(STRIP_WIDTH, FIELD_SIZE),
            Vec2::new(STRIP_WIDTH, outer_height),
        )
    }
}

/// The picker a control belongs to: the nearest ancestor of `entity` (or
/// `entity` itself) carrying a [`ColorPickerState`].
///
/// **Not** `host_floater`. A control's picker and the *window* it happens to sit
/// in are the same entity only because the live picker puts its state on the
/// floater root; a picker built into something that is not a floater window — the
/// gallery's specimen, where the body hangs inside a window whose root knows
/// nothing about colours — would find that window and then find no state on it,
/// which is a picker that draws perfectly and answers no click at all. Asking
/// for the state itself cannot miss that way.
///
/// Generic over the query so the immutable readers and the mutable writers can
/// each hand in their own: two queries over one component in one system is the
/// `B0001` conflict, not a convenience.
fn picker_window<D, F>(
    entity: Entity,
    parents: &Query<&ChildOf>,
    windows: &Query<'_, '_, D, F>,
) -> Option<Entity>
where
    D: bevy::ecs::query::QueryData,
    F: bevy::ecs::query::QueryFilter,
{
    let mut at = entity;
    loop {
        if windows.contains(at) {
            return Some(at);
        }
        at = parents.get(at).ok()?.parent();
    }
}

/// Aim the field or the strip a pointer is over, writing the new colour into the
/// picker's state.
fn aim_at_pointer(
    entity: Entity,
    position: Vec2,
    aims: &AimQuery,
    ui_scale: &UiScale,
    kinds: &Query<(Has<HueSaturationField>, Has<LuminanceStrip>)>,
    parents: &Query<&ChildOf>,
    windows: &mut Query<&mut ColorPickerState>,
) {
    let Ok((is_field, _strip)) = kinds.get(entity) else {
        return;
    };
    let (picture, outer) = control_boxes(is_field);
    let Some(fraction) = pointer_fraction(entity, position, aims, ui_scale, picture, outer) else {
        return;
    };
    let Some(window) = picker_window(entity, parents, windows) else {
        return;
    };
    let Ok(mut state) = windows.get_mut(window) else {
        return;
    };
    let mut hsl = state.hsl;
    if is_field {
        if let Some(hue) = hsl.get_mut(0) {
            *hue = fraction.x;
        }
        if let Some(saturation) = hsl.get_mut(1) {
            // The picture's top is full saturation; the reference's bottom-up
            // image says the same thing the other way up.
            *saturation = 1.0 - fraction.y;
        }
    } else if let Some(luminance) = hsl.get_mut(2) {
        *luminance = 1.0 - fraction.y;
    }
    state.set_hsl(hsl);
}

/// A press on the field or the strip starts a gesture and aims at once — the
/// reference's click-to-set.
fn on_aim_press(
    mut press: On<Pointer<Press>>,
    aims: AimQuery,
    kinds: Query<(Has<HueSaturationField>, Has<LuminanceStrip>)>,
    mut drags: Query<&mut PickerDrag>,
    parents: Query<&ChildOf>,
    mut windows: Query<&mut ColorPickerState>,
    ui_scale: Res<UiScale>,
) {
    if press.button != PointerButton::Primary {
        return;
    }
    let entity = press.entity;
    if !drags.contains(entity) {
        return;
    }
    press.propagate(false);
    if let Ok(mut drag) = drags.get_mut(entity) {
        drag.0 = true;
    }
    aim_at_pointer(
        entity,
        press.pointer_location.position,
        &aims,
        &ui_scale,
        &kinds,
        &parents,
        &mut windows,
    );
}

/// A drag re-aims at wherever the pointer now is, even off the control.
fn on_aim_drag(
    mut drag: On<Pointer<Drag>>,
    aims: AimQuery,
    kinds: Query<(Has<HueSaturationField>, Has<LuminanceStrip>)>,
    drags: Query<&PickerDrag>,
    parents: Query<&ChildOf>,
    mut windows: Query<&mut ColorPickerState>,
    ui_scale: Res<UiScale>,
) {
    if drag.button != PointerButton::Primary {
        return;
    }
    let entity = drag.entity;
    if !drags.get(entity).is_ok_and(|state| state.0) {
        return;
    }
    drag.propagate(false);
    aim_at_pointer(
        entity,
        drag.pointer_location.position,
        &aims,
        &ui_scale,
        &kinds,
        &parents,
        &mut windows,
    );
}

/// The end of a drag: one last aim, and the latch drops.
fn on_aim_drag_end(
    mut drag_end: On<Pointer<DragEnd>>,
    aims: AimQuery,
    kinds: Query<(Has<HueSaturationField>, Has<LuminanceStrip>)>,
    mut drags: Query<&mut PickerDrag>,
    parents: Query<&ChildOf>,
    mut windows: Query<&mut ColorPickerState>,
    ui_scale: Res<UiScale>,
) {
    if drag_end.button != PointerButton::Primary {
        return;
    }
    let entity = drag_end.entity;
    let Ok(mut drag) = drags.get_mut(entity) else {
        return;
    };
    if !drag.0 {
        return;
    }
    drag_end.propagate(false);
    drag.0 = false;
    aim_at_pointer(
        entity,
        drag_end.pointer_location.position,
        &aims,
        &ui_scale,
        &kinds,
        &parents,
        &mut windows,
    );
}

/// A release that ends a press which never became a drag — a plain click, whose
/// aim the press already set.
fn on_aim_release(release: On<Pointer<Release>>, mut drags: Query<&mut PickerDrag>) {
    if release.button != PointerButton::Primary {
        return;
    }
    if let Ok(mut drag) = drags.get_mut(release.entity) {
        drag.0 = false;
    }
}

/// A cancelled pointer drops the gesture where it is.
fn on_aim_cancel(cancel: On<Pointer<Cancel>>, mut drags: Query<&mut PickerDrag>) {
    if let Ok(mut drag) = drags.get_mut(cancel.entity) {
        drag.0 = false;
    }
}

// ---------------------------------------------------------------------------
// Sliders.

/// A slider drag: write the value back and drive the picker's state from it.
fn on_color_slider_change(
    change: On<ValueChange<f32>>,
    axes: Query<&ChannelAxis>,
    ranges: Query<&SliderRange>,
    mut windows: Query<&mut ColorPickerState>,
    parents: Query<&ChildOf>,
    mut commands: Commands,
) {
    let slider = change.source;
    let clamped = ranges
        .get(slider)
        .map_or(change.value, |range| range.clamp(change.value));
    commands.entity(slider).insert(SliderValue(clamped));
    // The slider's own picker, not "the" picker: two swatches being answered at
    // once are two windows with six sliders each.
    let Some(window) = picker_window(slider, &parents, &windows) else {
        return;
    };
    let Ok(mut state) = windows.get_mut(window) else {
        return;
    };
    if let Ok(axis) = axes.get(slider) {
        axis.write(&mut state, clamped);
    }
}

// ---------------------------------------------------------------------------
// The palette.

/// A click on a palette cell loads that colour — the reference's palette
/// `handleMouseDown`.
fn on_palette_press(
    press: On<Pointer<Press>>,
    cells: Query<&PaletteCell>,
    palette: Res<ColorPalette>,
    parents: Query<&ChildOf>,
    mut windows: Query<&mut ColorPickerState>,
) {
    if press.button != PointerButton::Primary {
        return;
    }
    let Ok(cell) = cells.get(press.entity) else {
        return;
    };
    let Some(window) = picker_window(press.entity, &parents, &windows) else {
        return;
    };
    let Ok(mut state) = windows.get_mut(window) else {
        return;
    };
    state.set_color(palette.entry(cell.0));
}

/// The current colour dragged over a cell: rim it, so the drop target is
/// obvious — the reference's highlighted entry.
fn on_palette_drag_enter(
    enter: On<Pointer<DragEnter>>,
    previews: Query<(), With<PreviewSwatch>>,
    mut borders: Query<&mut BorderColor, With<PaletteCell>>,
) {
    if !previews.contains(enter.dragged) {
        return;
    }
    if let Ok(mut border) = borders.get_mut(enter.entity) {
        *border = BorderColor::all(DROP_BORDER);
    }
}

/// The drag leaves a cell: put its rim back.
fn on_palette_drag_leave(
    leave: On<Pointer<DragLeave>>,
    mut borders: Query<&mut BorderColor, With<PaletteCell>>,
) {
    if let Ok(mut border) = borders.get_mut(leave.entity) {
        *border = BorderColor::all(CONTROL_BORDER);
    }
}

/// The saved palette and what a save writes through, bundled as one
/// [`SystemParam`](bevy::ecs::system::SystemParam): the in-memory row, the
/// settings store it is persisted to, and the cell rims a drop repaints.
#[derive(bevy::ecs::system::SystemParam)]
struct PaletteStore<'w, 's> {
    /// The palette row shown across every picker.
    palette: ResMut<'w, ColorPalette>,
    /// The settings store a saved cell is written through (absent headless).
    settings: Option<ResMut<'w, ViewerSettings>>,
    /// The cell rims, which a drag-over highlights and a drop restores.
    borders: Query<'w, 's, &'static mut BorderColor, With<PaletteCell>>,
}

/// Dropping the current colour on a cell **saves** it there, and persists it —
/// the reference's drag-from-the-swatch gesture, and its `LLUIColorTable`
/// write-back.
fn on_palette_drop(
    drop: On<Pointer<DragDrop>>,
    previews: Query<(), With<PreviewSwatch>>,
    cells: Query<&PaletteCell>,
    parents: Query<&ChildOf>,
    windows: Query<&ColorPickerState>,
    mut store: PaletteStore,
) {
    if let Ok(mut border) = store.borders.get_mut(drop.entity) {
        *border = BorderColor::all(CONTROL_BORDER);
    }
    if !previews.contains(drop.dropped) {
        return;
    }
    let Ok(cell) = cells.get(drop.entity) else {
        return;
    };
    // The *dragged* swatch's picker, not the cell's: a colour dragged out of one
    // picker onto another's palette is still that colour, and the state to read
    // is the one it was dragged from.
    let Some(window) = picker_window(drop.dropped, &parents, &windows) else {
        return;
    };
    let Ok(state) = windows.get(window) else {
        return;
    };
    let color = state.current();
    if let Some(slot) = store.palette.0.get_mut(cell.0) {
        *slot = color;
    }
    if let Some(settings) = store.settings.as_mut() {
        settings.set(
            Scope::Global,
            &palette_setting_name(cell.0),
            SettingValue::Color4(color.to_srgba().to_f32_array()),
        );
    }
}

// ---------------------------------------------------------------------------
// The hex field.

/// A colour as the six hex digits the field holds.
fn hex_of(color: Color) -> String {
    let srgba = color.to_srgba();
    format!(
        "{:02X}{:02X}{:02X}",
        byte(srgba.red * CHANNEL_MAX),
        byte(srgba.green * CHANNEL_MAX),
        byte(srgba.blue * CHANNEL_MAX)
    )
}

/// The colour six hex digits name, or `None` when the text is not six hex
/// digits — an incomplete field is not a rejection, just not a colour yet.
fn color_of_hex(text: &str) -> Option<Color> {
    let trimmed = text.trim().trim_start_matches('#');
    if trimmed.len() != 6 || !trimmed.chars().all(|digit| digit.is_ascii_hexdigit()) {
        return None;
    }
    let channel = |from: usize| {
        trimmed
            .get(from..from.saturating_add(2))
            .and_then(|pair| u8::from_str_radix(pair, 16).ok())
    };
    Some(Color::srgb_u8(channel(0)?, channel(2)?, channel(4)?))
}

/// Which hex field had the keyboard last, so its focus loss can commit it — the
/// debug-settings editor's `DebugFieldFocus`, for one field per window.
#[derive(Resource, Debug, Default)]
struct HexFieldFocus(Option<Entity>);

/// Commit a hex field on `Enter` or on focus loss, the way every other committed
/// field in the viewer does. Text that is not six hex digits is simply not a
/// colour; the sync puts the current one back.
fn commit_hex_field(
    focus: Option<Res<InputFocus>>,
    keyboard: Res<ButtonInput<KeyCode>>,
    mut tracked: ResMut<HexFieldFocus>,
    fields: Query<&EditableText, With<HexField>>,
    parents: Query<&ChildOf>,
    mut windows: Query<&mut ColorPickerState>,
) {
    let focused = focus
        .as_ref()
        .and_then(|focus| focus.get())
        .filter(|entity| fields.contains(*entity));
    let enter =
        keyboard.just_pressed(KeyCode::Enter) || keyboard.just_pressed(KeyCode::NumpadEnter);
    let commit = if enter {
        focused
    } else if tracked.0 != focused {
        tracked.0.filter(|entity| fields.contains(*entity))
    } else {
        None
    };
    tracked.0 = focused;
    let Some(field) = commit else {
        return;
    };
    let Ok(text) = fields.get(field) else {
        return;
    };
    let Some(color) = color_of_hex(&text.value().to_string()) else {
        return;
    };
    let Some(window) = picker_window(field, &parents, &windows) else {
        return;
    };
    if let Ok(mut state) = windows.get_mut(window) {
        state.set_color(color);
    }
}

// ---------------------------------------------------------------------------
// The eyedropper.

/// Arm or disarm the eyedropper for the pressed button's window.
fn on_pipette_press(
    press: On<Pointer<Press>>,
    buttons: Query<(), With<PipetteButton>>,
    parents: Query<&ChildOf>,
    windows: Query<(&ColorPickerState, Has<Eyedropper>)>,
    mut commands: Commands,
) {
    if press.button != PointerButton::Primary || !buttons.contains(press.entity) {
        return;
    }
    let Some(window) = picker_window(press.entity, &parents, &windows) else {
        return;
    };
    let Ok((state, armed)) = windows.get(window) else {
        return;
    };
    if armed {
        commands.entity(window).remove::<Eyedropper>();
        return;
    }
    commands.entity(window).insert(Eyedropper {
        frame: None,
        before: state.current(),
        shade: None,
    });
    // One read-back, taken now: the screen does not change underneath a pointer
    // that is only hunting for a pixel, and a capture per frame would copy the
    // whole window every frame of the gesture.
    commands.spawn(Screenshot::primary_window()).observe(
        move |captured: On<ScreenshotCaptured>,
              mut droppers: Query<&mut Eyedropper>,
              mut commands: Commands| {
            if let Ok(mut dropper) = droppers.get_mut(window) {
                dropper.frame = Some(captured.image.clone());
            }
            commands.entity(captured.entity).despawn();
        },
    );
}

/// The colour of the captured frame under a logical screen position, or `None`
/// when the position is off the frame.
fn sample_frame(frame: &Image, position: Vec2, scale_factor: f32) -> Option<Color> {
    let size = frame.texture_descriptor.size;
    let physical = Vec2::new(position.x * scale_factor, position.y * scale_factor);
    if physical.x < 0.0 || physical.y < 0.0 {
        return None;
    }
    #[expect(
        clippy::as_conversions,
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "both coordinates are non-negative and finite here, and the result is bounded \
                  against the frame's own extent immediately below"
    )]
    let (x, y) = (physical.x.floor() as u32, physical.y.floor() as u32);
    if x >= size.width || y >= size.height {
        return None;
    }
    frame.get_color_at(x, y).ok()
}

/// Drive an armed eyedropper: put the click-catcher up once the frame lands,
/// track the pointer through it, and finish on a click (keeping the sample) or
/// on `Escape` (restoring what the picker held).
fn drive_eyedropper(
    mut droppers: Query<(Entity, &mut Eyedropper)>,
    mut windows: Query<&mut ColorPickerState>,
    root: Option<Res<UiRoot>>,
    screens: Query<&Window, With<PrimaryWindow>>,
    keyboard: Res<ButtonInput<KeyCode>>,
    mut commands: Commands,
) {
    let abandon = keyboard.just_pressed(KeyCode::Escape);
    for (window, mut dropper) in &mut droppers {
        if abandon {
            if let Ok(mut state) = windows.get_mut(window) {
                state.set_color(dropper.before);
            }
            disarm(window, &dropper, &mut commands);
            continue;
        }
        if dropper.frame.is_none() {
            continue;
        }
        if dropper.shade.is_none()
            && let Some(root) = root.as_ref()
        {
            // A screen-wide catcher: it absorbs the click that finishes the
            // gesture, so hunting for a pixel over a button does not press it.
            // It is spawned *after* the capture, so it is never in the frame
            // being sampled.
            dropper.shade = Some(
                commands
                    .spawn((
                        Node {
                            position_type: PositionType::Absolute,
                            left: Val::Px(0.0),
                            top: Val::Px(0.0),
                            width: Val::Percent(100.0),
                            height: Val::Percent(100.0),
                            ..Default::default()
                        },
                        BackgroundColor(Color::NONE),
                        GlobalZIndex(i32::MAX),
                        PipetteShade(window),
                        Pickable::default(),
                        Name::new("color-picker-pipette-shade"),
                        ChildOf(root.0),
                    ))
                    .observe(on_pipette_shade_press)
                    .id(),
            );
        }
        let Some(frame) = dropper.frame.as_ref() else {
            continue;
        };
        let Some(screen) = screens.iter().next() else {
            continue;
        };
        let Some(position) = screen.cursor_position() else {
            continue;
        };
        let Some(color) = sample_frame(frame, position, screen.scale_factor()) else {
            continue;
        };
        if let Ok(mut state) = windows.get_mut(window) {
            state.set_color(color);
        }
    }
}

/// Take an armed eyedropper down: the shade goes with it.
fn disarm(window: Entity, dropper: &Eyedropper, commands: &mut Commands) {
    if let Some(shade) = dropper.shade {
        commands.entity(shade).despawn();
    }
    commands.entity(window).remove::<Eyedropper>();
}

/// A click anywhere while the eyedropper is armed **takes** the colour under it
/// and ends the gesture.
fn on_pipette_shade_press(
    mut press: On<Pointer<Press>>,
    shades: Query<&PipetteShade>,
    droppers: Query<&Eyedropper>,
    mut commands: Commands,
) {
    let Ok(shade) = shades.get(press.entity) else {
        return;
    };
    press.propagate(false);
    if let Ok(dropper) = droppers.get(shade.0) {
        disarm(shade.0, dropper, &mut commands);
    }
}

// ---------------------------------------------------------------------------
// Opening, replying, and the visual sync.

/// Open (or re-aim) the picker window for the swatch that asked, seeding its
/// state.
///
/// Keyed by the **opening window and the swatch's field together**, so several
/// requests in one frame are several windows. The shared window this replaces
/// could only honour one of them, and said the others out loud rather than
/// answering them.
fn handle_open_color_picker(
    mut opens: MessageReader<OpenColorPicker>,
    mut floaters: KeyedFloaters,
    mut windows: Query<&mut ColorPickerState>,
    parents: Query<&ChildOf>,
    openers: Query<(Entity, &Floater)>,
    mut commands: Commands,
) {
    let requests: Vec<OpenColorPicker> = opens.read().cloned().collect();
    for open in requests {
        let (owner, key) = picker_identity(open.requester, &open.field, &parents, &openers);
        let opened = floaters.open(color_picker_floater_spec(), key);
        let window = opened.root();
        let state = ColorPickerState::opened_on(open.requester, open.current);
        match opened {
            KeyedFloaterOpen::Spawned(handle) => {
                let ui = build_color_picker_content(&handle, &mut commands);
                // Seeded here rather than after the insert: the components only
                // reach the world when this frame's commands flush, so a window
                // spawned now is not queryable yet.
                commands.entity(handle.root).insert((state, ui));
                if let Some(owner) = owner {
                    commands.entity(handle.root).insert(FloaterOwner(owner));
                }
            }
            KeyedFloaterOpen::Existing(_root) => {
                if let Ok(mut existing) = windows.get_mut(window) {
                    *existing = state;
                }
            }
        }
    }
}

/// Hand every open picker's colour to its requester as it changes — the live
/// half of [`ColorPicked`], and the reference's `ApplyColorImmediately`.
///
/// Centralised here rather than in each of the seven controls that can change a
/// colour: the field, the strip, six sliders, the hex field, a palette cell and
/// the eyedropper all move the same state, and each one remembering to speak
/// was how the widget grew a control that silently did not.
fn emit_live_preview(
    mut windows: Query<&mut ColorPickerState, Changed<ColorPickerState>>,
    apply_now: Res<ApplyColorImmediately>,
    mut picked: MessageWriter<ColorPicked>,
) {
    if !apply_now.0 {
        return;
    }
    for mut state in &mut windows {
        let Some(requester) = state.requester else {
            continue;
        };
        let current = state.current();
        if current == state.previewed {
            continue;
        }
        state.previewed = current;
        picked.write(ColorPicked {
            requester,
            color: current,
            final_pick: false,
        });
    }
}

/// Where a thumb's leading edge sits along its track for `value`: the value's
/// fraction of `range`, over the track less the thumb's own width, so the thumb
/// spans the track exactly at the ends rather than hanging off them.
fn thumb_offset(value: f32, range: &SliderRange) -> f32 {
    let span = range.span();
    let fraction = if span > f32::EPSILON {
        ((value - range.start()) / span).clamp(0.0, 1.0)
    } else {
        0.0
    };
    fraction * (TRACK_WIDTH - THUMB_WIDTH)
}

/// Every kind of node one picker's visual sync writes, bundled as one
/// [`SystemParam`](bevy::ecs::system::SystemParam): the fills, the gradients,
/// the boxes, the thumb insets and the two kinds of text.
#[derive(bevy::ecs::system::SystemParam)]
struct PickerVisuals<'w, 's> {
    /// Flat fills — swatches, the preview, the latched check marks.
    backgrounds: Query<'w, 's, &'static mut BackgroundColor>,
    /// The channel ramps.
    gradients: Query<'w, 's, &'static mut BackgroundGradient>,
    /// Layout, for the saturation / value square's marker.
    nodes: Query<'w, 's, &'static mut Node>,
    /// The slider thumbs' logical insets.
    insets: Query<'w, 's, &'static mut LogicalInset, With<SliderThumb>>,
    /// Plain texts — the channel read-outs.
    texts: Query<'w, 's, &'static mut Text>,
    /// The editable hex field.
    editables: Query<'w, 's, &'static mut EditableText>,
    /// What toggles the eyedropper / latched markers.
    commands: Commands<'w, 's>,
}

/// Reconcile every open picker's visuals from its live state: the compare
/// swatches, the field and strip markers, the strip's own gradient, both slider
/// sets, the hex text, the palette fills, and the two latched controls.
///
/// Every write is guarded by a compare, as every other widget in this crate
/// does. `LogicalInset` is exactly what `ChangedLogicalBoxes` filters on so that
/// an unchanged UI does not re-resolve its boxes every frame, and an unguarded
/// thumb write put all three through that resolver on every frame of the
/// process, picker open or closed. (`resolve_logical_boxes` compares again
/// before it touches `Node`, so taffy was never re-entered — the waste was the
/// resolver's own pass, small but permanent.) A closed picker is not on screen,
/// so it does no work at all.
fn sync_color_picker_visual(
    windows: Query<(&ColorPickerState, &ColorPickerUi, Has<Eyedropper>)>,
    sliders: Query<(&SliderValue, &SliderRange, &ChannelAxis, &Children)>,
    palette: Res<ColorPalette>,
    apply_now: Res<ApplyColorImmediately>,
    focus: Option<Res<InputFocus>>,
    mut visuals: PickerVisuals,
) {
    let focused = focus.as_ref().and_then(|focus| focus.get());
    for (state, ui, armed) in &windows {
        if state.requester.is_none() {
            continue;
        }
        let current = state.current();
        paint(&mut visuals.backgrounds, ui.preview, current);
        paint(&mut visuals.backgrounds, ui.original, state.original);
        paint(
            &mut visuals.backgrounds,
            ui.pipette,
            if armed {
                BUTTON_LATCHED
            } else {
                BUTTON_BACKGROUND
            },
        );

        // The strip is drawn around the current hue and saturation.
        let wanted = luminance_gradient(state.mid_luminance_color());
        if let Ok(mut gradient) = visuals.gradients.get_mut(ui.strip)
            && *gradient != wanted
        {
            *gradient = wanted;
        }

        // The two markers, in their outer boxes' own coordinates.
        let hue = state.hsl.first().copied().unwrap_or(0.0);
        let saturation = state.hsl.get(1).copied().unwrap_or(0.0);
        let luminance = state.hsl.get(2).copied().unwrap_or(0.0);
        place(
            &mut visuals.nodes,
            ui.field_marker,
            Vec2::new(hue * FIELD_SIZE, (1.0 - saturation) * FIELD_SIZE),
        );
        place(
            &mut visuals.nodes,
            ui.strip_marker,
            Vec2::new(0.0, (1.0 - luminance) * FIELD_SIZE),
        );

        for (index, slider) in ui.sliders.iter().enumerate() {
            let Ok((value, range, axis, children)) = sliders.get(*slider) else {
                continue;
            };
            let wanted = axis.read(state);
            // `SliderValue` is an immutable component: it is replaced, not
            // written through, and only when it would change.
            if !slider_holds(value.0, wanted) {
                visuals.commands.entity(*slider).insert(SliderValue(wanted));
            }
            let offset = Val::Px(thumb_offset(wanted, range));
            for child in children.iter() {
                if let Ok(mut inset) = visuals.insets.get_mut(child)
                    && inset.0.inline_start != offset
                {
                    inset.0.inline_start = offset;
                }
            }
            if let Some(label) = ui.labels.get(index)
                && let Ok(mut text) = visuals.texts.get_mut(*label)
            {
                let want = format!("{}", wanted.round());
                if text.0 != want {
                    text.0 = want;
                }
            }
        }

        seed_hex(&mut visuals.editables, focused, ui.hex, &hex_of(current));

        for (index, cell) in ui.palette.iter().enumerate() {
            paint(&mut visuals.backgrounds, *cell, palette.entry(index));
        }

        if let Ok(mut glyph) = visuals.texts.get_mut(ui.apply_glyph) {
            let want = if apply_now.0 {
                CHECKED_GLYPH
            } else {
                UNCHECKED_GLYPH
            };
            if glyph.0 != want {
                glyph.0 = String::from(want);
            }
        }
    }
}

/// Whether a slider already holds the value the sync would write.
///
/// Exactly equal, not within a margin: the number the sync computes is the same
/// computation that produced the one the slider holds, so an unchanged state
/// reproduces it bit for bit, and anything else genuinely is a change. A margin
/// here would be the bug — it would leave a slider a nudge behind its state.
#[expect(
    clippy::float_cmp,
    reason = "both sides are the same computation over the same state, so equality is exact \
              whenever nothing changed; see the note above"
)]
fn slider_holds(value: f32, wanted: f32) -> bool {
    value == wanted
}

/// Paint a node's fill, only if it would change.
fn paint(backgrounds: &mut Query<&mut BackgroundColor>, entity: Entity, color: Color) {
    if let Ok(mut background) = backgrounds.get_mut(entity)
        && background.0 != color
    {
        background.0 = color;
    }
}

/// Move a marker's box, only if it would change.
fn place(nodes: &mut Query<&mut Node>, entity: Entity, at: Vec2) {
    let (left, top) = (Val::Px(at.x), Val::Px(at.y));
    if let Ok(mut node) = nodes.get_mut(entity)
        && (node.left != left || node.top != top)
    {
        node.left = left;
        node.top = top;
    }
}

/// Put the current colour into the hex field, unless the user is in it — the
/// debug editor's `seed_field`, for the one field here.
#[expect(
    clippy::cmp_owned,
    reason = "the editor's SplitString has no borrow-free comparison against &str; the guard \
              keeps the pass write-free when nothing changed"
)]
fn seed_hex(
    editables: &mut Query<&mut EditableText>,
    focused: Option<Entity>,
    entity: Entity,
    want: &str,
) {
    let Ok(mut editable) = editables.get_mut(entity) else {
        return;
    };
    if focused == Some(entity) || editable.is_composing() {
        return;
    }
    if editable.value().to_string() != want {
        editable.editor_mut().set_text(want);
    }
}

/// Paint every colour swatch's fill from its [`ColorSwatchValue`] whenever it
/// changes (a consumer writing the value drives the visual through here).
fn apply_color_swatch_fill(
    mut swatches: Query<(&ColorSwatchValue, &mut BackgroundColor), Changed<ColorSwatchValue>>,
) {
    for (value, mut background) in &mut swatches {
        if background.0 != value.0 {
            background.0 = value.0;
        }
    }
}

/// The Apply-now checkbox: flip the setting the live stream is gated on.
fn on_apply_now_press(
    press: On<Pointer<Press>>,
    toggles: Query<(), With<ApplyNowToggle>>,
    mut apply_now: ResMut<ApplyColorImmediately>,
    settings: Option<ResMut<ViewerSettings>>,
) {
    if press.button != PointerButton::Primary || !toggles.contains(press.entity) {
        return;
    }
    apply_now.0 = !apply_now.0;
    // The store only remembers it. A host without one (the gallery, a test fold)
    // still flips the flag, which is what makes the control a control.
    if let Some(mut settings) = settings {
        settings.set(
            Scope::Global,
            SETTING_APPLY_IMMEDIATELY,
            SettingValue::Bool(apply_now.0),
        );
    }
}

/// OK / Cancel: OK emits [`ColorPicked`] for the requester; both close the
/// floater.
fn on_picker_button(
    press: On<Pointer<Press>>,
    buttons: Query<&PickerButton>,
    mut windows: Query<&mut ColorPickerState>,
    parents: Query<&ChildOf>,
    floaters: Query<(Entity, &Floater)>,
    mut picked: MessageWriter<ColorPicked>,
    mut chrome: MessageWriter<FloaterCommand>,
) {
    if press.button != PointerButton::Primary {
        return;
    }
    let Ok(which) = buttons.get(press.entity) else {
        return;
    };
    // Two different questions, and deliberately two lookups: the state to
    // answer with is the *picker's* (which need not be a window at all — the
    // gallery's specimen is one), and the thing to close is the window it sits
    // in, if it sits in one.
    let Some(window) = picker_window(press.entity, &parents, &windows) else {
        return;
    };
    let closing = host_floater(press.entity, &parents, &floaters);
    let Ok(mut state) = windows.get_mut(window) else {
        return;
    };
    if let Some(requester) = state.requester {
        let reply = match which {
            // Commit the chosen colour.
            PickerButton::Ok => ColorPicked {
                requester,
                color: state.current(),
                final_pick: true,
            },
            // Revert the live preview to the colour the picker opened on.
            PickerButton::Cancel => ColorPicked {
                requester,
                color: state.original,
                final_pick: false,
            },
        };
        picked.write(reply);
    }
    state.requester = None;
    if let Some(closing) = closing {
        chrome.write(FloaterCommand {
            floater: closing,
            op: FloaterOp::Close,
        });
    }
}

/// Round a 0..255 channel value to a byte.
const fn byte(value: f32) -> u8 {
    let clamped = value.clamp(0.0, CHANNEL_MAX).round();
    #[expect(
        clippy::as_conversions,
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "clamped to 0..=255 and rounded, so the f32 → u8 narrowing is exact"
    )]
    let byte = clamped as u8;
    byte
}

// ---------------------------------------------------------------------------
// The gallery specimen.

/// The picker's content, built into a gallery card exactly as the live window
/// builds it — so the sweep measures the real field, strip, sliders, palette and
/// reply row rather than a line of prose about them.
pub fn spawn_color_picker_specimen(
    commands: &mut Commands,
    parent: Entity,
    _cx: sl_viewer_ui_core::ui_element::ElementCx,
) -> Entity {
    let (root, ui) = build_color_picker_body(commands, parent);
    // The specimen answers a placeholder rather than nobody: a closed picker is
    // one the sync skips, and a picker drawn on black would put every marker in
    // a corner and tell the sweep nothing about where they travel.
    let state = ColorPickerState::opened_on(Entity::PLACEHOLDER, Color::srgb(0.2, 0.55, 0.8));
    commands.entity(root).insert((state, ui));
    root
}

#[cfg(test)]
mod tests {
    use bevy::camera::NormalizedRenderTarget;
    use bevy::picking::backend::HitData;
    use bevy::picking::pointer::{Location, PointerId};
    use bevy::prelude::*;
    use bevy::ui_widgets::{SliderRange, SliderThumb, ValueChange};
    use pretty_assertions::assert_eq;

    use super::{
        ApplyColorImmediately, ApplyNowToggle, CHANNEL_MAX, ChannelAxis, ColorPicked,
        ColorPickerPlugin, ColorPickerState, ColorPickerUi, ColorSwatchValue, OpenColorPicker,
        PickerButton, byte, color_of_hex, hex_of, hsl_to_srgb, spawn_color_swatch, srgb_to_hsl,
        thumb_offset,
    };
    use crate::floater::FloaterPlugin;
    use sl_viewer_ui_core::ui::{LogicalInset, UiDirection, UiRoot};

    /// A boxed error so tests can use `?` instead of the disallowed
    /// `unwrap` / `expect`.
    type TestError = Box<dyn core::error::Error>;

    /// What the picker emitted, and how much churn the visual sync caused —
    /// copied out in `PostUpdate` so a frame's writes are seen after the systems
    /// that made them.
    #[derive(Resource, Debug, Default)]
    struct Recorded {
        /// Every [`ColorPicked`] the picker has emitted.
        picked: Vec<ColorPicked>,
        /// How many thumb insets were re-marked, summed over frames.
        inset_writes: usize,
        /// How many backgrounds were re-marked, summed over frames.
        background_writes: usize,
    }

    /// Copy this frame's replies and count the components the sync touched.
    fn record(
        mut picked: MessageReader<ColorPicked>,
        insets: Query<(), (With<SliderThumb>, Changed<LogicalInset>)>,
        backgrounds: Query<(), Changed<BackgroundColor>>,
        mut recorded: ResMut<Recorded>,
    ) {
        let replies: Vec<ColorPicked> = picked.read().copied().collect();
        recorded.picked.extend(replies);
        recorded.inset_writes = recorded.inset_writes.saturating_add(insets.iter().count());
        recorded.background_writes = recorded
            .background_writes
            .saturating_add(backgrounds.iter().count());
    }

    /// A headless app carrying the picker plugin, the floater manager it opens
    /// its windows through, a `UiRoot` for them to hang from, and the recorder —
    /// everything the picker's *behaviour* needs, minus the picking backend (the
    /// tests synthesise the presses themselves).
    ///
    /// [`FloaterPlugin`] is not optional here: the picker is a keyed floater, so
    /// the manager is what spawns its window and what carries out the close its
    /// reply buttons ask for.
    fn picker_app() -> App {
        let mut app = App::new();
        let root = app.world_mut().spawn(Node::default()).id();
        app.insert_resource(UiRoot(root));
        // What the manager's systems read and `MinimalPlugins` does not supply:
        // the writing direction the inset mirrors under, the UI scale the
        // on-screen clamp measures in, and the keyboard Ctrl+W reads.
        app.insert_resource(UiDirection::Ltr)
            .init_resource::<UiScale>()
            .init_resource::<ButtonInput<KeyCode>>();
        app.add_plugins(MinimalPlugins)
            .add_plugins(FloaterPlugin)
            .add_plugins(ColorPickerPlugin)
            .init_resource::<Recorded>()
            .add_systems(PostUpdate, record);
        // Two frames: Startup, then one that settles its own change marks so a
        // later churn count measures the sync alone.
        app.update();
        app.update();
        app
    }

    /// The `UiRoot` the floater and the test swatches hang from.
    fn root_of(app: &App) -> Entity {
        app.world().resource::<UiRoot>().0
    }

    /// Spawn a colour swatch and settle a frame, returning it.
    fn swatch(app: &mut App, initial: Color) -> Entity {
        let root = root_of(app);
        let mut queue = bevy::ecs::world::CommandQueue::default();
        let entity = {
            let mut commands = Commands::new(&mut queue, app.world());
            spawn_color_swatch(&mut commands, root, "test", 0, initial)
        };
        queue.apply(app.world_mut());
        app.update();
        entity
    }

    /// Synthesise a primary press on `entity` and run the frame it lands in.
    fn press(app: &mut App, entity: Entity) {
        let location = Location {
            target: NormalizedRenderTarget::None {
                width: 800,
                height: 600,
            },
            position: Vec2::ZERO,
        };
        let event = Pointer::new(
            PointerId::Mouse,
            location,
            Press {
                button: PointerButton::Primary,
                hit: HitData::new(Entity::PLACEHOLDER, 0.0, None, None),
                count: 1,
            },
            entity,
        );
        app.world_mut().trigger(event);
        app.update();
    }

    /// The picker's OK / Cancel button entity.
    fn action_button(app: &mut App, wanted: PickerButton) -> Entity {
        app.world_mut()
            .query::<(Entity, &PickerButton)>()
            .iter(app.world())
            .find(|(_, which)| **which == wanted)
            .map_or(Entity::PLACEHOLDER, |(entity, _)| entity)
    }

    /// The only open picker window's live requester — `None` when no window is
    /// open at all, which for a keyed floater is the same as "closed": its
    /// close despawns it.
    fn requester(app: &mut App) -> Option<Entity> {
        app.world_mut()
            .query::<&ColorPickerState>()
            .iter(app.world())
            .next()
            .and_then(|state| state.requester)
    }

    /// How many picker windows are open.
    fn open_windows(app: &mut App) -> usize {
        app.world_mut()
            .query::<&ColorPickerState>()
            .iter(app.world())
            .count()
    }

    /// The only open picker window's channel values.
    fn channels(app: &mut App) -> Option<[f32; 3]> {
        app.world_mut()
            .query::<&ColorPickerState>()
            .iter(app.world())
            .next()
            .map(|state| state.channels)
    }

    /// The only open picker window's HSL values.
    fn hsl(app: &mut App) -> Option<[f32; 3]> {
        app.world_mut()
            .query::<&ColorPickerState>()
            .iter(app.world())
            .next()
            .map(|state| state.hsl)
    }

    /// The only open picker window's slider for `axis`.
    fn slider_for(app: &mut App, axis: ChannelAxis) -> Option<Entity> {
        app.world_mut()
            .query::<&ColorPickerUi>()
            .iter(app.world())
            .next()
            .and_then(|ui| ui.sliders.get(axis.slot()).copied())
    }

    /// The bytes of a colour, the form the picker actually round-trips.
    fn bytes(color: Color) -> [u8; 4] {
        color.to_srgba().to_u8_array()
    }

    /// Zero the churn counters so the next frames measure only what follows.
    fn settle(app: &mut App) {
        app.update();
        let mut recorded = app.world_mut().resource_mut::<Recorded>();
        recorded.inset_writes = 0;
        recorded.background_writes = 0;
    }

    /// A channel value rounds to its byte, and anything outside 0..=255 is
    /// clamped rather than wrapped.
    #[test]
    fn a_channel_value_rounds_and_clamps_to_a_byte() {
        assert_eq!(byte(0.0), 0);
        assert_eq!(byte(127.4), 127);
        assert_eq!(byte(127.5), 128);
        assert_eq!(byte(255.0), 255);
        assert_eq!(byte(-40.0), 0, "below the floor clamps, it does not wrap");
        assert_eq!(byte(400.0), 255, "above the ceiling clamps");
    }

    /// The live colour is built from the three channel bytes.
    #[test]
    fn the_live_colour_is_the_three_channel_bytes() {
        let mut state = ColorPickerState::default();
        state.set_channels([12.0, 200.4, 255.0]);
        assert_eq!(bytes(state.current()), [12, 200, 255, 255]);
    }

    /// **The HSL model is the reference's.** Its primaries, its greys and its
    /// extremes land where `hslToRgb` puts them, and the round trip back through
    /// `calcHSL` returns what went in.
    #[expect(
        clippy::float_cmp,
        reason = "the grey case is exact by construction — `calcHSL` returns literal zeroes for a \
                  hue and saturation it never computed, and the luminance is the midpoint of two \
                  equal inputs"
    )]
    #[test]
    fn hsl_matches_the_reference_model() {
        let byte_of = |rgb: [f32; 3]| {
            [
                byte(rgb.first().copied().unwrap_or(0.0) * CHANNEL_MAX),
                byte(rgb.get(1).copied().unwrap_or(0.0) * CHANNEL_MAX),
                byte(rgb.get(2).copied().unwrap_or(0.0) * CHANNEL_MAX),
            ]
        };
        assert_eq!(
            byte_of(hsl_to_srgb([0.0, 1.0, 0.5])),
            [255, 0, 0],
            "hue zero at full saturation and mid luminance is pure red"
        );
        assert_eq!(
            byte_of(hsl_to_srgb([1.0 / 3.0, 1.0, 0.5])),
            [0, 255, 0],
            "a third of the way round is pure green"
        );
        assert_eq!(
            byte_of(hsl_to_srgb([2.0 / 3.0, 1.0, 0.5])),
            [0, 0, 255],
            "two thirds is pure blue"
        );
        assert_eq!(
            byte_of(hsl_to_srgb([0.0, 0.0, 0.5])),
            [128, 128, 128],
            "no saturation is a grey of the luminance, whatever the hue"
        );
        assert_eq!(byte_of(hsl_to_srgb([0.7, 0.9, 0.0])), [0, 0, 0]);
        assert_eq!(byte_of(hsl_to_srgb([0.7, 0.9, 1.0])), [255, 255, 255]);

        // The half-saturated colour is the midpoint of grey and the pure hue, in
        // sRGB components — which is what lets the luminance strip be three
        // gradient stops instead of a second generated picture.
        assert_eq!(byte_of(hsl_to_srgb([0.0, 0.5, 0.5])), [191, 64, 64]);

        let round_trip = srgb_to_hsl([1.0, 0.5, 0.0]);
        let back = hsl_to_srgb(round_trip);
        assert_eq!(
            byte_of(back),
            [255, 128, 0],
            "hsl → rgb → hsl → rgb is fixed"
        );
        assert_eq!(
            srgb_to_hsl([0.25, 0.25, 0.25]),
            [0.0, 0.0, 0.25],
            "a grey has no hue and no saturation, as the reference's calcHSL says"
        );
    }

    /// The hex field's two directions agree, and anything that is not six hex
    /// digits is simply not a colour.
    #[test]
    fn hex_round_trips_and_rejects_what_is_not_one() {
        assert_eq!(hex_of(Color::srgb_u8(255, 128, 0)), "FF8000");
        assert_eq!(hex_of(Color::BLACK), "000000");
        assert_eq!(
            color_of_hex("FF8000").map(bytes),
            Some([255, 128, 0, 255]),
            "six digits are a colour"
        );
        assert_eq!(
            color_of_hex("#ff8000").map(bytes),
            Some([255, 128, 0, 255]),
            "a leading hash and lower case are both accepted"
        );
        assert_eq!(color_of_hex("FF80").map(bytes), None, "five is not six");
        assert_eq!(
            color_of_hex("").map(bytes),
            None,
            "an empty field is not a colour"
        );
        assert_eq!(
            color_of_hex("GGGGGG").map(bytes),
            None,
            "nor are letters past F"
        );
    }

    /// The thumb spans the track at both ends: at the range's start its leading
    /// edge is at zero, at the end it is a thumb's width short of the track's, so
    /// the thumb never hangs off either end.
    #[expect(
        clippy::float_cmp,
        reason = "the offsets are exact multiples of the travel, asserted exactly"
    )]
    #[test]
    fn the_thumb_stays_inside_the_track() {
        let range = SliderRange::new(0.0, super::CHANNEL_MAX);
        let travel = super::TRACK_WIDTH - super::THUMB_WIDTH;
        assert_eq!(thumb_offset(0.0, &range), 0.0);
        assert_eq!(thumb_offset(super::CHANNEL_MAX, &range), travel);
        assert_eq!(thumb_offset(super::CHANNEL_MAX / 2.0, &range), travel / 2.0);
        assert_eq!(
            thumb_offset(-10.0, &range),
            0.0,
            "a value under the range clamps to the near end"
        );
        assert_eq!(
            thumb_offset(1000.0, &range),
            travel,
            "a value over the range clamps to the far end"
        );
        let degenerate = SliderRange::new(1.0, 1.0);
        assert_eq!(
            thumb_offset(1.0, &degenerate),
            0.0,
            "an empty range does not divide by zero"
        );
    }

    /// Clicking a swatch opens the picker on that swatch's colour, seeding both
    /// halves of the model and showing the floater.
    #[test]
    fn a_swatch_opens_the_picker_on_its_own_colour() -> Result<(), TestError> {
        let mut app = picker_app();
        let swatch = swatch(&mut app, Color::srgb_u8(10, 20, 30));
        press(&mut app, swatch);
        assert_eq!(requester(&mut app), Some(swatch));
        assert_eq!(open_windows(&mut app), 1, "the window opened");
        assert_eq!(channels(&mut app), Some([10.0, 20.0, 30.0]));
        let opened = hsl(&mut app).ok_or("the window has no HSL")?;
        assert_eq!(
            opened.get(2).map(|luminance| (luminance * 1000.0).round()),
            Some(78.0),
            "and the HSL half was computed from it, not left at zero"
        );
        Ok(())
    }

    /// A [disabled](bevy::ui::InteractionDisabled) swatch does not open the
    /// picker.
    #[test]
    fn a_disabled_swatch_does_not_open_the_picker() -> Result<(), TestError> {
        let mut app = picker_app();
        let swatch = swatch(&mut app, Color::WHITE);
        app.world_mut()
            .entity_mut(swatch)
            .insert(bevy::ui::InteractionDisabled);
        press(&mut app, swatch);
        assert_eq!(open_windows(&mut app), 0, "no window opened");
        Ok(())
    }

    /// **Two swatches asking at once are two windows.** The shared floater this
    /// replaces could answer only one of them and said the other out loud; the
    /// swatch that lost was left showing a live preview nobody would ever commit
    /// or revert.
    #[test]
    fn two_requests_in_a_frame_get_a_window_each() -> Result<(), TestError> {
        let mut app = picker_app();
        let first = swatch(&mut app, Color::srgb_u8(1, 2, 3));
        let second = swatch(&mut app, Color::srgb_u8(9, 8, 7));
        {
            let mut opens = app.world_mut().resource_mut::<Messages<OpenColorPicker>>();
            opens.write(OpenColorPicker {
                requester: first,
                field: Box::from("first"),
                current: Color::srgb_u8(1, 2, 3),
            });
            opens.write(OpenColorPicker {
                requester: second,
                field: Box::from("second"),
                current: Color::srgb_u8(9, 8, 7),
            });
        }
        app.update();
        assert_eq!(open_windows(&mut app), 2, "each swatch got its own window");
        let mut seen: Vec<(Option<Entity>, [f32; 3])> = app
            .world_mut()
            .query::<&ColorPickerState>()
            .iter(app.world())
            .map(|state| (state.requester, state.channels))
            .collect();
        seen.sort_by_key(|(requester, _channels)| *requester);
        let mut wanted = vec![
            (Some(first), [1.0, 2.0, 3.0]),
            (Some(second), [9.0, 8.0, 7.0]),
        ];
        wanted.sort_by_key(|(requester, _channels)| *requester);
        assert_eq!(
            seen, wanted,
            "each window opened on its own swatch's colour"
        );
        Ok(())
    }

    /// An open picker sitting still writes nothing: the thumb insets are what the
    /// logical-box resolver filters on, so re-marking them every frame would put
    /// the whole picker through layout for the life of the process.
    #[test]
    fn an_idle_open_picker_does_not_churn_the_layout() -> Result<(), TestError> {
        let mut app = picker_app();
        let swatch = swatch(&mut app, Color::srgb_u8(64, 128, 192));
        press(&mut app, swatch);
        settle(&mut app);
        for _ in 0..5_u8 {
            app.update();
        }
        let recorded = app.world().resource::<Recorded>();
        assert_eq!(
            recorded.inset_writes, 0,
            "an idle picker re-marks no thumb inset"
        );
        assert_eq!(
            recorded.background_writes, 0,
            "an idle picker re-marks no swatch fill"
        );
        Ok(())
    }

    /// Dragging a channel slider updates the channel, live-previews the new
    /// colour to the requester without committing, and moves that thumb — and
    /// only that thumb.
    #[test]
    fn a_slider_drag_previews_and_moves_its_thumb() -> Result<(), TestError> {
        let mut app = picker_app();
        let swatch = swatch(&mut app, Color::BLACK);
        press(&mut app, swatch);
        settle(&mut app);
        let slider =
            slider_for(&mut app, ChannelAxis::Red).ok_or("the picker has no red slider")?;
        app.world_mut().trigger(ValueChange {
            source: slider,
            value: super::CHANNEL_MAX,
            is_final: false,
        });
        app.update();

        assert_eq!(channels(&mut app), Some([255.0, 0.0, 0.0]));
        let recorded = app.world().resource::<Recorded>();
        let last = recorded.picked.last().ok_or("no preview was emitted")?;
        assert_eq!(last.requester, swatch);
        assert_eq!(bytes(last.color), [255, 0, 0, 255]);
        assert!(!last.final_pick, "a drag previews, it does not commit");

        let thumb_at = |app: &App, slider: Entity| -> Option<Val> {
            let children = app.world().entity(slider).get::<Children>()?;
            let child = children.iter().next()?;
            Some(
                app.world()
                    .entity(child)
                    .get::<LogicalInset>()?
                    .0
                    .inline_start,
            )
        };
        assert_eq!(
            thumb_at(&app, slider),
            Some(Val::Px(super::TRACK_WIDTH - super::THUMB_WIDTH)),
            "a full-scale channel puts the thumb at the far end"
        );
        Ok(())
    }

    /// **Driving the hue slider drives the channels**, and driving a channel
    /// drives the hue back: the two halves of the model are one state, which is
    /// what lets the field, the strip and six sliders all name the same colour.
    #[test]
    fn the_two_models_follow_each_other() -> Result<(), TestError> {
        let mut app = picker_app();
        let swatch = swatch(&mut app, Color::srgb_u8(255, 0, 0));
        press(&mut app, swatch);
        let hue = slider_for(&mut app, ChannelAxis::Hue).ok_or("the picker has no hue slider")?;
        app.world_mut().trigger(ValueChange {
            source: hue,
            value: 120.0_f32,
            is_final: true,
        });
        app.update();
        assert_eq!(
            channels(&mut app),
            Some([0.0, 255.0, 0.0]),
            "a third of the way round the wheel is pure green"
        );

        let blue = slider_for(&mut app, ChannelAxis::Blue).ok_or("no blue slider")?;
        app.world_mut().trigger(ValueChange {
            source: blue,
            value: 255.0_f32,
            is_final: true,
        });
        app.update();
        let after = hsl(&mut app).ok_or("no HSL")?;
        assert_eq!(
            after.first().map(|hue| (hue * 360.0).round()),
            Some(180.0),
            "green plus blue is cyan, and the hue slider now says so"
        );
        Ok(())
    }

    /// **OK** commits the chosen colour to the requester and closes the floater.
    #[test]
    fn ok_commits_the_chosen_colour() -> Result<(), TestError> {
        let mut app = picker_app();
        let swatch = swatch(&mut app, Color::srgb_u8(10, 20, 30));
        press(&mut app, swatch);
        let slider =
            slider_for(&mut app, ChannelAxis::Red).ok_or("the picker has no red slider")?;
        app.world_mut().trigger(ValueChange {
            source: slider,
            value: 200.0_f32,
            is_final: true,
        });
        app.update();
        let ok = action_button(&mut app, PickerButton::Ok);
        press(&mut app, ok);

        let recorded = app.world().resource::<Recorded>();
        let last = recorded.picked.last().ok_or("OK emitted nothing")?;
        assert_eq!(last.requester, swatch);
        assert_eq!(bytes(last.color), [200, 20, 30, 255]);
        assert!(last.final_pick, "OK is the committed choice");
        assert_eq!(
            open_windows(&mut app),
            0,
            "the picker is closed — a keyed window ends on close"
        );
        Ok(())
    }

    /// **Apply now flips, and turning it off silences the live stream.**
    ///
    /// The flag is a resource the settings store merely persists, not a read of
    /// the store: reading the store directly made the checkbox inert in every
    /// host without one — it drew itself permanently ticked and swallowed every
    /// click, which is worse than not being there. This test has no store at
    /// all, which is exactly the case that was broken.
    #[test]
    fn apply_now_toggles_and_gates_the_live_stream() -> Result<(), TestError> {
        let mut app = picker_app();
        let swatch = swatch(&mut app, Color::BLACK);
        press(&mut app, swatch);
        assert!(
            app.world().resource::<ApplyColorImmediately>().0,
            "it starts on, as the reference's setting does"
        );

        let toggle = app
            .world_mut()
            .query_filtered::<Entity, With<ApplyNowToggle>>()
            .iter(app.world())
            .next()
            .ok_or("the picker has no Apply-now toggle")?;
        press(&mut app, toggle);
        assert!(
            !app.world().resource::<ApplyColorImmediately>().0,
            "a press flips it even with no settings store to persist it to"
        );

        let before = app.world().resource::<Recorded>().picked.len();
        let slider = slider_for(&mut app, ChannelAxis::Red).ok_or("no red slider")?;
        app.world_mut().trigger(ValueChange {
            source: slider,
            value: 200.0_f32,
            is_final: false,
        });
        app.update();
        assert_eq!(
            app.world().resource::<Recorded>().picked.len(),
            before,
            "turned off, the picker says nothing at all until OK"
        );

        let ok = action_button(&mut app, PickerButton::Ok);
        press(&mut app, ok);
        let recorded = app.world().resource::<Recorded>();
        let last = recorded.picked.last().ok_or("OK emitted nothing")?;
        assert!(last.final_pick, "OK still commits");
        assert_eq!(bytes(last.color), [200, 0, 0, 255]);
        Ok(())
    }

    /// **Cancel** hands the requester back the colour the picker opened on, and
    /// says so with `final_pick: false` so the consumer reverts its live preview
    /// rather than storing the cancelled colour.
    #[test]
    fn cancel_reverts_to_the_original_and_does_not_commit() -> Result<(), TestError> {
        let mut app = picker_app();
        let swatch = swatch(&mut app, Color::srgb_u8(10, 20, 30));
        press(&mut app, swatch);
        let slider =
            slider_for(&mut app, ChannelAxis::Red).ok_or("the picker has no red slider")?;
        app.world_mut().trigger(ValueChange {
            source: slider,
            value: 200.0_f32,
            is_final: true,
        });
        app.update();
        let cancel = action_button(&mut app, PickerButton::Cancel);
        press(&mut app, cancel);

        let recorded = app.world().resource::<Recorded>();
        let last = recorded.picked.last().ok_or("Cancel emitted nothing")?;
        assert_eq!(last.requester, swatch);
        assert_eq!(
            bytes(last.color),
            [10, 20, 30, 255],
            "the colour the picker opened on"
        );
        assert!(!last.final_pick, "Cancel never commits");
        assert_eq!(open_windows(&mut app), 0);
        Ok(())
    }

    /// A consumer writing a swatch's value repaints that swatch — and a write of
    /// the same colour repaints nothing.
    #[test]
    fn a_swatch_repaints_from_its_value_only_when_it_changes() -> Result<(), TestError> {
        let mut app = picker_app();
        let swatch = swatch(&mut app, Color::BLACK);
        settle(&mut app);
        app.world_mut()
            .entity_mut(swatch)
            .insert(ColorSwatchValue(Color::srgb_u8(255, 0, 0)));
        app.update();
        assert_eq!(
            app.world()
                .entity(swatch)
                .get::<BackgroundColor>()
                .map(|background| bytes(background.0)),
            Some([255, 0, 0, 255])
        );
        let touched = app.world().resource::<Recorded>().background_writes;
        app.world_mut()
            .entity_mut(swatch)
            .insert(ColorSwatchValue(Color::srgb_u8(255, 0, 0)));
        app.update();
        assert_eq!(
            app.world().resource::<Recorded>().background_writes,
            touched,
            "re-writing the same colour repaints nothing"
        );
        Ok(())
    }

    /// **The eyedropper reads the frame it was given**, at the pointer's
    /// physical pixel, and says nothing at all off the edge of it.
    mod eyedropper {
        use bevy::asset::RenderAssetUsages;
        use bevy::image::Image;
        use bevy::math::Vec2;
        use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
        use pretty_assertions::assert_eq;

        use super::{TestError, bytes};
        use crate::ui_color_picker::sample_frame;

        /// A 2×2 frame whose four texels are four different colours.
        fn frame() -> Image {
            Image::new(
                Extent3d {
                    width: 2,
                    height: 2,
                    depth_or_array_layers: 1,
                },
                TextureDimension::D2,
                vec![
                    255, 0, 0, 255, // (0, 0) red
                    0, 255, 0, 255, // (1, 0) green
                    0, 0, 255, 255, // (0, 1) blue
                    255, 255, 0, 255, // (1, 1) yellow
                ],
                TextureFormat::Rgba8UnormSrgb,
                RenderAssetUsages::RENDER_WORLD,
            )
        }

        /// The sample is the texel under the pointer, after the window's scale
        /// factor has turned the logical position into a physical one.
        #[test]
        fn a_sample_is_the_texel_under_the_pointer() -> Result<(), TestError> {
            let frame = frame();
            assert_eq!(
                sample_frame(&frame, Vec2::new(0.2, 0.2), 1.0).map(bytes),
                Some([255, 0, 0, 255])
            );
            assert_eq!(
                sample_frame(&frame, Vec2::new(1.5, 0.0), 1.0).map(bytes),
                Some([0, 255, 0, 255])
            );
            assert_eq!(
                sample_frame(&frame, Vec2::new(0.0, 1.0), 1.0).map(bytes),
                Some([0, 0, 255, 255])
            );
            assert_eq!(
                sample_frame(&frame, Vec2::new(0.5, 0.5), 2.0).map(bytes),
                Some([255, 255, 0, 255]),
                "a doubled scale factor puts the same logical point on the far texel"
            );
            Ok(())
        }

        /// Off the frame — past either edge, or behind the origin — there is no
        /// colour, and the sampler says so rather than clamping to a lie.
        #[test]
        fn a_sample_off_the_frame_is_nothing() {
            let frame = frame();
            assert!(sample_frame(&frame, Vec2::new(2.0, 0.0), 1.0).is_none());
            assert!(sample_frame(&frame, Vec2::new(0.0, 2.0), 1.0).is_none());
            assert!(sample_frame(&frame, Vec2::new(-1.0, 0.0), 1.0).is_none());
        }
    }

    /// **The picker, driven** (`viewer-ui-widget-interaction-suite`): a click on
    /// the swatch, a drag along a channel track, a drag across the hue ×
    /// saturation field, and OK — through the real pointer, on the real
    /// geometry.
    ///
    /// Every test above hands the widget a `ValueChange` already carrying the
    /// number it is meant to arrive at, which makes them tests of what the
    /// picker does with a value and not of where a value comes from. A slider
    /// and a two-dimensional field are the widgets here whose output is a
    /// *function of their own layout*: a track that laid out at the wrong size,
    /// or a field whose picture is not where the pointer thinks it is, moves the
    /// colour by the wrong amount for a gesture that still looks right. Only a
    /// real drag on a laid-out control can see that.
    mod scenarios {
        use bevy::prelude::*;
        use bevy::ui_widgets::{SliderRange, SliderThumb, SliderValue};
        use pretty_assertions::assert_eq;

        use super::{TestError, bytes};
        use crate::ui_color_picker::{
            CHANNEL_MAX, ColorPicked, ColorPickerPlugin, FIELD_SIZE, MARKER_SIZE, THUMB_WIDTH,
            TRACK_WIDTH, spawn_color_swatch, thumb_offset,
        };
        use crate::ui_test::interact::{self, InteractionTest, centre_of};
        use crate::ui_test::{drain, find_by_name, record, settle};
        use sl_viewer_ui_core::ui::{LogicalInset, UiRoot, UiScaffoldSystems};

        /// The swatch's node name.
        const SWATCH: &str = "test:color-swatch";

        /// The red channel's slider.
        const RED_SLIDER: &str = "color-picker-slider:R";

        /// The hue × saturation field.
        const FIELD: &str = "color-picker-field";

        /// How far the drag travels along the track, in logical pixels. Chosen
        /// so the value it lands on is exact: the usable track is
        /// `TRACK_WIDTH - THUMB_WIDTH` = 150 px for a span of 255, so 60 px is
        /// 102 — no rounding to hide a small error behind.
        const DRAG_PX: f32 = 60.0;

        /// The channel value [`DRAG_PX`] must produce.
        const DRAGGED_VALUE: f32 = 102.0;

        /// A swatch and the picker floater under the real pointer stack.
        fn picker_app() -> App {
            let mut app = InteractionTest::new().build();
            // The picker is a keyed floater: the manager is what spawns its
            // window and what carries out the close OK asks for.
            app.add_plugins(crate::floater::FloaterPlugin);
            app.add_plugins(ColorPickerPlugin);
            record::<ColorPicked>(&mut app);
            app.add_systems(
                Startup,
                (|mut commands: Commands, root: Res<UiRoot>| {
                    spawn_color_swatch(&mut commands, root.0, "test", 1, Color::BLACK);
                })
                .after(UiScaffoldSystems::SpawnRoot),
            );
            settle(&mut app);
            settle(&mut app);
            app
        }

        /// Whether a picker window is on screen. The picker is a keyed floater,
        /// so it exists only while it is open: its close despawns it.
        fn picker_shown(app: &mut App) -> bool {
            app.world_mut()
                .query::<&super::ColorPickerState>()
                .iter(app.world())
                .next()
                .is_some()
        }

        /// The open picker window's red channel value.
        fn red_channel(app: &mut App) -> Option<f32> {
            app.world_mut()
                .query::<&super::ColorPickerState>()
                .iter(app.world())
                .next()
                .and_then(|state| state.channels.first().copied())
        }

        /// The open picker window's HSL triple.
        fn hsl(app: &mut App) -> Option<[f32; 3]> {
            app.world_mut()
                .query::<&super::ColorPickerState>()
                .iter(app.world())
                .next()
                .map(|state| state.hsl)
        }

        /// Where the named slider's thumb sits along its track, in logical
        /// pixels from the leading edge.
        fn thumb_at(app: &mut App, slider: &str) -> Option<f32> {
            let entity = find_by_name(app, slider)?;
            let children = app.world().get::<Children>(entity)?;
            let thumb = children
                .iter()
                .find(|child| app.world().get::<SliderThumb>(*child).is_some())?;
            match app.world().get::<LogicalInset>(thumb)?.0.inline_start {
                Val::Px(px) => Some(px),
                _other => None,
            }
        }

        /// Clicking the swatch opens the picker; dragging a channel track moves
        /// that channel by what the gesture actually travelled; OK commits it
        /// and puts the picker away.
        #[test]
        fn a_swatch_click_a_track_drag_and_ok() -> Result<(), TestError> {
            let mut app = picker_app();
            assert!(!picker_shown(&mut app), "the picker starts closed");

            interact::click_node(&mut app, SWATCH)?;
            settle(&mut app);
            assert!(
                picker_shown(&mut app),
                "a click on the swatch opens the picker"
            );
            assert_eq!(
                thumb_at(&mut app, RED_SLIDER),
                Some(0.0),
                "it opens on the swatch's colour — black, so every thumb is home"
            );
            let _opening = drain::<ColorPicked>(&mut app);

            let track = centre_of(&mut app, RED_SLIDER).ok_or("the red track never laid out")?;
            interact::drag(
                &mut app,
                track,
                Vec2::new(track.x + DRAG_PX, track.y),
                4,
                MouseButton::Left,
            );
            settle(&mut app);

            let red = red_channel(&mut app).ok_or("no red channel")?;
            assert!(
                (red - DRAGGED_VALUE).abs() < 1.0,
                "a {DRAG_PX} px drag along a {TRACK_WIDTH} px track (thumb {THUMB_WIDTH}) is \
                 {DRAGGED_VALUE} of {CHANNEL_MAX}, not {red}"
            );
            let thumb = thumb_at(&mut app, RED_SLIDER).ok_or("the thumb went missing")?;
            let wanted = thumb_offset(red, &SliderRange::new(0.0, CHANNEL_MAX));
            assert!(
                (thumb - wanted).abs() < 0.5,
                "the thumb follows the value it produced: {thumb} vs {wanted}"
            );

            let previews = drain::<ColorPicked>(&mut app);
            let last = previews.last().ok_or("the drag previewed nothing")?;
            assert!(
                !last.final_pick,
                "a drag previews, it does not commit: {last:?}"
            );
            let [red_byte, green, blue, _alpha] = bytes(last.color);
            assert_eq!(
                (red_byte, green, blue),
                (102, 0, 0),
                "the preview is the dragged channel and nothing else"
            );

            interact::click_node(&mut app, "color-picker-button:color-picker-ok")?;
            settle(&mut app);

            let committed = drain::<ColorPicked>(&mut app);
            let commit = committed
                .iter()
                .find(|reply| reply.final_pick)
                .ok_or("OK committed nothing")?;
            assert_eq!(bytes(commit.color), [102, 0, 0, 255]);
            assert!(!picker_shown(&mut app), "OK puts the picker away");
            Ok(())
        }

        /// **The field aims at its picture, not its box.** The outer box is a
        /// marker wider than the picture inside it, so a click at the box's
        /// centre must be the picture's centre — half way round the hue wheel,
        /// half saturated — and a click a quarter of the picture in from the
        /// left must be a quarter of the way round. A field that measured the
        /// pointer against the box would be off by half a marker everywhere, and
        /// would never quite reach either end.
        #[test]
        fn a_field_click_aims_at_the_picture_inside_the_box() -> Result<(), TestError> {
            let mut app = picker_app();
            interact::click_node(&mut app, SWATCH)?;
            settle(&mut app);

            let centre = centre_of(&mut app, FIELD).ok_or("the field never laid out")?;
            interact::click(&mut app, centre, MouseButton::Left);
            settle(&mut app);
            let middle = hsl(&mut app).ok_or("no HSL")?;
            assert!(
                middle.first().is_some_and(|hue| (hue - 0.5).abs() < 0.02),
                "the picture's centre is half way round the wheel: {middle:?}"
            );
            assert!(
                middle
                    .get(1)
                    .is_some_and(|saturation| (saturation - 0.5).abs() < 0.02),
                "and half saturated: {middle:?}"
            );

            // A quarter of the picture in from its left edge. The picture starts
            // half a marker inside the box, and the box's centre is the
            // picture's, so a quarter of the picture's width to the left of
            // centre is the three-eighths mark.
            let quarter = Vec2::new(centre.x - FIELD_SIZE / 4.0, centre.y);
            interact::click(&mut app, quarter, MouseButton::Left);
            settle(&mut app);
            let aimed = hsl(&mut app).ok_or("no HSL")?;
            assert!(
                aimed.first().is_some_and(|hue| (hue - 0.25).abs() < 0.02),
                "a quarter of the picture in is a quarter of the way round: {aimed:?} \
                 (field {FIELD_SIZE}, marker {MARKER_SIZE})"
            );
            Ok(())
        }

        /// **A picker that is not a floater window still answers its controls.**
        ///
        /// Every other test here drives the *live* picker, whose state sits on
        /// the floater root — so a handler that looked the state up by walking
        /// to the enclosing **window** rather than to the picker passes all of
        /// them. The gallery's specimen is the case that is not a window: its
        /// state is on the body's own root, inside a floater whose root knows
        /// nothing about colours, and against a window-shaped lookup it drew
        /// perfectly and answered nothing at all.
        #[test]
        fn a_picker_that_is_not_a_floater_still_answers_its_controls() -> Result<(), TestError> {
            let mut app = InteractionTest::new().build();
            // `FloaterPlugin` for the picker's own open path (it opens keyed
            // windows and its `KeyedFloaters` wants the manager's resources) —
            // but the specimen below is spawned straight under the UI root, so
            // nothing in its parent chain is a floater.
            app.add_plugins(crate::floater::FloaterPlugin);
            app.add_plugins(ColorPickerPlugin);
            app.add_systems(
                Startup,
                (|mut commands: Commands, root: Res<UiRoot>| {
                    crate::ui_color_picker::spawn_color_picker_specimen(
                        &mut commands,
                        root.0,
                        sl_viewer_ui_core::ui_element::ElementCx::new(),
                    );
                })
                .after(UiScaffoldSystems::SpawnRoot),
            );
            settle(&mut app);
            settle(&mut app);

            let centre = centre_of(&mut app, FIELD).ok_or("the field never laid out")?;
            interact::click(&mut app, centre, MouseButton::Left);
            settle(&mut app);

            let aimed = hsl(&mut app).ok_or("the specimen has no state")?;
            assert!(
                aimed.first().is_some_and(|hue| (hue - 0.5).abs() < 0.02),
                "a click on the field's centre aims at half the wheel: {aimed:?}"
            );
            assert!(
                aimed
                    .get(1)
                    .is_some_and(|saturation| (saturation - 0.5).abs() < 0.02),
                "and half saturation: {aimed:?}"
            );
            Ok(())
        }

        /// A drag that starts on the track and is released far outside it still
        /// belongs to the slider it began on — the pointer capture every drag
        /// widget relies on, and the reason a user can slide past the end of a
        /// short track without the value freezing.
        #[test]
        fn a_drag_leaving_the_track_keeps_driving_it() -> Result<(), TestError> {
            let mut app = picker_app();
            interact::click_node(&mut app, SWATCH)?;
            settle(&mut app);

            let track = centre_of(&mut app, RED_SLIDER).ok_or("the red track never laid out")?;
            // Well below the row, and past the track's trailing end: a slider
            // that only listened while hovered would stop here.
            interact::drag(
                &mut app,
                track,
                Vec2::new(track.x + TRACK_WIDTH, track.y + 120.0),
                4,
                MouseButton::Left,
            );
            settle(&mut app);

            let red = red_channel(&mut app).ok_or("no red channel")?;
            let slider = find_by_name(&mut app, RED_SLIDER).ok_or("the slider went missing")?;
            let value = app
                .world()
                .get::<SliderValue>(slider)
                .map(|value| value.0)
                .ok_or("the slider lost its value")?;
            assert!(
                (red - CHANNEL_MAX).abs() < f32::EPSILON,
                "a drag past the end pins the channel at its maximum: {red}"
            );
            assert!(
                (value - red).abs() < f32::EPSILON,
                "and the slider and the picker agree about it: {value} vs {red}"
            );
            Ok(())
        }
    }
}
