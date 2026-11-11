//! The **reusable slider widget**: the track box, the thumb inside it, and the
//! one system that puts the thumb where the value says.
//!
//! # Why this exists
//!
//! `bevy_ui_widgets` gives a slider its *behaviour* — its `Slider` takes the
//! presses and drags and writes [`SliderValue`] — and says in as many words that
//! drawing it is the stylist's job. So every panel that wanted one drew its own:
//! a bordered track node, an absolutely-positioned thumb inside it, a marker
//! component to find that thumb again, and a system computing
//! `fraction * (track - thumb)` to place it. Ten panels, ten copies, in seven
//! crates.
//!
//! They agreed on everything but their constants, and they were all wrong in the
//! same way: the thumb was given the track's **own** height, which is its
//! *border box*, while an absolutely-positioned child is placed inside the
//! border — so every thumb in the viewer hung one border-width past the bottom
//! of its track. One defect, ten places to fix it, and it survived because no
//! single place looked wrong (`viewer-ui-rows-shorter-than-their-text`).
//!
//! Here the thumb has **no height at all**: its block insets are both zero, so it
//! stretches to exactly the track's interior, and there is one place for that to
//! be true.
//!
//! # What a caller still owns
//!
//! The behaviour bundle (a plain `Slider` or a bound one), the `Name`, any
//! observer, and what the value *means*. [`spawn_slider`] returns the track, so
//! anything else goes on with an ordinary `insert` / `observe`.
//!
//! # The skin paints it
//!
//! The track wears [`SLIDER_CLASS`] and the thumb [`SLIDER_THUMB_CLASS`], so
//! `common.css` paints both from the `--track-bg`, `--control-border` and
//! `--slider-thumb` tokens and greys a refused one from `:disabled`. The colours
//! in [`SliderStyle`] are the **pre-load fallback** — what a headless harness,
//! which resolves no stylesheet, and the frame before the sheet lands measure —
//! the same arrangement as a `ButtonSpec`'s. Before the classes, every slider in
//! the viewer painted whatever colours its panel had picked, and no skin could
//! reach one.
//!
//! A **static** slider — a gallery specimen, a screenshot fixture — needs no
//! plugin: [`slider_thumb`] takes the fraction to draw at, so the thumb is in the
//! right place the moment it is spawned. [`SliderWidgetPlugin`] is what keeps a
//! *live* one in step with its value.
//!
//! # The numbers it shows
//!
//! A track and a thumb alone say nothing a user can read: not the value, not how
//! far the track goes, not what one notch moves it by
//! (`viewer-sliders-show-no-value`). So unless a caller says otherwise
//! ([`SliderLabels`]) a slider also draws
//!
//! - a **readout** of its value beside the track (or in a text node the caller
//!   placed, such as an environment editor's caption line);
//! - its **bounds**, small, under the two ends of the track — except where the
//!   range says nothing and the row has no room for a second line (a volume in
//!   a toolbar);
//! - its **step**, in a tooltip while the pointer is over the track.
//!
//! All three are written by [`label_sliders`] and [`SliderWidgetPlugin`]'s
//! step tooltip from the slider's own `SliderValue`, `SliderRange` and
//! `SliderStep`, through one [`SliderFormat`] per slider — so the ends and the readout never disagree
//! about how a number is written, and the forty-odd sliders that existed before
//! the readout got it without each writing its own.

use bevy::input_focus::tab_navigation::TabIndex;
use bevy::picking::hover::Hovered;
use bevy::prelude::*;
use bevy::ui_widgets::{SliderRange, SliderStep, SliderThumb, SliderValue};
use bevy::window::PrimaryWindow;
use bevy_flair::style::components::ClassList;

use sl_viewer_ui_core::i18n::{TransArgs, Translator};
use sl_viewer_ui_core::skin::{text_role, tooltip_box, tooltip_text};
use sl_viewer_ui_core::skin_palette::SkinPalette;
use sl_viewer_ui_core::ui::{
    LogicalInset, LogicalRect, UiRoot, column, resolve_logical_boxes, row,
};
use sl_viewer_ui_core::ui_font::UiFont;

/// The skin class on a slider's **track** — the node that carries the value,
/// the tab stop and, when refused, `InteractionDisabled`, so `:disabled` is
/// selected here and reaches the thumb as a descendant.
pub const SLIDER_CLASS: &str = "sk-slider";

/// The skin class on a slider's **thumb**.
pub const SLIDER_THUMB_CLASS: &str = "sk-slider-thumb";

/// How a slider is drawn: its geometry in **logical** pixels, and its colours.
///
/// The colours are the pre-load fallback: the skin paints the track and thumb
/// through [`SLIDER_CLASS`] and [`SLIDER_THUMB_CLASS`] (see the module docs).
///
/// A plain data struct rather than a builder: every field is load-bearing, a
/// slider with a default width would be a slider nobody sized, and the call
/// sites already had their values as constants of their own.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SliderStyle {
    /// The track's width — the slider's whole extent along the inline axis.
    pub track_width: f32,
    /// The track's height, border included.
    pub track_height: f32,
    /// The track's border width, on every edge.
    pub border: f32,
    /// The track's border colour.
    pub border_color: Color,
    /// The track's fill, behind the thumb.
    pub track_fill: Color,
    /// The thumb's width. Its height is the track's interior, never a number.
    pub thumb_width: f32,
    /// The thumb's fill.
    pub thumb_fill: Color,
    /// The readout's font size, logical px — the row's own text size, so the
    /// number reads as part of the row. The bounds under the track are drawn
    /// smaller ([`END_LABEL_SCALE`]).
    pub font_size: f32,
}

