//! The controls an environment editor is built out of: a labelled slider, a
//! colour swatch, a texture swatch, an action button.
//!
//! Every window in this crate draws the same [`knobs`](crate::knobs) with the
//! same widgets, so the widgets are spawned from here and only the *wiring*
//! differs per window: which knob a control drives and which buffer it writes
//! are the window's business, the geometry and the thumb-following are not.
//!
//! [`SliderRow`] is the piece that makes that split work. A slider's thumb
//! position and its value readout are the same job in every window — read
//! `SliderValue`, place the thumb, format the number — so one system
//! ([`sync_slider_rows`]) does it for all of them, and a new window gets it
//! without a line of code. What a window keeps for itself is the component
//! naming its own knob, which is what its write-back and its re-seed query on.

use bevy::input_focus::tab_navigation::TabIndex;
use bevy::prelude::*;
use bevy::ui_widgets::{Slider, SliderRange, SliderStep, SliderThumb, SliderValue};
use sl_client_bevy::TextureKey;
use sl_viewer_pickers::ui_texture_picker::spawn_texture_swatch;
use sl_viewer_ui_core::i18n::Translated;
use sl_viewer_ui_core::ui::{LogicalInset, LogicalRect, column, row};
use sl_viewer_ui_core::ui_font::UiFont;
use sl_viewer_ui_widgets::ui_color_picker::spawn_color_swatch;

use crate::knobs::{ColorKnob, TextureKnob, label_key};
use crate::style::{
    ACTION_BACKGROUND, CONTROL_BORDER, DIM_LABEL_COLOR, FONT_SIZE, LABEL_COLOR, THUMB_FILL,
    TRACK_FILL,
};

/// A slider track's width, logical px — and, with it, a column's.
///
/// The label sits **above** the slider rather than beside it, as the reference
/// lays these windows out. Beside it, a column is label + track + readout wide
/// and four of them do not fit a window anyone would open; above it, four
/// columns are a normal floater wide and the labels have room to say what they
/// mean.
pub const TRACK_WIDTH: f32 = 140.0;

/// A slider track's height, logical px.
const TRACK_HEIGHT: f32 = 12.0;

/// A slider thumb's width, logical px.
const THUMB_WIDTH: f32 = 9.0;

/// A value readout's width, logical px — right of the label, on its line.
const READOUT_WIDTH: f32 = 44.0;

/// A slider that shows its value: the readout beside its label.
///
/// Carried by every environment slider whatever window it is in, so one system
/// keeps every thumb and every number in step.
#[derive(Component, Debug, Clone, Copy)]
pub struct SliderRow {
    /// The `Text` entity showing the value.
    pub readout: Entity,
    /// How many decimals the readout shows.
    ///
    /// Per row rather than one format for the widget, because the environment's
    /// knobs are not one scale: a haze density reads at two decimals and a
    /// Rayleigh linear term is `0.00` at every position of its slider unless it
    /// gets eight.
    pub decimals: usize,
}

/// One labelled control: a caption line, then the control under it. Returns the
/// node the control is parented into, and the caption row a readout can join.
pub fn spawn_labelled_block(
    commands: &mut Commands,
    parent: Entity,
    label_key: String,
) -> (Entity, Entity) {
    let block = commands
        .spawn((
            Node {
                width: Val::Px(TRACK_WIDTH),
                ..column(Val::Px(1.0))
            },
            ChildOf(parent),
        ))
        .id();
    let caption = commands
        .spawn((
            Node {
                width: Val::Percent(100.0),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::SpaceBetween,
                ..row(Val::Px(4.0))
            },
            ChildOf(block),
        ))
        .id();
    commands.spawn((
        Text::new(String::new()),
        TextLayout {
            linebreak: LineBreak::NoWrap,
            ..Default::default()
        },
        UiFont::Sans.at(FONT_SIZE),
        TextColor(LABEL_COLOR),
        Translated::new(label_key),
        ChildOf(caption),
    ));
    (block, caption)
}

/// A labelled slider over `range`, named `{element}-{slug}:slider`. Returns the
/// track, which the caller tags with the knob it drives and an observer.
pub fn spawn_slider(
    commands: &mut Commands,
    parent: Entity,
    element: &str,
    slug: &str,
    range: (f32, f32),
    decimals: usize,
    tab: &mut i32,
) -> Entity {
    let (block, caption) = spawn_labelled_block(commands, parent, label_key(slug));
    let (min, max) = range;
    let readout = commands
        .spawn((
            Text::new(String::new()),
            TextLayout {
                linebreak: LineBreak::NoWrap,
                ..Default::default()
            },
            UiFont::Sans.at(FONT_SIZE),
            TextColor(DIM_LABEL_COLOR),
            Node {
                min_width: Val::Px(READOUT_WIDTH),
                justify_content: JustifyContent::End,
                ..Default::default()
            },
            ChildOf(caption),
        ))
        .id();
    let track = commands
        .spawn((
            Slider::default(),
            SliderValue(min),
            SliderRange::new(min, max),
            SliderStep((max - min) / 100.0),
            SliderRow { readout, decimals },
            Node {
                width: Val::Px(TRACK_WIDTH),
                height: Val::Px(TRACK_HEIGHT),
                border: UiRect::all(Val::Px(1.0)),
                ..Default::default()
            },
            BorderColor::all(CONTROL_BORDER),
            BackgroundColor(TRACK_FILL),
            TabIndex(*tab),
            Name::new(format!("{element}-{slug}:slider")),
            ChildOf(block),
        ))
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
    *tab = tab.saturating_add(1);
    track
}

