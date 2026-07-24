//! The **colour-picker floater + swatch widget** (`viewer-ui-color-picker`): a
//! reusable colour swatch any panel can host, and a shared picker floater it
//! opens — the reference's `LLColorSwatchCtrl` + `LLFloaterColorPicker`.
//!
//! # Model
//!
//! - [`spawn_color_swatch`] drops a bordered button whose fill is its current
//!   colour ([`ColorSwatchValue`]); clicking it emits [`OpenColorPicker`] tagged
//!   with the swatch entity as the **requester**. A consumer keeps the swatch's
//!   [`ColorSwatchValue`] up to date (this module paints the fill from it) and
//!   reads [`ColorPicked`] filtered to its own swatch.
//! - The picker floater carries three R/G/B [`Slider`]s (0..255), a live preview
//!   swatch, and the original colour to compare against; **OK** emits
//!   [`ColorPicked`], **Cancel** just closes (the reference reverts on cancel).
//!   The saturation/value square, hue strip, and saved-swatch palette of the
//!   full reference floater are a refinement left for later; R/G/B with a live
//!   preview is the useful core (and, like the light colour, more than the
//!   numeric-field stand-in it replaces).
//!
//! Reference (Firestorm, read-only): `llfloatercolorpicker`, `llcolorswatch`.

use bevy::input_focus::tab_navigation::TabIndex;
use bevy::prelude::*;
use bevy::ui_widgets::{
    Button, Slider, SliderRange, SliderStep, SliderThumb, SliderValue, ValueChange,
};
use bevy_flair::style::components::ClassList;

use crate::floater::{FloaterCaps, FloaterSpec, spawn_floater};
use crate::i18n::Translated;
use crate::ui::{LogicalInset, LogicalRect, UiPanelShown, UiRoot, UiScaffoldSystems, column, row};
use crate::ui_font::UiFont;

/// The picker's numeric-channel maximum (an sRGB byte).
const CHANNEL_MAX: f32 = 255.0;

/// The RGB slider track width, in logical pixels.
const TRACK_WIDTH: f32 = 160.0;

/// The RGB slider track height.
const TRACK_HEIGHT: f32 = 14.0;

/// The slider thumb width.
const THUMB_WIDTH: f32 = 10.0;

/// A preview / original swatch's side length.
const SWATCH_SIZE: f32 = 40.0;

/// The picker font size.
const PICKER_FONT: f32 = 13.0;

/// A bordered control's border colour.
const CONTROL_BORDER: Color = Color::srgba(0.4, 0.4, 0.45, 1.0);

/// A slider track's fill.
const TRACK_FILL: Color = Color::srgba(0.12, 0.12, 0.14, 1.0);

/// A slider thumb's fill.
const THUMB_FILL: Color = Color::srgb(0.75, 0.78, 0.85);

/// A button's background.
const BUTTON_BACKGROUND: Color = Color::srgba(0.18, 0.18, 0.2, 1.0);

/// The text colour.
const TEXT_COLOR: Color = Color::srgb(0.9, 0.92, 0.96);

/// The skin class for value text.
const VALUE_CLASS: &str = "sk-build-value";

/// A reusable colour swatch's current value; this module paints the swatch fill
/// from it, and a consumer reads / writes it.
#[derive(Component, Debug, Clone, Copy)]
pub(crate) struct ColorSwatchValue(pub(crate) Color);

/// Spawn a colour swatch under `parent`: a bordered button filled with `initial`
/// that opens the picker on click, tagged with `element` for its [`Name`]. The
/// returned entity is the **requester** a [`ColorPicked`] reply is matched by.
pub(crate) fn spawn_color_swatch(
    commands: &mut Commands,
    parent: Entity,
    element: &'static str,
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
    swatches: Query<&ColorSwatchValue>,
    mut opens: MessageWriter<OpenColorPicker>,
) {
    if press.button != PointerButton::Primary {
        return;
    }
    if let Ok(value) = swatches.get(press.entity) {
        opens.write(OpenColorPicker {
            requester: press.entity,
            current: value.0,
        });
    }
}

/// Open the colour picker for `requester`, seeded with `current`.
#[derive(Message, Debug, Clone, Copy)]
pub(crate) struct OpenColorPicker {
    /// The swatch (or other widget) the reply is tagged back to.
    pub(crate) requester: Entity,
    /// The colour to open on.
    pub(crate) current: Color,
}

/// The chosen colour, tagged back to the [`requester`](Self::requester) that
/// opened the picker. Emitted **continuously** while dragging (with
/// [`final_pick`](Self::final_pick) `false`) so a consumer can live-preview, and
/// once on **OK** with `final_pick` `true`; **Cancel** emits the original colour
/// with `final_pick` `false` so the consumer reverts its preview.
#[derive(Message, Debug, Clone, Copy)]
pub(crate) struct ColorPicked {
    /// The widget that opened the picker.
    pub(crate) requester: Entity,
    /// The chosen colour.
    pub(crate) color: Color,
    /// Whether this is the committed choice (**OK**) rather than a live-preview
    /// or revert update.
    pub(crate) final_pick: bool,
}