impl SliderStyle {
    /// How far the thumb travels: the track's interior — its width less the
    /// border on both ends, the box an absolutely positioned thumb is placed in
    /// — less the thumb's own width, since the thumb has to stay inside that
    /// interior at either end.
    ///
    /// The same number `bevy_ui_widgets`' core slider works out from the track's
    /// measured border and the thumb's *measured* size when it converts a click
    /// or a drag into a value, which is why the thumb must carry a real width and
    /// why the two agree. Counting the border box here instead drew every thumb
    /// one border width off the value the pointer set, and let it overhang the
    /// trailing border at the maximum
    /// (`viewer-slider-thumb-offset-by-track-border`).
    #[must_use]
    pub fn travel(&self) -> f32 {
        (self.track_width - 2.0 * self.border - self.thumb_width).max(0.0)
    }
}

/// The travel of the track this is on, logical px — what
/// [`place_slider_thumbs`] needs and the only thing it needs.
///
/// On the *track*, not the thumb: the track is what carries the value and the
/// range, so one lookup answers the whole question.
#[derive(Component, Debug, Clone, Copy)]
pub struct SliderTravel(f32);

/// Where in its range a slider sits, as 0…1.
///
/// Zero for an empty range rather than a division by it.
#[must_use]
pub fn slider_fraction(value: &SliderValue, range: &SliderRange) -> f32 {
    let span = range.span();
    if span > f32::EPSILON {
        ((value.0 - range.start()) / span).clamp(0.0, 1.0)
    } else {
        0.0
    }
}

/// How much smaller than the readout the bounds under the track are drawn: they
/// are a scale to read the thumb against, not a second value.
pub const END_LABEL_SCALE: f32 = 0.8;

/// The narrowest a readout beside the track is, in multiples of its font size —
/// so a column of sliders lines its readouts up and a value growing a digit
/// does not shove the row. A longer value still grows it rather than clip.
const READOUT_MIN_EMS: f32 = 3.5;

/// The gap between the track and a readout beside it, logical px.
const READOUT_GAP: f32 = 6.0;

/// The most decimals [`SliderFormat::for_step`] gives a step, and so a readout:
/// the environment's scattering terms are the finest scale in the viewer, and
/// they are read at eight.
pub const MAX_DECIMALS: u8 = 8;

/// The step tooltip's font size, logical px — every other tooltip's.
const TOOLTIP_FONT_SIZE: f32 = 12.0;

/// Where the step tooltip sits from the pointer, logical px: below and to the
/// trailing side, clear of the cursor glyph, as the link tooltip's.
const TOOLTIP_CURSOR_OFFSET: Vec2 = Vec2::new(14.0, 18.0);

/// How a slider writes a number — its value, its bounds and its step, all three
/// the same way, so the ends and the readout cannot disagree about units.
///
/// The unit itself is the row's label's ("Maximum bandwidth (kbps)"), as in the
/// reference, so a format is only the shape of the number.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub enum SliderFormat {
    /// A decimal number with this many fraction digits, in the locale's own
    /// decimal mark and grouping: `4`, `0.70`, `1,024`.
    Decimal(u8),
    /// A `0…1` value as a whole percentage: `0.35` is `35%` — a volume.
    Percent,
}

impl SliderFormat {
    /// The decimal format that shows every notch of `step` apart: a step of
    /// `1` reads as an integer, `0.05` at two decimals, `0.001` at three.
    ///
    /// The format a slider whose number is just "a quantity" wants, which is
    /// most of them; a slider that knows better (a volume, a knob whose
    /// meaningful precision is not its step's) names its own.
    #[must_use]
    pub fn for_step(step: f32) -> Self {
        Self::Decimal(step_decimals(step))
    }
}

/// How many fraction digits it takes to write `step` exactly, capped at
/// [`MAX_DECIMALS`] — zero for a whole step, and for a step that is not a
/// positive number at all.
///
/// The tolerance is relative, because a step is an `f32`: `0.05` is stored as
/// `0.0500000007…`, which is a hundredth-exact step to anyone but the float.
#[must_use]
pub fn step_decimals(step: f32) -> u8 {
    let step = f64::from(step.abs());
    if !step.is_normal() {
        return 0;
    }
    let mut scaled = step;
    for decimals in 0..MAX_DECIMALS {
        if (scaled - scaled.round()).abs() <= scaled * 1e-4 {
            return decimals;
        }
        scaled *= 10.0;
    }
    MAX_DECIMALS
}

/// `value` written as `format` says, in the active locale when there is one —
/// a headless harness with no locale resources gets the plain Rust rendering,
/// which is the English one.
#[must_use]
pub fn slider_text(
    format: SliderFormat,
    value: f32,
    translator: Option<&Translator<'_>>,
) -> String {
    match format {
        SliderFormat::Decimal(decimals) => decimal_text(f64::from(value), decimals, translator),
        SliderFormat::Percent => {
            let number = decimal_text(f64::from(value) * 100.0, 0, translator);
            translator.map_or_else(
                || format!("{number}%"),
                // The sign's side and spacing are the locale's (`35 %` in
                // German), so the bundle writes it around the number.
                |translator| {
                    translator.format("slider-percent", &TransArgs::new().text("percent", &number))
                },
            )
        }
    }
}

/// `value` rounded to `decimals` places, as text.
fn decimal_text(value: f64, decimals: u8, translator: Option<&Translator<'_>>) -> String {
    let scale = 10_f64.powi(i32::from(decimals));
    // Adding zero turns a negative zero into a positive one, so a value a hair
    // under zero reads `0.00` rather than `-0.00`.
    let rounded = (value * scale).round() / scale + 0.0;
    translator.map_or_else(
        || {
            let digits = usize::from(decimals);
            format!("{rounded:.digits$}")
        },
        |translator| translator.decimal(rounded, decimals),
    )
}

/// What a slider shows besides its track: where its value goes, and whether its
/// bounds are drawn. A slider that shows anything also shows its step, in a
/// tooltip over the track.
///
/// Built from the constructors, each of which is one arrangement a caller
/// wants: [`beside`](Self::beside), [`written_into`](Self::written_into),
/// [`bounds_only`](Self::bounds_only), [`value_only`](Self::value_only).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SliderLabels {
    /// The numbers the slider draws.
    Shown {
        /// How every number of the slider is written.
        format: SliderFormat,
        /// Where its value goes.
        readout: SliderReadout,
        /// Whether its bounds are drawn under the track.
        bounds: bool,
    },
    /// Nothing at all: the media scrubber, whose position and length are its
    /// own clock text and whose range is whatever the clip lasts.
    None,
}

