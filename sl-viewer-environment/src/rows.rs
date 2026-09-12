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
use sl_viewer_ui_widgets::ui_trackball::{TrackballAim, TrackballPlugin, spawn_trackball};

use crate::knobs::{AimKnobs, ColorKnob, SkyKnob, TextureKnob, label_key};
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

// ---------------------------------------------------------------------------
// The trackball, and the two sliders it shares a body with.
// ---------------------------------------------------------------------------

/// A trackball in an environment window, and which body's knobs it writes.
///
/// The `scope` is the window's element prefix. Three windows draw a sun and a
/// moon trackball, and every one of them is in the same world at once — so a
/// control only ever drives a control that names the same window.
#[derive(Component, Debug, Clone, Copy)]
pub struct AimTrackball {
    /// The window's element prefix.
    pub scope: &'static str,
    /// The pair the control writes.
    pub knobs: AimKnobs,
}

/// One of the two sliders a trackball shares a body with.
#[derive(Component, Debug, Clone, Copy)]
pub struct AimSlider {
    /// The window's element prefix.
    pub scope: &'static str,
    /// Which of the pair this slider is.
    pub knob: SkyKnob,
    /// The pair it belongs to.
    pub knobs: AimKnobs,
}

/// Tag a freshly spawned sky slider as one half of a body's aim, if its knob is
/// one — which is what puts it in step with the trackball above it.
///
/// Called by every window's slider spawner rather than by [`spawn_slider`]
/// itself, because that one takes a slug and a range and deliberately knows
/// nothing about knobs.
pub fn tag_aim_slider(commands: &mut Commands, slider: Entity, scope: &'static str, knob: SkyKnob) {
    if let Some(knobs) = AimKnobs::of(knob) {
        commands
            .entity(slider)
            .insert(AimSlider { scope, knob, knobs });
    }
}

/// A labelled trackball for one body. Returns it, for the caller's observer —
/// what the control writes into is the window's business, exactly as a slider's
/// is.
pub fn spawn_trackball_row(
    commands: &mut Commands,
    parent: Entity,
    element: &'static str,
    knobs: AimKnobs,
    tab: &mut i32,
) -> Entity {
    let (block, _caption) = spawn_labelled_block(commands, parent, label_key(knobs.slug()));
    let trackball = spawn_trackball(
        commands,
        block,
        element,
        knobs.body,
        *tab,
        TrackballAim::ZENITH,
    );
    commands.entity(trackball).insert(AimTrackball {
        scope: element,
        knobs,
    });
    *tab = tab.saturating_add(1);
    trackball
}

/// A slider moved: aim the trackball beside it the same way.
///
/// The value travels straight from the widget rather than back out of the sky
/// it was just written into, and that is the whole point: a body sitting
/// exactly at a pole has no azimuth stored, so a re-read would hand the compass
/// back a zero the user did not type.
fn aim_sliders_drive_trackballs(
    sliders: Query<(&AimSlider, &SliderValue), Changed<SliderValue>>,
    mut trackballs: Query<(&AimTrackball, &mut TrackballAim)>,
) {
    for (slider, value) in &sliders {
        for (trackball, mut aim) in &mut trackballs {
            if trackball.scope != slider.scope || trackball.knobs != slider.knobs {
                continue;
            }
            let wanted = slider.knobs.with(*aim, slider.knob, value.0);
            if *aim != wanted {
                *aim = wanted;
            }
        }
    }
}