/// The picker's live state while open.
#[derive(Resource, Debug, Default)]
struct ColorPickerState {
    /// The widget that opened it, or `None` when closed.
    requester: Option<Entity>,
    /// The colour it opened on (for Cancel / the original swatch).
    original: Color,
    /// The three channel values, 0..255.
    channels: [f32; 3],
}

impl ColorPickerState {
    /// The current colour built from the channel bytes.
    fn current(&self) -> Color {
        let channel = |index: usize| byte(self.channels.get(index).copied().unwrap_or(0.0));
        Color::srgb_u8(channel(0), channel(1), channel(2))
    }
}

/// The picker floater's entities.
#[derive(Resource, Debug)]
struct ColorPickerUi {
    /// The floater root (carries `UiPanelShown`).
    panel: Entity,
    /// The live preview swatch.
    preview: Entity,
    /// The original-colour swatch.
    original: Entity,
    /// The three R/G/B sliders.
    sliders: [Entity; 3],
    /// The three channel value labels.
    labels: [Entity; 3],
}

/// A slider's channel index (0 = R, 1 = G, 2 = B).
#[derive(Component, Debug, Clone, Copy)]
struct ColorChannel(usize);

/// A picker action button.
#[derive(Component, Debug, Clone, Copy)]
enum PickerButton {
    /// Accept the current colour.
    Ok,
    /// Discard and close.
    Cancel,
}

/// The plugin wiring the colour picker into the viewer.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct ColorPickerPlugin;

impl Plugin for ColorPickerPlugin {
    /// Register the messages, state, floater, and systems.
    fn build(&self, app: &mut App) {
        app.add_message::<OpenColorPicker>()
            .add_message::<ColorPicked>()
            .init_resource::<ColorPickerState>()
            .add_systems(
                Startup,
                spawn_color_picker_floater.after(UiScaffoldSystems::SpawnRoot),
            )
            .add_systems(
                Update,
                (
                    handle_open_color_picker,
                    sync_color_picker_visual,
                    apply_color_swatch_fill,
                ),
            );
    }
}

/// Build the shared colour-picker floater (hidden until opened).
fn spawn_color_picker_floater(mut commands: Commands, root: Option<Res<UiRoot>>) {
    let Some(root) = root.map(|root| root.0) else {
        return;
    };
    let handle = spawn_floater(
        &mut commands,
        root,
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
        },
    );
    // Subject-bound: it opens on whatever swatch requested it, disconnected from
    // saved app state, so it is exempt from floater persistence — never restored
    // open, no remembered rectangle (as the avatar profile / item previews are).
    commands
        .entity(handle.root)
        .insert(crate::floater_persist::FloaterPersistExempt);
    commands
        .entity(handle.title_text)
        .insert(Translated::new("color-picker-title"));
    let content = commands
        .spawn((
            Node {
                padding: UiRect::all(Val::Px(8.0)),
                ..column(Val::Px(8.0))
            },
            ChildOf(handle.content),
        ))
        .id();

    // Preview + original comparison row.
    let compare = commands
        .spawn((
            Node {
                align_items: AlignItems::Center,
                ..row(Val::Px(10.0))
            },
            ChildOf(content),
        ))
        .id();
    let preview = spawn_compare_swatch(&mut commands, compare, "color-picker-preview");
    let original = spawn_compare_swatch(&mut commands, compare, "color-picker-original");

    // Three R/G/B slider rows.
    let mut sliders = [Entity::PLACEHOLDER; 3];
    let mut labels = [Entity::PLACEHOLDER; 3];
    for (channel, name) in [(0_usize, "R"), (1, "G"), (2, "B")] {
        let (slider, label) = spawn_channel_row(&mut commands, content, channel, name);
        if let Some(slot) = sliders.get_mut(channel) {
            *slot = slider;
        }
        if let Some(slot) = labels.get_mut(channel) {
            *slot = label;
        }
    }

    // OK / Cancel buttons.
    let buttons = commands
        .spawn((
            Node {
                align_items: AlignItems::Center,
                justify_content: JustifyContent::FlexEnd,
                ..row(Val::Px(8.0))
            },
            ChildOf(content),
        ))
        .id();
    spawn_picker_button(&mut commands, buttons, PickerButton::Ok, "color-picker-ok");
    spawn_picker_button(
        &mut commands,
        buttons,
        PickerButton::Cancel,
        "color-picker-cancel",
    );

    commands.insert_resource(ColorPickerUi {
        panel: handle.root,
        preview,
        original,
        sliders,
        labels,
    });
}