impl SliderLabels {
    /// The value in a readout beside the track's trailing end and the bounds
    /// under it — what a slider in a row of label and control wants.
    #[must_use]
    pub const fn beside(format: SliderFormat) -> Self {
        Self::Shown {
            format,
            readout: SliderReadout::Beside,
            bounds: true,
        }
    }

    /// The value written into a `Text` the caller placed — an environment
    /// editor's caption line, where a readout beside the track would make four
    /// columns too wide for any window — and the bounds under the track.
    #[must_use]
    pub const fn written_into(text: Entity, format: SliderFormat) -> Self {
        Self::Shown {
            format,
            readout: SliderReadout::Into(text),
            bounds: true,
        }
    }

    /// No readout, because the value is already shown by a control of its own
    /// beside the slider — the reference's spinner, which a user can type into
    /// — but the bounds under the track.
    #[must_use]
    pub const fn bounds_only(format: SliderFormat) -> Self {
        Self::Shown {
            format,
            readout: SliderReadout::Elsewhere,
            bounds: true,
        }
    }

    /// The readout beside the track and no bounds, for a slider whose range
    /// says nothing — a volume, which runs from silent to full in every window
    /// — in a bar too short for a second line.
    #[must_use]
    pub const fn value_only(format: SliderFormat) -> Self {
        Self::Shown {
            format,
            readout: SliderReadout::Beside,
            bounds: false,
        }
    }
}

/// Where a slider's value is written.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SliderReadout {
    /// A readout the widget spawns beside the track's trailing end.
    Beside,
    /// A `Text` the caller placed.
    Into(Entity),
    /// Nowhere by the slider: a control beside it shows the value.
    Elsewhere,
}

/// The text nodes a slider's numbers are written into, on its **track**.
///
/// Spawned by [`spawn_slider`] from a [`SliderLabels`]; [`label_sliders`]
/// keeps them current.
#[derive(Component, Debug, Clone, Copy)]
pub struct SliderLabelParts {
    /// How every number of this slider is written.
    pub format: SliderFormat,
    /// The value's readout, when the slider writes one.
    pub readout: Option<Entity>,
    /// The range's start and end, under the track's two ends, when the slider
    /// draws its bounds.
    pub bounds: Option<(Entity, Entity)>,
}

/// The track's own bundle: the bordered box, its fallback colours and skin
/// class, its tab stop, and the travel its thumb will need.
///
/// `flex_shrink: 0.0` deliberately. The thumb is placed from `style`'s widths,
/// so a track squeezed by a crowded row would put its thumb somewhere the value
/// never said — a slider is a fixed-width control or it is lying.
#[must_use]
pub fn slider_track(style: SliderStyle, tab_index: i32) -> impl Bundle {
    (
        Node {
            width: Val::Px(style.track_width),
            height: Val::Px(style.track_height),
            border: UiRect::all(Val::Px(style.border)),
            flex_shrink: 0.0,
            ..default()
        },
        BorderColor::all(style.border_color),
        BackgroundColor(style.track_fill),
        ClassList::new_with_classes([SLIDER_CLASS]),
        TabIndex(tab_index),
        Pickable::default(),
        SliderTravel(style.travel()),
    )
}

/// The thumb's bundle, drawn at `fraction` (0…1) of its track's travel.
///
/// **No height**, which is the whole point of this widget existing: both block
/// insets are zero, so the thumb stretches to the track's interior. Handing it
/// the track's own height instead — as all ten hand-built sliders did — makes it
/// as tall as the *border box* while it sits inside the border, so it overhangs
/// the bottom by exactly the border.
#[must_use]
pub fn slider_thumb(style: SliderStyle, fraction: f32) -> impl Bundle {
    (
        SliderThumb,
        Node {
            position_type: PositionType::Absolute,
            width: Val::Px(style.thumb_width),
            ..default()
        },
        LogicalInset(LogicalRect {
            inline_start: Val::Px(fraction.clamp(0.0, 1.0) * style.travel()),
            ..LogicalRect::ZERO
        }),
        BackgroundColor(style.thumb_fill),
        ClassList::new_with_classes([SLIDER_THUMB_CLASS]),
        // The press that starts a drag belongs to the track: it is what carries
        // the `Slider`, and it is what turns a pointer position into a value.
        Pickable::IGNORE,
    )
}