/// The trackball moved: put its two sliders on the same two angles.
///
/// The pair settles in one further frame — the inserts below mark the sliders
/// changed, [`aim_sliders_drive_trackballs`] reads them back and finds the
/// trackball already holding what they say, and writes nothing.
fn trackballs_drive_aim_sliders(
    trackballs: Query<(&AimTrackball, &TrackballAim), Changed<TrackballAim>>,
    sliders: Query<(Entity, &AimSlider, &SliderRange, &SliderValue)>,
    mut commands: Commands,
) {
    for (trackball, aim) in &trackballs {
        for (entity, slider, range, value) in &sliders {
            if slider.scope != trackball.scope || slider.knobs != trackball.knobs {
                continue;
            }
            let Some(wanted) = trackball.knobs.value_of(*aim, slider.knob) else {
                continue;
            };
            let wanted = range.clamp(wanted);
            // `SliderValue` is immutable, so a new value is inserted rather than
            // assigned — and an insert marks the component changed whether or
            // not it carries a new number, which is what would keep the two
            // controls writing to each other forever.
            if value.0.to_bits() != wanted.to_bits() {
                commands.entity(entity).insert(SliderValue(wanted));
            }
        }
    }
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

/// What [`paint_action_button`] writes through: a button's own background, its
/// label's colour, and the filter saying whether it is already disabled — so a
/// button that already looks right is left alone rather than re-marked changed
/// every frame (which would give the translation sweep and the layout gate work
/// sixty times a second over a window where nothing moved).
pub type ButtonPaint<'w, 's> = (
    Query<'w, 's, &'static mut BackgroundColor>,
    Query<'w, 's, &'static mut TextColor>,
    Query<'w, 's, (), With<bevy::ui::InteractionDisabled>>,
);

/// Mark one action button enabled or disabled, in `background` / `label` when it
/// is and the caller's dim pair when it is not.
///
/// Bevy's `InteractionDisabled` is **advisory**: it stops a window's own press
/// observer (each one filters on it) and nothing paints it. So the colours are
/// written here beside it, and both halves are written only when they would
/// change.
///
/// Shared because three windows in this crate grey the same kind of button on
/// the same kind of predicate, and a copy per window is a place for the disabled
/// look — or worse, the `InteractionDisabled` half of it — to drift.
pub fn paint_action_button(
    commands: &mut Commands,
    children: &Query<&Children>,
    paint: &mut ButtonPaint,
    entity: Entity,
    enabled: bool,
    colours: (Color, Color),
) {
    let (backgrounds, texts, disabled) = paint;
    let (background, label) = colours;
    if let Ok(mut current) = backgrounds.get_mut(entity)
        && current.0 != background
    {
        current.0 = background;
    }
    if let Ok(kids) = children.get(entity) {
        for child in kids.iter() {
            if let Ok(mut colour) = texts.get_mut(child)
                && colour.0 != label
            {
                colour.0 = label;
            }
        }
    }
    if disabled.contains(entity) == enabled {
        if enabled {
            commands
                .entity(entity)
                .remove::<bevy::ui::InteractionDisabled>();
        } else {
            commands
                .entity(entity)
                .insert(bevy::ui::InteractionDisabled);
        }
    }
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
        if !app.is_plugin_added::<TrackballPlugin>() {
            app.add_plugins(TrackballPlugin);
        }
        app.add_systems(
            Update,
            // Ordered: a slider drag has to reach the trackball and the
            // trackball's own drag has to reach the sliders in the frame it
            // happened, and `sync_slider_rows` draws whatever the pair settled
            // on — an unordered tuple would leave one of the two a frame behind
            // the hand.
            (
                aim_sliders_drive_trackballs,
                trackballs_drive_aim_sliders,
                sync_slider_rows,
            )
                .chain(),
        );
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

#[cfg(test)]
mod tests {
    use bevy::prelude::*;
    use bevy::ui_widgets::{SliderRange, SliderValue};
    use pretty_assertions::assert_eq;

    use super::{AimSlider, AimTrackball, RowsPlugin};
    use crate::knobs::{AimKnobs, SkyKnob};
    use sl_viewer_ui_widgets::ui_trackball::TrackballAim;

    /// A boxed error so tests can use `?` instead of the disallowed
    /// `unwrap` / `expect`.
    type TestError = Box<dyn core::error::Error>;

    /// One window's element prefix.
    const WINDOW: &str = "one-window";

    /// A second window's, open at the same time — which is the ordinary case:
    /// the Personal Lighting floater, the sky editor and the day-cycle editor
    /// all draw a sun trackball, and all three can be on screen at once.
    const OTHER: &str = "another-window";

    /// An app carrying only the row systems — the pair's wiring needs no
    /// layout, and the sliders here are bare values rather than laid-out
    /// widgets.
    fn rows_app() -> App {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins).add_plugins(RowsPlugin);
        app
    }

    /// Spawn a bare aim slider for `knob` in `scope`, or `None` if `knob` is not
    /// one of the four angles a trackball drives — which is the lookup every
    /// window's slider spawner makes, so a test that asked for a knob outside it
    /// would be testing nothing.
    fn slider(app: &mut App, scope: &'static str, knob: SkyKnob, value: f32) -> Option<Entity> {
        let knobs = AimKnobs::of(knob)?;
        let (min, max) = knob.range();
        Some(
            app.world_mut()
                .spawn((
                    AimSlider { scope, knob, knobs },
                    SliderValue(value),
                    SliderRange::new(min, max),
                ))
                .id(),
        )
    }

    /// Spawn a bare trackball for `knobs` in `scope`.
    fn trackball(app: &mut App, scope: &'static str, knobs: AimKnobs, aim: TrackballAim) -> Entity {
        app.world_mut()
            .spawn((AimTrackball { scope, knobs }, aim))
            .id()
    }

    /// A slider's value.
    fn value_of(app: &App, slider: Entity) -> Option<f32> {
        app.world().get::<SliderValue>(slider).map(|value| value.0)
    }

    /// A trackball's aim.
    fn aim_of(app: &App, trackball: Entity) -> Option<TrackballAim> {
        app.world().get::<TrackballAim>(trackball).copied()
    }

    /// **A slider drag reaches the trackball beside it**, and moves only the
    /// angle it is.
    #[test]
    fn a_slider_aims_its_trackball() -> Result<(), TestError> {
        let mut app = rows_app();
        let ball = trackball(
            &mut app,
            WINDOW,
            AimKnobs::SUN,
            TrackballAim {
                azimuth: 10.0,
                elevation: 20.0,
            },
        );
        let azimuth =
            slider(&mut app, WINDOW, SkyKnob::SunAzimuth, 10.0).ok_or("not an aim knob")?;
        app.update();

        app.world_mut()
            .entity_mut(azimuth)
            .insert(SliderValue(250.0));
        app.update();

        assert_eq!(
            aim_of(&app, ball),
            Some(TrackballAim {
                azimuth: 250.0,
                elevation: 20.0,
            }),
            "the compass moved and the height did not"
        );
        Ok(())
    }

    /// **A trackball reaches both its sliders**, and each takes the angle it
    /// shows.
    #[test]
    fn a_trackball_drives_both_its_sliders() -> Result<(), TestError> {
        let mut app = rows_app();
        let ball = trackball(&mut app, WINDOW, AimKnobs::MOON, TrackballAim::ZENITH);
        let azimuth =
            slider(&mut app, WINDOW, SkyKnob::MoonAzimuth, 0.0).ok_or("not an aim knob")?;
        let elevation =
            slider(&mut app, WINDOW, SkyKnob::MoonElevation, 0.0).ok_or("not an aim knob")?;
        app.update();

        app.world_mut().entity_mut(ball).insert(TrackballAim {
            azimuth: 123.0,
            elevation: -45.0,
        });
        app.update();

        assert_eq!(value_of(&app, azimuth), Some(123.0));
        assert_eq!(value_of(&app, elevation), Some(-45.0));
        Ok(())
    }

    /// **A body's controls leave the other body's alone.** One column carries a
    /// sun trackball and a moon trackball and four sliders between them; the
    /// pairing is what keeps a sun drag off the moon.
    #[test]
    fn the_sun_does_not_drive_the_moon() -> Result<(), TestError> {
        let mut app = rows_app();
        let sun = trackball(&mut app, WINDOW, AimKnobs::SUN, TrackballAim::ZENITH);
        let moon_azimuth =
            slider(&mut app, WINDOW, SkyKnob::MoonAzimuth, 7.0).ok_or("not an aim knob")?;
        app.update();

        app.world_mut().entity_mut(sun).insert(TrackballAim {
            azimuth: 200.0,
            elevation: 5.0,
        });
        app.update();

        assert_eq!(
            value_of(&app, moon_azimuth),
            Some(7.0),
            "the sun wrote the moon's slider"
        );
        Ok(())
    }

    /// **Two windows showing the same body do not drive each other.** Three
    /// windows in this crate draw a sun trackball, and any two of them can be
    /// open at once over completely different skies — one holding an inventory
    /// asset, the other the sky the user is standing under.
    #[test]
    fn one_window_does_not_drive_another() -> Result<(), TestError> {
        let mut app = rows_app();
        let mine = trackball(&mut app, WINDOW, AimKnobs::SUN, TrackballAim::ZENITH);
        let theirs = slider(&mut app, OTHER, SkyKnob::SunAzimuth, 11.0).ok_or("not an aim knob")?;
        let theirs_ball = trackball(
            &mut app,
            OTHER,
            AimKnobs::SUN,
            TrackballAim {
                azimuth: 11.0,
                elevation: 3.0,
            },
        );
        app.update();

        app.world_mut().entity_mut(mine).insert(TrackballAim {
            azimuth: 300.0,
            elevation: -60.0,
        });
        app.update();

        assert_eq!(value_of(&app, theirs), Some(11.0), "the other window moved");
        assert_eq!(
            aim_of(&app, theirs_ball),
            Some(TrackballAim {
                azimuth: 11.0,
                elevation: 3.0,
            })
        );
        Ok(())
    }

    /// **The pair settles.** Each control writes the other, so the one thing
    /// this wiring must not do is keep writing: a trackball that moved its
    /// sliders must find, on the next frame, that they are telling it exactly
    /// what it already holds.
    #[test]
    fn the_pair_stops_writing_once_it_agrees() -> Result<(), TestError> {
        let mut app = rows_app();
        let ball = trackball(&mut app, WINDOW, AimKnobs::SUN, TrackballAim::ZENITH);
        let azimuth =
            slider(&mut app, WINDOW, SkyKnob::SunAzimuth, 0.0).ok_or("not an aim knob")?;
        let elevation =
            slider(&mut app, WINDOW, SkyKnob::SunElevation, 0.0).ok_or("not an aim knob")?;
        app.update();
        app.world_mut().entity_mut(ball).insert(TrackballAim {
            azimuth: 42.0,
            elevation: 17.0,
        });
        for _ in 0..3_u8 {
            app.update();
        }
        // A frame with nothing in it: if either half were still writing, one of
        // the three components below would be marked changed by it.
        app.update();
        let changed = |app: &mut App, entity: Entity| -> bool {
            app.world_mut()
                .query_filtered::<Entity, Or<(Changed<SliderValue>, Changed<TrackballAim>)>>()
                .iter(app.world())
                .any(|touched| touched == entity)
        };
        assert!(!changed(&mut app, ball), "the trackball is still writing");
        assert!(!changed(&mut app, azimuth), "the azimuth slider is");
        assert!(!changed(&mut app, elevation), "the elevation slider is");
        assert_eq!(value_of(&app, azimuth), Some(42.0));
        assert_eq!(value_of(&app, elevation), Some(17.0));
        Ok(())
    }

    /// **A slider's range is what a trackball's angle lands in.** The azimuth
    /// slider stops a hair short of a full turn, so a trackball aimed past that
    /// point must not hand it a value it cannot show.
    #[test]
    fn a_trackballs_angle_is_clamped_to_the_sliders_range() -> Result<(), TestError> {
        let mut app = rows_app();
        let ball = trackball(&mut app, WINDOW, AimKnobs::SUN, TrackballAim::ZENITH);
        let azimuth =
            slider(&mut app, WINDOW, SkyKnob::SunAzimuth, 0.0).ok_or("not an aim knob")?;
        app.update();
        app.world_mut().entity_mut(ball).insert(TrackballAim {
            azimuth: 359.999,
            elevation: 0.0,
        });
        app.update();
        let (_min, max) = SkyKnob::SunAzimuth.range();
        assert_eq!(value_of(&app, azimuth), Some(max));
        Ok(())
    }
}
