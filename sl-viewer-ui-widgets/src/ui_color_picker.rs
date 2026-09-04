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
use sl_viewer_ui_core::i18n::Translated;
use sl_viewer_ui_core::ui::{
    LogicalInset, LogicalRect, UiPanelShown, UiRoot, UiScaffoldSystems, column, row,
};
use sl_viewer_ui_core::ui_font::UiFont;

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

/// A [disabled](bevy::ui::InteractionDisabled) swatch's border — dimmed so a
/// swatch the consumer cannot change reads as disabled.
const DISABLED_BORDER: Color = Color::srgba(0.28, 0.28, 0.32, 1.0);

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
pub struct ColorSwatchValue(pub Color);

/// Spawn a colour swatch under `parent`: a bordered button filled with `initial`
/// that opens the picker on click, tagged with `element` for its [`Name`]. The
/// returned entity is the **requester** a [`ColorPicked`] reply is matched by.
pub fn spawn_color_swatch(
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
    if let Ok(value) = swatches.get(press.entity) {
        opens.write(OpenColorPicker {
            requester: press.entity,
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

/// Open the colour picker for `requester`, seeded with `current`.
#[derive(Message, Debug, Clone, Copy)]
pub struct OpenColorPicker {
    /// The swatch (or other widget) the reply is tagged back to.
    pub requester: Entity,
    /// The colour to open on.
    pub current: Color,
}

/// The chosen colour, tagged back to the [`requester`](Self::requester) that
/// opened the picker. Emitted **continuously** while dragging (with
/// [`final_pick`](Self::final_pick) `false`) so a consumer can live-preview, and
/// once on **OK** with `final_pick` `true`; **Cancel** emits the original colour
/// with `final_pick` `false` so the consumer reverts its preview.
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
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
enum PickerButton {
    /// Accept the current colour.
    Ok,
    /// Discard and close.
    Cancel,
}

/// The plugin wiring the colour picker into the viewer.
#[derive(Debug, Clone, Copy, Default)]
pub struct ColorPickerPlugin;

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
                // Ordered, not a bare tuple: the visual sync reads the slider
                // values the open handler seeds (and needs its commands applied
                // to see them), so an unordered pair would leave the thumbs a
                // frame behind the colour the picker opened on.
                (
                    handle_open_color_picker,
                    sync_color_picker_visual,
                    apply_color_swatch_fill,
                    reflect_color_swatch_disabled,
                )
                    .chain(),
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
    // One shared floater serves one requester, so a frame carrying several
    // requests can only honour one of them — the **first**, the earliest click.
    // Taking the last instead would silently leave that requester waiting on a
    // picker that opened on somebody else's colour, so the losers are said out
    // loud rather than dropped.
    let mut requests = opens.read();
    let Some(open) = requests.next().copied() else {
        return;
    };
    let ignored = requests.count();
    if ignored > 0 {
        warn!(
            "{ignored} further colour-picker request(s) in one frame ignored; the picker opened \
             for {:?}",
            open.requester
        );
    }
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

/// Reconcile the picker's preview / original swatches, slider thumbs, and value
/// labels from the live state.
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
    if state.requester.is_none() {
        return;
    }
    let current = state.current();
    if let Ok(mut preview) = backgrounds.get_mut(ui.preview)
        && preview.0 != current
    {
        preview.0 = current;
    }
    if let Ok(mut original) = backgrounds.get_mut(ui.original)
        && original.0 != state.original
    {
        original.0 = state.original;
    }
    for (index, slider) in ui.sliders.iter().enumerate() {
        let Ok((value, range, children)) = sliders.get(*slider) else {
            continue;
        };
        let offset = Val::Px(thumb_offset(value.0, range));
        for child in children.iter() {
            if let Ok(mut inset) = insets.get_mut(child)
                && inset.0.inline_start != offset
            {
                inset.0.inline_start = offset;
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

#[cfg(test)]
mod tests {
    use bevy::camera::NormalizedRenderTarget;
    use bevy::picking::backend::HitData;
    use bevy::picking::pointer::{Location, PointerId};
    use bevy::prelude::*;
    use bevy::ui_widgets::{SliderRange, SliderThumb, ValueChange};
    use pretty_assertions::assert_eq;

    use super::{
        ColorPicked, ColorPickerPlugin, ColorPickerState, ColorPickerUi, ColorSwatchValue,
        OpenColorPicker, PickerButton, byte, spawn_color_swatch, thumb_offset,
    };
    use sl_viewer_ui_core::ui::{LogicalInset, UiPanelShown, UiRoot};

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

    /// A headless app carrying the picker plugin, a `UiRoot` for its floater to
    /// hang from, and the recorder — everything the picker's *behaviour* needs,
    /// minus the picking backend (the tests synthesise the presses themselves).
    fn picker_app() -> App {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .add_plugins(ColorPickerPlugin)
            .init_resource::<Recorded>()
            .add_systems(PostUpdate, record);
        let root = app.world_mut().spawn(Node::default()).id();
        app.insert_resource(UiRoot(root));
        // Startup builds the floater; a second frame settles the spawn's own
        // change marks so a later churn count measures the sync alone.
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

    /// The picker's live requester, if it is open.
    fn requester(app: &App) -> Option<Entity> {
        app.world().resource::<ColorPickerState>().requester
    }

    /// Whether the picker floater is showing.
    fn shown(app: &App) -> bool {
        let panel = app.world().resource::<ColorPickerUi>().panel;
        app.world()
            .entity(panel)
            .get::<UiPanelShown>()
            .is_some_and(|shown| shown.0)
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
        let state = ColorPickerState {
            requester: None,
            original: Color::BLACK,
            channels: [12.0, 200.4, 255.0],
        };
        assert_eq!(bytes(state.current()), [12, 200, 255, 255]);
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

    /// Clicking a swatch opens the picker on that swatch's colour, seeding the
    /// three sliders and showing the floater.
    #[expect(
        clippy::float_cmp,
        reason = "the channels are exact bytes seeded from an exact colour"
    )]
    #[test]
    fn a_swatch_opens_the_picker_on_its_own_colour() -> Result<(), TestError> {
        let mut app = picker_app();
        let swatch = swatch(&mut app, Color::srgb_u8(10, 20, 30));
        press(&mut app, swatch);
        assert_eq!(requester(&app), Some(swatch));
        assert!(shown(&app), "the floater is shown");
        let state = app.world().resource::<ColorPickerState>();
        assert_eq!(state.channels, [10.0, 20.0, 30.0]);
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
        assert_eq!(requester(&app), None);
        assert!(!shown(&app));
        Ok(())
    }

    /// Two swatches asking in one frame: one shared floater can only answer one,
    /// and it answers the **first** — the earliest click — rather than silently
    /// discarding it in favour of the last.
    #[expect(
        clippy::float_cmp,
        reason = "the channels are exact bytes seeded from an exact colour"
    )]
    #[test]
    fn the_first_of_two_requests_in_a_frame_wins() -> Result<(), TestError> {
        let mut app = picker_app();
        let first = swatch(&mut app, Color::srgb_u8(1, 2, 3));
        let second = swatch(&mut app, Color::srgb_u8(9, 8, 7));
        {
            let mut opens = app.world_mut().resource_mut::<Messages<OpenColorPicker>>();
            opens.write(OpenColorPicker {
                requester: first,
                current: Color::srgb_u8(1, 2, 3),
            });
            opens.write(OpenColorPicker {
                requester: second,
                current: Color::srgb_u8(9, 8, 7),
            });
        }
        app.update();
        assert_eq!(requester(&app), Some(first));
        let state = app.world().resource::<ColorPickerState>();
        assert_eq!(
            state.channels,
            [1.0, 2.0, 3.0],
            "the picker opened on the first requester's colour"
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

    /// Dragging a slider updates the channel, live-previews the new colour to the
    /// requester without committing, and moves that thumb — and only that thumb.
    #[expect(
        clippy::float_cmp,
        reason = "the channel takes the exact value the drag reported"
    )]
    #[test]
    fn a_slider_drag_previews_and_moves_its_thumb() -> Result<(), TestError> {
        let mut app = picker_app();
        let swatch = swatch(&mut app, Color::BLACK);
        press(&mut app, swatch);
        settle(&mut app);
        let slider = *app
            .world()
            .resource::<ColorPickerUi>()
            .sliders
            .first()
            .ok_or("the picker has no red slider")?;
        app.world_mut().trigger(ValueChange {
            source: slider,
            value: super::CHANNEL_MAX,
            is_final: false,
        });
        app.update();

        let state = app.world().resource::<ColorPickerState>();
        assert_eq!(state.channels, [255.0, 0.0, 0.0]);
        let recorded = app.world().resource::<Recorded>();
        let last = recorded.picked.last().ok_or("no preview was emitted")?;
        assert_eq!(last.requester, swatch);
        assert_eq!(bytes(last.color), [255, 0, 0, 255]);
        assert!(!last.final_pick, "a drag previews, it does not commit");
        assert_eq!(
            recorded.inset_writes, 1,
            "only the red thumb moved; the other two are where they were"
        );

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

    /// **OK** commits the chosen colour to the requester and closes the floater.
    #[test]
    fn ok_commits_the_chosen_colour() -> Result<(), TestError> {
        let mut app = picker_app();
        let swatch = swatch(&mut app, Color::srgb_u8(10, 20, 30));
        press(&mut app, swatch);
        let slider = *app
            .world()
            .resource::<ColorPickerUi>()
            .sliders
            .first()
            .ok_or("the picker has no red slider")?;
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
        assert_eq!(requester(&app), None, "the picker is closed");
        assert!(!shown(&app));
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
        let slider = *app
            .world()
            .resource::<ColorPickerUi>()
            .sliders
            .first()
            .ok_or("the picker has no red slider")?;
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
        assert_eq!(requester(&app), None);
        assert!(!shown(&app));
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

    /// **The picker, driven** (`viewer-ui-widget-interaction-suite`): a click on
    /// the swatch, a drag along a channel track, and OK — through the real
    /// pointer, on the real geometry.
    ///
    /// Every test above hands the widget a `ValueChange` already carrying the
    /// number it is meant to arrive at, which makes them tests of what the
    /// picker does with a value and not of where a value comes from. A slider
    /// is the one widget here whose output is a *function of its own layout*:
    /// `bevy_ui_widgets` converts a drag distance into a value through the
    /// track's measured width less the thumb's, so a track that laid out at
    /// the wrong size, or a thumb the descendant search cannot find, moves the
    /// channel by the wrong amount for a gesture that still looks right. Only a
    /// real drag on a laid-out track can see that.
    mod scenarios {
        use bevy::prelude::*;
        use bevy::ui_widgets::{SliderRange, SliderThumb, SliderValue};
        use pretty_assertions::assert_eq;

        use super::{TestError, bytes};
        use crate::ui_color_picker::{
            CHANNEL_MAX, ColorPicked, ColorPickerPlugin, ColorPickerState, THUMB_WIDTH,
            TRACK_WIDTH, spawn_color_swatch, thumb_offset,
        };
        use crate::ui_test::interact::{self, InteractionTest, centre_of};
        use crate::ui_test::{drain, find_by_name, record, settle};
        use sl_viewer_ui_core::ui::{LogicalInset, UiPanelShown, UiRoot, UiScaffoldSystems};

        /// The swatch's node name.
        const SWATCH: &str = "test:color-swatch";

        /// The red channel's slider.
        const RED_SLIDER: &str = "color-picker-slider:R";

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

        /// Whether the picker floater is on screen.
        fn picker_shown(app: &App) -> Option<bool> {
            let panel = app.world().get_resource::<super::ColorPickerUi>()?.panel;
            app.world().get::<UiPanelShown>(panel).map(|shown| shown.0)
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
            assert_eq!(picker_shown(&app), Some(false), "the picker starts closed");

            interact::click_node(&mut app, SWATCH)?;
            settle(&mut app);
            assert_eq!(
                picker_shown(&app),
                Some(true),
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

            let channels = app.world().resource::<ColorPickerState>().channels;
            let red = channels.first().copied().ok_or("no red channel")?;
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
            assert_eq!(picker_shown(&app), Some(false), "OK puts the picker away");
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

            let red = app
                .world()
                .resource::<ColorPickerState>()
                .channels
                .first()
                .copied()
                .ok_or("no red channel")?;
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