/// Spawn a whole slider under `parent`: the track carrying `behaviour`, with its
/// thumb inside it, drawn at `fraction`, and the numbers `labels` asks for.
/// Returns the **track**, which is the entity every other component belongs on.
///
/// With any labels the track is not `parent`'s child but sits in a small group
/// of its own — the track over its bounds, and the readout beside them — which
/// is the one node `parent` gains.
pub fn spawn_slider(
    commands: &mut Commands,
    parent: Entity,
    style: SliderStyle,
    tab_index: i32,
    fraction: f32,
    labels: SliderLabels,
    behaviour: impl Bundle,
) -> Entity {
    let SliderLabels::Shown {
        format,
        readout: placement,
        bounds: bounded,
    } = labels
    else {
        return commands
            .spawn((behaviour, slider_track(style, tab_index), ChildOf(parent)))
            .with_child(slider_thumb(style, fraction))
            .id();
    };
    let group = commands
        .spawn((
            Node {
                // The top, not the centre: the readout belongs to the track's
                // line, not halfway down the bounds under it. Without bounds
                // the track is the group's whole height, and the readout is
                // centred on it.
                align_items: if bounded {
                    AlignItems::FlexStart
                } else {
                    AlignItems::Center
                },
                flex_shrink: 0.0,
                ..row(Val::Px(READOUT_GAP))
            },
            ChildOf(parent),
        ))
        .id();
    let scale = commands
        .spawn((
            Node {
                width: Val::Px(style.track_width),
                flex_shrink: 0.0,
                ..column(Val::Px(0.0))
            },
            ChildOf(group),
        ))
        .id();
    let track = commands
        .spawn((behaviour, slider_track(style, tab_index), ChildOf(scale)))
        .with_child(slider_thumb(style, fraction))
        .id();
    // The bounds share a line under the track, one at each end; `row` follows
    // the text direction, as the thumb's inline inset does, so in a
    // right-to-left layout the start is on the right at both.
    let bounds = bounded.then(|| {
        let ends = commands
            .spawn((
                Node {
                    width: Val::Percent(100.0),
                    justify_content: JustifyContent::SpaceBetween,
                    ..row(Val::Px(4.0))
                },
                Pickable::IGNORE,
                ChildOf(scale),
            ))
            .id();
        let end_font = style.font_size * END_LABEL_SCALE;
        let start = commands
            .spawn((slider_number(end_font, false), ChildOf(ends)))
            .id();
        let end = commands
            .spawn((slider_number(end_font, false), ChildOf(ends)))
            .id();
        (start, end)
    });
    let readout = match placement {
        SliderReadout::Into(text) => Some(text),
        SliderReadout::Elsewhere => None,
        SliderReadout::Beside => Some(
            commands
                .spawn((
                    slider_number(style.font_size, true),
                    Node {
                        min_width: Val::Px(style.font_size * READOUT_MIN_EMS),
                        flex_shrink: 0.0,
                        ..default()
                    },
                    ChildOf(group),
                ))
                .id(),
        ),
    };
    commands.entity(track).insert(SliderLabelParts {
        format,
        readout,
        bounds,
    });
    // The step tooltip's trigger. `if_new`, so a behaviour bundle that tracks
    // its own hover keeps it — either one answers the same question.
    commands.entity(track).insert_if_new(Hovered::default());
    track
}

/// A number's text node: empty until [`label_sliders`] writes it, never
/// wrapping, never taking the pointer. `readout` for the value — the primary
/// text role, set against the trailing edge of its column so a column of
/// readouts lines up on the digits — and otherwise a bound, muted.
fn slider_number(font_size: f32, readout: bool) -> impl Bundle {
    let (color, justify) = if readout {
        (SkinPalette::FALLBACK.text_primary, Justify::End)
    } else {
        (SkinPalette::FALLBACK.text_muted, Justify::default())
    };
    (
        Text::default(),
        TextLayout {
            linebreak: LineBreak::NoWrap,
            justify,
        },
        UiFont::Sans.at(font_size),
        text_role(color),
        Pickable::IGNORE,
    )
}

/// Slide every thumb to the value of the track it is on.
///
/// In `PostUpdate`, before the scaffold resolves logical boxes, rather than in
/// `Update` beside whatever wrote the value: this way it sees the *settled*
/// value of the frame whoever set it — a drag, a trackball, a setting reloaded
/// from the store — instead of each panel having to chain it after its own
/// writer and a new writer silently arriving a frame late.
pub fn place_slider_thumbs(
    sliders: Query<(&SliderValue, &SliderRange, &SliderTravel, &Children)>,
    mut thumbs: Query<&mut LogicalInset, With<SliderThumb>>,
) {
    for (value, range, travel, children) in &sliders {
        let offset = Val::Px(slider_fraction(value, range) * travel.0);
        for child in children {
            if let Ok(mut inset) = thumbs.get_mut(*child)
                && inset.0.inline_start != offset
            {
                // Only on a change: the inset is a logical box, and touching one
                // re-resolves it and dirties the node's layout. A slider nobody
                // is dragging should cost nothing.
                inset.0.inline_start = offset;
            }
        }
    }
}

/// Write every slider's numbers — its value into its readout, its range into
/// its bounds — where they changed, and all of them when the locale did.
///
/// Public so a host that composes a specimen without the plugin can draw them
/// once; in the viewer [`SliderWidgetPlugin`] runs it beside
/// [`place_slider_thumbs`], so it sees the same settled value the thumb does.
pub fn label_sliders(
    sliders: Query<(Ref<SliderLabelParts>, Ref<SliderValue>, Ref<SliderRange>)>,
    translator: Option<Translator>,
    mut texts: Query<&mut Text>,
) {
    let relocalised = translator.as_ref().is_some_and(Translator::changed);
    let translator = translator.as_ref();
    for (parts, value, range) in &sliders {
        let fresh = parts.is_added() || relocalised;
        if (fresh || value.is_changed())
            && let Some(readout) = parts.readout
        {
            write_text(
                &mut texts,
                readout,
                slider_text(parts.format, value.0, translator),
            );
        }
        if (fresh || range.is_changed())
            && let Some((start, end)) = parts.bounds
        {
            write_text(
                &mut texts,
                start,
                slider_text(parts.format, range.start(), translator),
            );
            write_text(
                &mut texts,
                end,
                slider_text(parts.format, range.end(), translator),
            );
        }
    }
}

/// Set `entity`'s text to `wanted`, only when it differs — a text write
/// re-measures the node.
fn write_text(texts: &mut Query<&mut Text>, entity: Entity, wanted: String) {
    if let Ok(mut text) = texts.get_mut(entity)
        && text.0 != wanted
    {
        text.0 = wanted;
    }
}

/// The step tooltip's two entities, spawned once under the UI root the first
/// time it is wanted.
#[derive(Resource, Debug, Clone, Copy)]
struct SliderStepTooltip {
    /// The skinned box, shown while a labelled track is hovered.
    panel: Entity,
    /// Its text: the hovered slider's step.
    text: Entity,
}