/// A labelled colour swatch. Returns the swatch, for the caller's knob tag.
pub fn spawn_color_row(
    commands: &mut Commands,
    parent: Entity,
    element: &str,
    knob: ColorKnob,
    tab: &mut i32,
) -> Entity {
    let (block, _caption) = spawn_labelled_block(commands, parent, label_key(knob.slug()));
    let name = format!("{element}-{}", knob.slug());
    let swatch = spawn_color_swatch(commands, block, &name, *tab, Color::BLACK);
    *tab = tab.saturating_add(1);
    swatch
}

/// A labelled texture-picker swatch, opening on the built-in default its field
/// means when it holds nothing. Returns the swatch, for the caller's knob tag.
pub fn spawn_texture_row(
    commands: &mut Commands,
    parent: Entity,
    element: &str,
    knob: TextureKnob,
    tab: &mut i32,
) -> Entity {
    let (block, _caption) = spawn_labelled_block(commands, parent, label_key(knob.slug()));
    let name = format!("{element}-{}", knob.slug());
    let swatch = spawn_texture_swatch(
        commands,
        block,
        &name,
        *tab,
        TextureKey::from(knob.default_texture()),
    );
    *tab = tab.saturating_add(1);
    swatch
}

/// An action button with a translated label, named `{element}-{slug}:button`.
/// Returns it, for the caller's marker component and observer.
pub fn spawn_action_button(
    commands: &mut Commands,
    parent: Entity,
    element: &str,
    slug: &str,
    label_key: String,
    tab: &mut i32,
) -> Entity {
    let button = commands
        .spawn((
            Button,
            TabIndex(*tab),
            Node {
                padding: UiRect::axes(Val::Px(10.0), Val::Px(4.0)),
                align_self: AlignSelf::FlexStart,
                ..Default::default()
            },
            BackgroundColor(ACTION_BACKGROUND),
            Name::new(format!("{element}-{slug}:button")),
            ChildOf(parent),
        ))
        .with_child((
            Text::new(String::new()),
            UiFont::Sans.at(FONT_SIZE),
            TextColor(LABEL_COLOR),
            Translated::new(label_key),
            Pickable::IGNORE,
        ))
        .id();
    *tab = tab.saturating_add(1);
    button
}

/// The one system every environment window's sliders share.
///
/// Added by each window's plugin rather than by a plugin group, because the
/// viewer adds those windows individually — so each one guards with
/// `is_plugin_added` and the first to build it wins.
#[derive(Debug, Clone, Copy, Default)]
pub struct RowsPlugin;

impl Plugin for RowsPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, sync_slider_rows);
    }
}

/// Keep every environment slider's thumb and readout in step with its value —
/// the one job that is the same in every window that draws one.
pub fn sync_slider_rows(
    sliders: Query<(&SliderRow, &SliderValue, &SliderRange, &Children)>,
    mut insets: Query<&mut LogicalInset, With<SliderThumb>>,
    mut texts: Query<&mut Text>,
) {
    for (row_info, value, range, children) in &sliders {
        place_thumb(value, range, children, &mut insets);
        write_readout(row_info.readout, value.0, row_info.decimals, &mut texts);
    }
}

/// Move a slider's thumb to where its value sits in its range.
fn place_thumb(
    value: &SliderValue,
    range: &SliderRange,
    children: &Children,
    insets: &mut Query<&mut LogicalInset, With<SliderThumb>>,
) {
    let span = range.span();
    let fraction = if span > f32::EPSILON {
        ((value.0 - range.start()) / span).clamp(0.0, 1.0)
    } else {
        0.0
    };
    let offset = fraction * (TRACK_WIDTH - THUMB_WIDTH);
    for child in children.iter() {
        if let Ok(mut inset) = insets.get_mut(child) {
            inset.0.inline_start = Val::Px(offset);
        }
    }
}

/// Write a slider's value into its readout, only when the text would change.
fn write_readout(readout: Entity, value: f32, decimals: usize, texts: &mut Query<&mut Text>) {
    if let Ok(mut text) = texts.get_mut(readout) {
        let wanted = format!("{value:.decimals$}");
        if text.0 != wanted {
            text.0 = wanted;
        }
    }
}