/// Spawn a comparison swatch (preview / original).
fn spawn_compare_swatch(commands: &mut Commands, parent: Entity, name: &'static str) -> Entity {
    commands
        .spawn((
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
        ))
        .id()
}

/// Spawn one channel row: a name label, a slider track + thumb, and a value
/// label. Returns the slider and value-label entities.
fn spawn_channel_row(
    commands: &mut Commands,
    parent: Entity,
    channel: usize,
    name: &'static str,
) -> (Entity, Entity) {
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
            SliderRange::new(0.0, CHANNEL_MAX),
            SliderStep(1.0),
            ColorChannel(channel),
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
                min_width: Val::Px(28.0),
                ..Default::default()
            },
            ChildOf(channel_row),
        ))
        .id();
    (slider, label)
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

/// A slider drag: write the value back, update the picker's channel, and drive
/// the preview live.
fn on_color_slider_change(
    change: On<ValueChange<f32>>,
    channels: Query<&ColorChannel>,
    ranges: Query<&SliderRange>,
    mut state: ResMut<ColorPickerState>,
    mut picked: MessageWriter<ColorPicked>,
    mut commands: Commands,
) {
    let slider = change.source;
    let clamped = ranges
        .get(slider)
        .map_or(change.value, |range| range.clamp(change.value));
    commands.entity(slider).insert(SliderValue(clamped));
    if let Ok(channel) = channels.get(slider)
        && let Some(slot) = state.channels.get_mut(channel.0)
    {
        *slot = clamped;
    }
    // Live-preview the new colour on the requester (no commit).
    if let Some(requester) = state.requester {
        picked.write(ColorPicked {
            requester,
            color: state.current(),
            final_pick: false,
        });
    }
}

/// Handle an [`OpenColorPicker`]: seed the state and sliders and show the floater.
fn handle_open_color_picker(
    mut opens: MessageReader<OpenColorPicker>,
    ui: Option<Res<ColorPickerUi>>,
    mut state: ResMut<ColorPickerState>,
    mut panels: Query<&mut UiPanelShown>,
    mut commands: Commands,
) {
    let Some(ui) = ui else {
        return;
    };
    let Some(open) = opens.read().last() else {
        return;
    };
    let srgba = open.current.to_srgba();
    state.requester = Some(open.requester);
    state.original = open.current;
    state.channels = [
        (srgba.red * CHANNEL_MAX).round(),
        (srgba.green * CHANNEL_MAX).round(),
        (srgba.blue * CHANNEL_MAX).round(),
    ];
    for (channel, slider) in ui.sliders.iter().enumerate() {
        if let Some(value) = state.channels.get(channel) {
            commands.entity(*slider).insert(SliderValue(*value));
        }
    }
    // The original swatch's colour is painted by the visual sync from
    // `state.original`; only the panel needs showing here.
    if let Ok(mut shown) = panels.get_mut(ui.panel) {
        shown.0 = true;
    }
}

/// Reconcile the picker's preview / original swatches, slider thumbs, and value
/// labels from the live state.
fn sync_color_picker_visual(
    ui: Option<Res<ColorPickerUi>>,
    state: Res<ColorPickerState>,
    sliders: Query<(&SliderValue, &SliderRange, &Children)>,
    mut backgrounds: Query<&mut BackgroundColor>,
    mut insets: Query<&mut LogicalInset, With<SliderThumb>>,
    mut texts: Query<&mut Text>,
) {
    let Some(ui) = ui else {
        return;
    };
    if let Ok(mut preview) = backgrounds.get_mut(ui.preview) {
        preview.0 = state.current();
    }
    if let Ok(mut original) = backgrounds.get_mut(ui.original) {
        original.0 = state.original;
    }
    for (index, slider) in ui.sliders.iter().enumerate() {
        let Ok((value, range, children)) = sliders.get(*slider) else {
            continue;
        };
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
        if let Some(label) = ui.labels.get(index)
            && let Ok(mut text) = texts.get_mut(*label)
        {
            let want = byte(value.0).to_string();
            if text.0 != want {
                text.0 = want;
            }
        }
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

/// OK / Cancel: OK emits [`ColorPicked`] for the requester; both close the
/// floater.
fn on_picker_button(
    press: On<Pointer<Press>>,
    buttons: Query<&PickerButton>,
    ui: Option<Res<ColorPickerUi>>,
    mut state: ResMut<ColorPickerState>,
    mut picked: MessageWriter<ColorPicked>,
    mut panels: Query<&mut UiPanelShown>,
) {
    if press.button != PointerButton::Primary {
        return;
    }
    let Ok(which) = buttons.get(press.entity) else {
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
    if let Some(ui) = ui
        && let Ok(mut shown) = panels.get_mut(ui.panel)
    {
        shown.0 = false;
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