/// Spawn the step tooltip, hidden, once there is a UI root to hang it on.
fn spawn_slider_step_tooltip(root: Option<Res<UiRoot>>, mut commands: Commands) {
    let Some(root) = root else {
        return;
    };
    let panel = commands
        .spawn((
            tooltip_box(),
            Visibility::Hidden,
            Name::new("slider-step-tooltip"),
            ChildOf(root.0),
        ))
        .id();
    let text = commands
        .spawn((
            Text::default(),
            UiFont::Sans.at(TOOLTIP_FONT_SIZE),
            tooltip_text(),
            Pickable::IGNORE,
            ChildOf(panel),
        ))
        .id();
    commands.insert_resource(SliderStepTooltip { panel, text });
}

/// Show what one notch of the hovered slider moves it by, by the pointer.
///
/// The step is the one number of the three that has no place on the slider
/// itself — the readout is the value and the ends are the range — and it only
/// matters to someone about to press an arrow key or wondering why a drag
/// lands where it does, which is someone with the pointer on the track.
fn show_slider_step_tooltip(
    sliders: Query<(&Hovered, &SliderStep, &SliderLabelParts)>,
    tooltip: Res<SliderStepTooltip>,
    windows: Query<&Window, With<PrimaryWindow>>,
    translator: Option<Translator>,
    mut panels: Query<(&mut Node, &mut Visibility)>,
    mut texts: Query<&mut Text>,
) {
    let Ok((mut node, mut visibility)) = panels.get_mut(tooltip.panel) else {
        return;
    };
    let hovered = sliders.iter().find(|(hovered, ..)| hovered.get());
    let cursor = windows.single().ok().and_then(Window::cursor_position);
    let (Some((_, step, parts)), Some(cursor)) = (hovered, cursor) else {
        visibility.set_if_neq(Visibility::Hidden);
        return;
    };
    visibility.set_if_neq(Visibility::Inherited);
    let left = Val::Px(cursor.x + TOOLTIP_CURSOR_OFFSET.x);
    let top = Val::Px(cursor.y + TOOLTIP_CURSOR_OFFSET.y);
    if node.left != left {
        node.left = left;
    }
    if node.top != top {
        node.top = top;
    }
    let translator = translator.as_ref();
    let shown = slider_text(step_format(parts.format, step.0), step.0, translator);
    let wanted = translator.map_or_else(
        || format!("Step: {shown}"),
        |translator| translator.format("slider-step", &TransArgs::new().text("step", &shown)),
    );
    write_text(&mut texts, tooltip.text, wanted);
}

/// How a slider's step is written: in the slider's own format, with as many
/// decimals as the step needs if that is more than the value shows — an
/// environment knob read at two decimals still steps by a hundredth of its
/// range, and `0.00` would be no answer.
fn step_format(format: SliderFormat, step: f32) -> SliderFormat {
    match format {
        SliderFormat::Decimal(decimals) => SliderFormat::Decimal(decimals.max(step_decimals(step))),
        SliderFormat::Percent => SliderFormat::Percent,
    }
}

/// Keeps every live slider's thumb in step with its value.
///
/// Idempotent to add, like the other widget plugins here: a panel that draws a
/// slider adds it and the first one to arrive wins, because the viewer adds its
/// windows one at a time and no plugin group owns them all.
#[derive(Debug, Clone, Copy, Default)]
pub struct SliderWidgetPlugin;

impl Plugin for SliderWidgetPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            PostUpdate,
            (place_slider_thumbs, label_sliders).before(resolve_logical_boxes),
        )
        .add_systems(
            Update,
            (
                spawn_slider_step_tooltip.run_if(not(resource_exists::<SliderStepTooltip>)),
                show_slider_step_tooltip.run_if(resource_exists::<SliderStepTooltip>),
            )
                .chain(),
        );
    }
}

#[cfg(test)]
mod tests {
    //! Two things a layout sweep cannot tell you about this widget: that the
    //! thumb has a *size* at all, and that it is where the value says.
    //!
    //! The first matters because the thumb is deliberately given no height —
    //! a stretch that failed to stretch would be a zero-height node, invisible
    //! and passing every overflow check in the harness by being nothing.

    use super::{
        SLIDER_CLASS, SLIDER_THUMB_CLASS, SliderFormat, SliderLabelParts, SliderLabels,
        SliderStyle, SliderWidgetPlugin, slider_text, spawn_slider, step_decimals,
    };
    use bevy::prelude::*;
    use bevy::ui_widgets::{Slider, SliderRange, SliderStep, SliderValue, TrackClick};
    use bevy_flair::style::components::ClassList;
    use pretty_assertions::assert_eq;
    use sl_viewer_testkit::interact::{self, InteractionTest};
    use sl_viewer_testkit::{LayoutTest, TestError, settle, spawn_under_root};
    use sl_viewer_ui_core::ui::UiDirection;

    /// A slider with a 2 px border, so the interior is 4 px shorter than the
    /// track and the difference is visible in a whole number of pixels.
    const STYLE: SliderStyle = SliderStyle {
        track_width: 200.0,
        track_height: 16.0,
        border: 2.0,
        border_color: Color::WHITE,
        track_fill: Color::BLACK,
        thumb_width: 10.0,
        thumb_fill: Color::WHITE,
        font_size: 12.0,
    };

    /// Spawn one slider at `value` over 0…100 and settle. Gives back the track
    /// and its thumb.
    fn laid_out(
        app: &mut App,
        value: f32,
        with_plugin: bool,
    ) -> Result<(Entity, Entity), TestError> {
        if with_plugin {
            app.add_plugins(SliderWidgetPlugin);
        }
        let row = spawn_under_root(app, (Node::default(), Name::new("row")));
        let mut commands = app.world_mut().commands();
        let track = spawn_slider(
            &mut commands,
            row,
            STYLE,
            0,
            0.0,
            SliderLabels::beside(SliderFormat::Decimal(0)),
            (
                Slider::default(),
                SliderValue(value),
                SliderRange::new(0.0, 100.0),
                SliderStep(5.0),
            ),
        );
        app.world_mut().flush();
        settle(app);
        let thumb = *app
            .world()
            .entity(track)
            .get::<Children>()
            .and_then(|children| children.first())
            .ok_or("the track has no thumb")?;
        Ok((track, thumb))
    }

    /// The thumb fills the track's **interior** — the box inside its border —
    /// rather than being the track's own height, or nothing at all.
    #[test]
    fn a_thumb_fills_its_track_interior() -> Result<(), TestError> {
        let mut app = LayoutTest::new().build();
        let (track, thumb) = laid_out(&mut app, 0.0, false)?;
        let interior = app
            .world()
            .entity(track)
            .get::<ComputedNode>()
            .ok_or("the track has no layout")?
            .content_box()
            .size();
        let thumb_box = app
            .world()
            .entity(thumb)
            .get::<ComputedNode>()
            .ok_or("the thumb has no layout")?
            .size;
        // Bit-exact: the thumb is *stretched* to the interior, so it is that
        // number or the stretch did not happen.
        assert_eq!(
            thumb_box.y.to_bits(),
            interior.y.to_bits(),
            "the thumb is {} px tall in a {} px interior — a stretch that did not \
             stretch is an invisible widget every layout check would pass",
            thumb_box.y,
            interior.y,
        );
        assert_eq!(
            thumb_box.x.to_bits(),
            STYLE.thumb_width.to_bits(),
            "the thumb is {} px wide, not the {} it asked for",
            thumb_box.x,
            STYLE.thumb_width,
        );
        Ok(())
    }

    /// With the plugin, the thumb moves to where the value sits in its range.
    #[test]
    fn a_thumb_sits_where_the_value_says() -> Result<(), TestError> {
        let mut app = LayoutTest::new().build();
        let (_, thumb) = laid_out(&mut app, 25.0, true)?;
        let node = app
            .world()
            .entity(thumb)
            .get::<Node>()
            .ok_or("the thumb has no node")?;
        assert_eq!(
            node.left,
            Val::Px(0.25 * STYLE.travel()),
            "a quarter of the way along a 0…100 range is a quarter of the travel",
        );
        Ok(())
    }

    /// **The skin can reach a slider.** The track and the thumb each carry
    /// their class, so `common.css` paints them from tokens; before this every
    /// slider painted the colours its panel picked, and no skin could restyle
    /// one.
    #[test]
    fn a_slider_carries_the_classes_the_skin_paints() -> Result<(), TestError> {
        let mut app = LayoutTest::new().build();
        let (track, thumb) = laid_out(&mut app, 0.0, false)?;
        for (node, class) in [(track, SLIDER_CLASS), (thumb, SLIDER_THUMB_CLASS)] {
            assert!(
                app.world()
                    .entity(node)
                    .get::<ClassList>()
                    .is_some_and(|classes| classes.contains(class)),
                "a slider node without `{class}` is one no skin can paint"
            );
        }
        Ok(())
    }

    /// **A live slider moves under the pointer.** A drag from the thumb at the
    /// start of the track to three quarters along it carries the value there,
    /// through the real input and picking stack.
    #[test]
    fn a_drag_along_the_track_moves_the_value() -> Result<(), TestError> {
        let mut app = InteractionTest::new().build();
        let (track, _thumb) = laid_out(&mut app, 0.0, true)?;
        // The headless slider only announces a move; its owner writes the
        // value back, as `settings_binding`'s observer does.
        app.world_mut()
            .entity_mut(track)
            .insert(Name::new("slider-under-test"))
            .observe(
                |change: On<bevy::ui_widgets::ValueChange<f32>>, mut commands: Commands| {
                    commands
                        .entity(change.source)
                        .insert(SliderValue(change.value));
                },
            );
        settle(&mut app);
        let centre = interact::centre_of(&mut app, "slider-under-test")
            .ok_or("the track has no position")?;
        let from = centre - Vec2::new(STYLE.track_width / 2.0 - STYLE.thumb_width / 2.0, 0.0);
        let to = centre + Vec2::new(STYLE.track_width / 4.0, 0.0);
        interact::drag(&mut app, from, to, 8, MouseButton::Left);
        settle(&mut app);
        let value = app
            .world()
            .get::<SliderValue>(track)
            .map(|value| value.0)
            .ok_or("no value")?;
        assert!(
            value > 50.0,
            "a drag three quarters along the track left the value at {value}"
        );
        Ok(())
    }

    /// An interactive app in `direction` holding one slider at `value` over
    /// 0…100 that clicks its track as `track_click` says, and writes each
    /// value it announces back, as `settings_binding`'s observer does. Gives
    /// back the app, the track and its thumb.
    fn live_slider(
        direction: UiDirection,
        value: f32,
        track_click: TrackClick,
    ) -> Result<(App, Entity, Entity), TestError> {
        let mut app = InteractionTest::over(LayoutTest::new().with_direction(direction)).build();
        app.add_plugins(SliderWidgetPlugin);
        let row = spawn_under_root(&mut app, (Node::default(), Name::new("row")));
        let mut commands = app.world_mut().commands();
        let track = spawn_slider(
            &mut commands,
            row,
            STYLE,
            0,
            0.0,
            SliderLabels::beside(SliderFormat::Decimal(0)),
            (
                Slider {
                    track_click,
                    ..Slider::default()
                },
                SliderValue(value),
                SliderRange::new(0.0, 100.0),
                SliderStep(5.0),
            ),
        );
        app.world_mut().flush();
        app.world_mut()
            .entity_mut(track)
            .insert(Name::new("slider-under-test"))
            .observe(
                |change: On<bevy::ui_widgets::ValueChange<f32>>, mut commands: Commands| {
                    commands
                        .entity(change.source)
                        .insert(SliderValue(change.value));
                },
            );
        settle(&mut app);
        let thumb = *app
            .world()
            .entity(track)
            .get::<Children>()
            .and_then(|children| children.first())
            .ok_or("the track has no thumb")?;
        Ok((app, track, thumb))
    }

    /// The slider's current value.
    fn value_of(app: &App, track: Entity) -> Result<f32, TestError> {
        Ok(app.world().get::<SliderValue>(track).ok_or("no value")?.0)
    }

    /// **A drag moves the thumb with the hand, in both directions.** The thumb
    /// starts at the minimum — the left end, or the right end under a
    /// right-to-left layout — and a drag from it three quarters of the way
    /// toward the other end carries the value past the middle. Before the core
    /// slider read its node's direction, a leftward drag under `Rtl` lowered
    /// the value and the thumb ran away from the pointer.
    #[test]
    fn a_drag_carries_the_value_toward_the_hand_in_either_direction() -> Result<(), TestError> {
        for direction in [UiDirection::Ltr, UiDirection::Rtl] {
            let (mut app, track, thumb) = live_slider(direction, 0.0, TrackClick::Drag)?;
            let from =
                interact::centre_of_entity(&app, thumb).ok_or("the thumb has no position")?;
            let toward_max = if direction.is_rtl() { -1.0 } else { 1.0 };
            let to = from + Vec2::new(toward_max * 0.75 * STYLE.travel(), 0.0);
            interact::drag(&mut app, from, to, 8, MouseButton::Left);
            settle(&mut app);
            let value = value_of(&app, track)?;
            assert!(
                (70.0..=80.0).contains(&value),
                "{direction:?}: a drag three quarters of the travel toward the maximum left \
                 the value at {value}"
            );
        }
        Ok(())
    }

    /// **A track click lands on the value under the pointer, in both
    /// directions.** A snapping click a quarter of the way along the track from
    /// its right end sets the value whose thumb is drawn there: three quarters
    /// left to right, a quarter right to left — and the thumb then sits under
    /// the pointer.
    #[test]
    fn a_track_click_sets_the_value_drawn_under_the_pointer() -> Result<(), TestError> {
        for (direction, expected) in [(UiDirection::Ltr, 75.0), (UiDirection::Rtl, 25.0)] {
            let (mut app, track, thumb) = live_slider(direction, 50.0, TrackClick::Snap)?;
            let centre = interact::centre_of(&mut app, "slider-under-test")
                .ok_or("the track has no position")?;
            let at = centre + Vec2::new(0.25 * STYLE.travel(), 0.0);
            interact::click(&mut app, at, MouseButton::Left);
            settle(&mut app);
            let value = value_of(&app, track)?;
            assert!(
                (value - expected).abs() < 1.0,
                "{direction:?}: a click a quarter of the travel right of centre set {value}, \
                 not {expected}"
            );
            let drawn =
                interact::centre_of_entity(&app, thumb).ok_or("the thumb has no position")?;
            // Within a pixel: the thumb travels inside the track's border and
            // the core slider measures the same box. Measuring the border box
            // put the thumb one border width toward the trailing end
            // (viewer-slider-thumb-offset-by-track-border).
            assert!(
                (drawn.x - at.x).abs() <= 1.0,
                "{direction:?}: the thumb is drawn at x = {}, the click was at x = {}",
                drawn.x,
                at.x,
            );
        }
        Ok(())
    }

    /// **A thumb stays inside its track's border at either end, in both
    /// directions.** At the minimum and the maximum the thumb's edge meets the
    /// inner edge of the border on that side, never covering it.
    #[test]
    fn a_thumb_never_overhangs_its_track_border() -> Result<(), TestError> {
        for direction in [UiDirection::Ltr, UiDirection::Rtl] {
            for value in [0.0, 100.0] {
                let (mut app, _track, thumb) = live_slider(direction, value, TrackClick::Snap)?;
                let centre = interact::centre_of(&mut app, "slider-under-test")
                    .ok_or("the track has no position")?;
                let drawn =
                    interact::centre_of_entity(&app, thumb).ok_or("the thumb has no position")?;
                let inner_half = STYLE.track_width / 2.0 - STYLE.border;
                let (thumb_left, thumb_right) = (
                    drawn.x - STYLE.thumb_width / 2.0,
                    drawn.x + STYLE.thumb_width / 2.0,
                );
                let at_right_end = (value > 50.0) != direction.is_rtl();
                let (edge, wanted) = if at_right_end {
                    (thumb_right, centre.x + inner_half)
                } else {
                    (thumb_left, centre.x - inner_half)
                };
                assert!(
                    (edge - wanted).abs() <= 0.5,
                    "{direction:?} at {value}: the thumb spans x = {thumb_left}…{thumb_right}, \
                     its edge belongs at the border's inner edge x = {wanted}",
                );
            }
        }
        Ok(())
    }

    /// **The arrow keys step toward the side they point at.** `ArrowRight`
    /// raises a left-to-right slider and lowers a right-to-left one, whose
    /// maximum is on the left; `ArrowLeft` the other way round.
    #[test]
    fn the_arrow_keys_follow_the_thumb_in_either_direction() -> Result<(), TestError> {
        use bevy::input::keyboard::Key;
        for (direction, right_step) in [(UiDirection::Ltr, 5.0), (UiDirection::Rtl, -5.0)] {
            let (mut app, track, _thumb) = live_slider(direction, 50.0, TrackClick::Drag)?;
            interact::focus(&mut app, track);
            interact::tap(&mut app, KeyCode::ArrowRight, Key::ArrowRight);
            settle(&mut app);
            // Bit-exact: a step of 5 from 50 is exact in `f32`.
            assert_eq!(
                value_of(&app, track)?.to_bits(),
                (50.0_f32 + right_step).to_bits(),
                "{direction:?}: ArrowRight"
            );
            interact::tap(&mut app, KeyCode::ArrowLeft, Key::ArrowLeft);
            interact::tap(&mut app, KeyCode::ArrowLeft, Key::ArrowLeft);
            settle(&mut app);
            // Bit-exact: a step of 5 from 50 is exact in `f32`.
            assert_eq!(
                value_of(&app, track)?.to_bits(),
                (50.0_f32 - right_step).to_bits(),
                "{direction:?}: ArrowLeft twice"
            );
        }
        Ok(())
    }

    /// The text of one of a slider's number nodes.
    fn text_of(app: &App, entity: Option<Entity>) -> Result<String, TestError> {
        let entity = entity.ok_or("the slider has no such label")?;
        Ok(app
            .world()
            .get::<Text>(entity)
            .ok_or("the label has no text")?
            .0
            .clone())
    }

    /// **A slider says what it holds, and how far it goes** — the readout is
    /// the value and the two ends are the range, written the same way.
    #[test]
    fn a_slider_shows_its_value_and_its_bounds() -> Result<(), TestError> {
        let mut app = LayoutTest::new().build();
        let (track, _) = laid_out(&mut app, 25.0, true)?;
        let parts = *app
            .world()
            .get::<SliderLabelParts>(track)
            .ok_or("the slider spawned no labels")?;
        let (start, end) = parts.bounds.ok_or("the slider drew no bounds")?;
        assert_eq!(text_of(&app, parts.readout)?, "25");
        assert_eq!(text_of(&app, Some(start))?, "0");
        assert_eq!(text_of(&app, Some(end))?, "100");

        // A value written from outside — a setting reloaded, a trackball —
        // reaches the readout the same frame it reaches the thumb.
        app.world_mut().entity_mut(track).insert(SliderValue(70.0));
        settle(&mut app);
        assert_eq!(text_of(&app, parts.readout)?, "70");

        // And a range that moves — the media scrubber's clip length — moves
        // the end that names it.
        app.world_mut()
            .entity_mut(track)
            .insert(SliderRange::new(0.0, 250.0));
        settle(&mut app);
        assert_eq!(text_of(&app, Some(end))?, "250");
        Ok(())
    }

    /// **The numbers have a size.** A readout or a bound that laid out to
    /// nothing would pass every overflow check by being invisible.
    #[test]
    fn a_slider_number_is_laid_out_with_a_size() -> Result<(), TestError> {
        let mut app = LayoutTest::new().build();
        let (track, _) = laid_out(&mut app, 25.0, true)?;
        let parts = *app
            .world()
            .get::<SliderLabelParts>(track)
            .ok_or("the slider spawned no labels")?;
        let (start, end) = parts.bounds.ok_or("the slider drew no bounds")?;
        for (what, entity) in [
            ("readout", parts.readout.ok_or("no readout")?),
            ("start", start),
            ("end", end),
        ] {
            let size = app
                .world()
                .get::<ComputedNode>(entity)
                .ok_or("a label has no layout")?
                .size;
            assert!(
                size.x > 0.0 && size.y > 0.0,
                "the {what} label laid out at {size}"
            );
        }
        Ok(())
    }

    /// A bare slider (the media scrubber) gains no group and no labels: its
    /// track is still its parent's own child.
    #[test]
    fn an_unlabelled_slider_is_just_its_track() -> Result<(), TestError> {
        let mut app = LayoutTest::new().build();
        let row = spawn_under_root(&mut app, (Node::default(), Name::new("row")));
        let mut commands = app.world_mut().commands();
        let track = spawn_slider(
            &mut commands,
            row,
            STYLE,
            0,
            0.0,
            SliderLabels::None,
            Slider::default(),
        );
        app.world_mut().flush();
        settle(&mut app);
        assert_eq!(
            app.world().get::<ChildOf>(track).map(ChildOf::parent),
            Some(row)
        );
        assert!(app.world().get::<SliderLabelParts>(track).is_none());
        Ok(())
    }

    /// The decimals a step needs: whole steps none, and a float step its own
    /// count despite `f32` not holding a hundredth exactly.
    #[test]
    fn a_step_names_the_decimals_it_needs() {
        for (step, decimals) in [
            (1.0, 0),
            (8.0, 0),
            (256.0, 0),
            (0.5, 1),
            (0.1, 1),
            (0.05, 2),
            (0.01, 2),
            (0.035, 3),
            (0.000_001, 6),
            (0.0, 0),
            (f32::NAN, 0),
        ] {
            assert_eq!(step_decimals(step), decimals, "step {step}");
        }
        // A step finer than any readout is capped, not looped on.
        assert_eq!(step_decimals(1.0e-12), super::MAX_DECIMALS);
    }

    /// The three shapes a number takes, headless (the English rendering).
    #[test]
    fn a_number_is_written_as_its_format_says() {
        assert_eq!(slider_text(SliderFormat::Decimal(0), 3.6, None), "4");
        assert_eq!(slider_text(SliderFormat::Decimal(2), 0.7, None), "0.70");
        assert_eq!(slider_text(SliderFormat::Percent, 0.35, None), "35%");
        // A value a hair under zero is zero, not "-0.00".
        assert_eq!(slider_text(SliderFormat::Decimal(2), -0.001, None), "0.00");
        assert_eq!(slider_text(SliderFormat::Decimal(1), -2.5, None), "-2.5");
    }

    /// **Hovering the track shows its step.** One notch is the only number
    /// with no place on the slider itself.
    #[test]
    fn hovering_a_slider_shows_its_step() -> Result<(), TestError> {
        use sl_viewer_testkit::interact::{self, InteractionTest};
        let mut app = InteractionTest::new().build();
        let (track, _) = laid_out(&mut app, 0.0, true)?;
        app.world_mut()
            .entity_mut(track)
            .insert(Name::new("slider-under-test"));
        settle(&mut app);
        interact::hover_node(&mut app, "slider-under-test")?;
        settle(&mut app);
        let tip = app
            .world_mut()
            .query::<(&Name, &Visibility, &Children)>()
            .iter(app.world())
            .find(|(name, ..)| name.as_str() == "slider-step-tooltip")
            .map(|(_, visibility, children)| (*visibility, children.first().copied()))
            .ok_or("hovering spawned no tooltip")?;
        assert_eq!(tip.0, Visibility::Inherited, "the tooltip is hidden");
        assert_eq!(text_of(&app, tip.1)?, "Step: 5");
        Ok(())
    }
}
