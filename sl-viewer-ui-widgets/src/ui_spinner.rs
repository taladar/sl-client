//! The reusable **spinner** (`viewer-build-numeric-field-spinners`): a numeric
//! text field with a pair of up / down step arrows beside it — the reference
//! viewer's `LLSpinCtrl`.
//!
//! ```text
//! root (.sk-spinner)                    `{element}:spinner`
//! ├─ arrows (.sk-spinner-arrows)        the column at the inline start
//! │  ├─ up   (.sk-spinner-arrow-up)     `{element}:up`, "Increase"
//! │  └─ down (.sk-spinner-arrow-down)   `{element}:down`, "Decrease"
//! └─ field (.sk-field)                  `{element}:field`, a spin button
//! ```
//!
//! The field is an ordinary [`spawn_text_input`] numeric field — every consumer
//! that already reads and writes it (its sync, its Enter / blur commit) keeps
//! doing so unchanged. The widget adds the arrows and what they do:
//!
//! - **A press steps once, and holding repeats.** Each arrow is
//!   `bevy_ui_widgets`' [`Button`] with [`ActivateOnPress`] and
//!   [`HoldToRepeat`], the scrollbar arrows' arrangement: one step on the
//!   press, then the reference button's 0.5 s hold delay and a steady repeat.
//! - **A step is the reference's arithmetic.** The value is read back out of
//!   the field's text (a field that holds no number does not step, as
//!   `LLSpinCtrl::onUpBtn` refuses one `postvalidateFloat` rejects), moved by
//!   the increment, rounded to the field's decimals (`clamp_precision`) and
//!   clamped to its range. The modifier keys scale the increment the way
//!   Firestorm's `<FS:KC>` change does: **Alt** ×10, **Ctrl** ×0.1, **Shift**
//!   ×0.01, the first of them held in that order.
//! - **A step is a commit.** The reference calls `onCommit` after every step,
//!   so each one sends exactly what an `Enter` in the field would. The widget
//!   cannot know what that is — a `MultipleObjectUpdate`, an `ObjectImage`, a
//!   settings write — so it says *that* the field was stepped, with a
//!   [`SpinnerStepped`] message, and a consumer's commit system treats it as an
//!   `Enter`. [`FieldCommits`] is that commit system's input: Enter, blur and
//!   step, in one list, so no consumer re-derives the three by hand.
//! - **The arrow keys step too**, while the field has the keyboard focus —
//!   `LLSpinCtrl::handleKeyHere`'s `KEY_UP` / `KEY_DOWN`, with the same
//!   modifiers. In a one-line field they would only move the caret to an end.
//! - **The arrows share the field's gate.** A consumer disables a field the
//!   standard way — [`InteractionDisabled`] on the field — and the widget mirrors
//!   it onto both arrows (`mirror_field_gate`), so they grey (`:disabled` in the
//!   skin) and refuse the press with no per-consumer wiring. A
//!   [read-only](ReadOnlyField) field steps no more than it types.
//!
//! The arrows are not focus stops — the reference spawns them `tab_stop(false)`
//! — so the keyboard reaches the spinner through its field, and the field is
//! what the automation model and a screen reader see as the
//! [`Role::SpinButton`], valued by its number, with its range and step on its
//! [`AccessibilityNode`].
//!
//! The wheel does **not** step. The reference's `handleScrollWheel` does, over
//! any spinner under the pointer; here the wheel scrolls the panel a spinner
//! sits in (the build floater's pages are long), and a value that changed
//! because the page was scrolled across it is the one behaviour of the
//! reference's that is a trap rather than a feature.
//!
//! Reference (Firestorm, read-only): `llspinctrl.cpp` / `llspinctrl.h`,
//! `widgets/spinner.xml`.

use bevy::a11y::AccessibilityNode;
use bevy::ecs::system::SystemParam;
use bevy::input::ButtonState;
use bevy::input::keyboard::{Key, KeyboardInput};
use bevy::input_focus::InputFocus;
use bevy::prelude::*;
use bevy::text::{EditableText, EditableTextSystems, FontCx, LayoutCx, LineHeight, TextEdit};
use bevy::ui::InteractionDisabled;
use bevy::ui_widgets::{Activate, ActivateOnPress, Button};
use bevy_flair::style::components::{ClassList, PseudoElementsSupport};

use sl_viewer_ui_core::hold_repeat::{HoldToRepeat, ensure_hold_repeat};
use sl_viewer_ui_core::semantic::{Role, Semantic};
use sl_viewer_ui_core::skin::{
    SPINNER_ARROW_CLASS, SPINNER_ARROW_DOWN_CLASS, SPINNER_ARROW_GLYPH_CLASS,
    SPINNER_ARROW_UP_CLASS, SPINNER_ARROWS_CLASS, SPINNER_CLASS,
};
use sl_viewer_ui_core::skin_palette::SkinPalette;
use sl_viewer_ui_core::ui_element::ElementCx;
use sl_viewer_ui_core::ui_font::UiFont;
use sl_viewer_ui_core::ui_text::set_editor_text;

use crate::settings_binding::{SettingBinding, write_bound_number};
use crate::ui_text_input::{ReadOnlyField, TextInputKind, TextInputSpec, spawn_text_input};
use sl_viewer_settings::ViewerSettings;

/// The width of the arrow column against the field's font size — the
/// reference's `UISpinctrlBtnWidth` is a little under one line of its small
/// font.
const ARROW_WIDTH_SCALE: f32 = 0.9;

/// The narrowest the arrow column gets, in logical pixels, so a small font
/// still leaves an arrow the pointer can hit.
const MIN_ARROW_WIDTH: f32 = 12.0;

/// The arrow glyph's size against the field's font size.
const ARROW_GLYPH_SCALE: f32 = 0.7;

/// The border around and between the two arrows, in logical pixels.
const ARROWS_BORDER: f32 = 1.0;

/// The increment multiplier while **Alt** is held (Firestorm's coarse step).
const ALT_SCALE: f64 = 10.0;

/// The increment multiplier while **Ctrl** is held (Firestorm's fine step).
const CTRL_SCALE: f64 = 0.1;

/// The increment multiplier while **Shift** is held (Firestorm's finest step).
const SHIFT_SCALE: f64 = 0.01;

/// What one step of a spinner is: how far it moves, between which bounds, and
/// how many decimals the result is rounded to — the reference's `increment`,
/// `min_val`, `max_val` and `decimal_digits`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SpinStep {
    /// How far one unmodified step moves the value.
    pub increment: f64,
    /// The smallest value a step leaves behind.
    pub min: f64,
    /// The largest value a step leaves behind.
    pub max: f64,
    /// How many decimals a stepped value is rounded to and shown with. Match
    /// it to the decimals the consumer's own sync writes, so a step and a
    /// re-sync show the same text.
    pub decimals: usize,
}

impl SpinStep {
    /// A step of `increment` within `min..=max`, shown with `decimals`
    /// decimals.
    #[must_use]
    pub const fn new(increment: f64, min: f64, max: f64, decimals: usize) -> Self {
        Self {
            increment,
            min,
            max,
            decimals,
        }
    }
}

/// Which way an arrow (or an arrow key) steps.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpinDirection {
    /// Toward `max`.
    Up,
    /// Toward `min`.
    Down,
}

impl SpinDirection {
    /// The sign this direction moves the value by.
    const fn sign(self) -> f64 {
        match self {
            Self::Up => 1.0,
            Self::Down => -1.0,
        }
    }
}

/// Everything a spinner is built from: the field it wraps and the step it
/// takes.
#[derive(Debug, Clone)]
pub struct SpinnerSpec {
    /// The numeric field. Its `element` names the spinner's parts too
    /// (`{element}:spinner`, `:up`, `:down`), and its `name_key`, when set,
    /// names the spin button.
    pub input: TextInputSpec,
    /// What one step does.
    pub step: SpinStep,
}

/// The entities of a spawned spinner.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SpinnerParts {
    /// The row holding the arrows and the field — what a caller places.
    pub root: Entity,
    /// The numeric field — what a caller reads, writes, gates and commits,
    /// exactly as it would a bare [`spawn_text_input`] field.
    pub field: Entity,
    /// The up arrow.
    pub up: Entity,
    /// The down arrow.
    pub down: Entity,
}

/// On a spinner's **field**: the step it takes and its two arrows.
#[derive(Component, Debug, Clone, Copy, PartialEq)]
pub struct Spinner {
    /// What one step does.
    step: SpinStep,
    /// The up arrow.
    up: Entity,
    /// The down arrow.
    down: Entity,
}

impl Spinner {
    /// What one step does.
    #[must_use]
    pub const fn step(&self) -> SpinStep {
        self.step
    }

    /// Replace what one step does — for a range or an increment that depends
    /// on what the field is showing (the reference re-ranges its build
    /// spinners per prim type: a circular path twists twice as far, in twice
    /// the step).
    pub const fn set_step(&mut self, step: SpinStep) {
        self.step = step;
    }
}

/// On a spinner arrow: whose field it steps, and which way.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
struct SpinArrow {
    /// The spinner's field.
    field: Entity,
    /// Which way it steps.
    direction: SpinDirection,
}

/// A spinner's field was **stepped** — by an arrow, or by an arrow key — to
/// `text`, which the field now shows. A consumer commits it exactly as it
/// commits an `Enter` in the field; [`FieldCommits`] does the reading.
///
/// The text travels with the message because a consumer's own sync may have
/// rewritten the field (from the not-yet-committed value it mirrors) between
/// the step and the commit: the commit must send what the step produced.
#[derive(Message, Debug, Clone, PartialEq, Eq)]
pub struct SpinnerStepped {
    /// The spinner's field.
    pub field: Entity,
    /// What the step left in it.
    pub text: String,
}

/// The increment multiplier the held modifier keys ask for: Firestorm's
/// `<FS:KC>` order — Alt, then Ctrl, then Shift, the first held winning.
fn modifier_scale(keys: Option<&ButtonInput<KeyCode>>) -> f64 {
    let Some(keys) = keys else {
        return 1.0;
    };
    if keys.any_pressed([KeyCode::AltLeft, KeyCode::AltRight]) {
        ALT_SCALE
    } else if keys.any_pressed([KeyCode::ControlLeft, KeyCode::ControlRight]) {
        CTRL_SCALE
    } else if keys.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]) {
        SHIFT_SCALE
    } else {
        1.0
    }
}

/// Round `value` to `decimals` decimals — the reference's `clamp_precision`.
fn round_to_decimals(value: f64, decimals: usize) -> f64 {
    let Ok(exponent) = i32::try_from(decimals) else {
        return value;
    };
    let factor = 10_f64.powi(exponent);
    if factor.is_finite() && factor > 0.0 {
        (value * factor).round() / factor
    } else {
        value
    }
}

/// The value `text` steps to one `direction` step of `step`, scaled by
/// `scale`: rounded to the step's decimals, then clamped to its range. `None`
/// when `text` is not a number (an empty field — nothing selected — steps
/// nowhere, as the reference refuses one).
#[must_use]
pub fn stepped_value(
    text: &str,
    step: &SpinStep,
    direction: SpinDirection,
    scale: f64,
) -> Option<f64> {
    let current: f64 = text
        .trim()
        .parse()
        .ok()
        .filter(|value: &f64| value.is_finite())?;
    let moved = round_to_decimals(
        current + direction.sign() * step.increment * scale,
        step.decimals,
    );
    // Not `clamp`, which panics on a range a consumer re-ranged backwards;
    // the minimum wins, as the reference's last `if` does.
    let clamped = moved.min(step.max).max(step.min);
    clamped.is_finite().then_some(clamped)
}

/// `value` as a spinner shows it: `decimals` decimals, and never a negative
/// zero (the reference's "don't display very small negative values as
/// -0.000").
#[must_use]
pub fn format_spin_value(value: f64, decimals: usize) -> String {
    let shown = format!("{value:.decimals$}");
    if shown.starts_with('-') && shown.chars().skip(1).all(|c| c == '0' || c == '.') {
        format!("{:.decimals$}", 0.0_f64)
    } else {
        shown
    }
}

/// Spawn a spinner of `spec` under `parent`: the arrow column, then the field.
///
/// The field is [`spawn_text_input`]'s, named, sized and tab-ordered by
/// `spec.input`, and it wears a [`Semantic`] of [`Role::SpinButton`] — named by
/// `spec.input.name_key` when there is one, otherwise by its caption the way
/// any field in a captioned row is.
pub fn spawn_spinner(commands: &mut Commands, parent: Entity, spec: &SpinnerSpec) -> SpinnerParts {
    let element = spec.input.element;
    let palette = SkinPalette::default();
    let root = commands
        .spawn((
            Node {
                flex_direction: FlexDirection::Row,
                align_items: AlignItems::Stretch,
                flex_shrink: 0.0,
                ..default()
            },
            ClassList::new_with_classes([SPINNER_CLASS]),
            Name::new(format!("{element}:spinner")),
            ChildOf(parent),
        ))
        .id();
    let arrow_width = (spec.input.font_size * ARROW_WIDTH_SCALE).max(MIN_ARROW_WIDTH);
    let arrows = commands
        .spawn((
            Node {
                flex_direction: FlexDirection::Column,
                width: Val::Px(arrow_width),
                flex_shrink: 0.0,
                border: UiRect::all(Val::Px(ARROWS_BORDER)),
                row_gap: Val::Px(ARROWS_BORDER),
                ..default()
            },
            BorderColor::all(palette.control_border),
            BackgroundColor(palette.control_border),
            ClassList::new_with_classes([SPINNER_ARROWS_CLASS]),
            ChildOf(root),
        ))
        .id();
    let field = spawn_text_input(commands, root, &spec.input);
    let glyph_size = spec.input.font_size * ARROW_GLYPH_SCALE;
    let up = spawn_arrow(
        commands,
        arrows,
        SpinArrow {
            field,
            direction: SpinDirection::Up,
        },
        &format!("{element}:up"),
        glyph_size,
    );
    let down = spawn_arrow(
        commands,
        arrows,
        SpinArrow {
            field,
            direction: SpinDirection::Down,
        },
        &format!("{element}:down"),
        glyph_size,
    );
    let mut semantic = Semantic::new(Role::SpinButton);
    if let Some(key) = spec.input.name_key {
        semantic = semantic.name_key(key);
    }
    commands.entity(field).insert((
        Spinner {
            step: spec.step,
            up,
            down,
        },
        semantic,
        AccessibilityNode(spin_accessibility(&spec.step)),
    ));
    SpinnerParts {
        root,
        field,
        up,
        down,
    }
}

/// The range and step a screen reader announces for a spin button. The
/// screen-reader bridge copies these onto the node it builds from the model,
/// as it does a slider's.
fn spin_accessibility(step: &SpinStep) -> accesskit::Node {
    let mut node = accesskit::Node::new(accesskit::Role::SpinButton);
    node.set_min_numeric_value(step.min);
    node.set_max_numeric_value(step.max);
    node.set_numeric_value_step(step.increment);
    node
}

/// Spawn one arrow: a button that fills half the column, holding an empty
/// glyph host whose `::before` the skin fills.
fn spawn_arrow(
    commands: &mut Commands,
    column: Entity,
    arrow: SpinArrow,
    name: &str,
    glyph_size: f32,
) -> Entity {
    let (direction_class, name_key) = match arrow.direction {
        SpinDirection::Up => (SPINNER_ARROW_UP_CLASS, "spinner-increase"),
        SpinDirection::Down => (SPINNER_ARROW_DOWN_CLASS, "spinner-decrease"),
    };
    let entity = commands
        .spawn((
            // Grows to share the field's height, and never shrinks below its
            // glyph's line: a field shorter than two arrows makes the row
            // taller rather than slicing the arrows.
            Node {
                flex_grow: 1.0,
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                overflow: Overflow::clip(),
                ..default()
            },
            BackgroundColor(SkinPalette::default().control_bg),
            ClassList::new_with_classes([SPINNER_ARROW_CLASS, direction_class]),
            Button,
            ActivateOnPress,
            HoldToRepeat::default(),
            // Only a skin glyph shows on it, which names nothing.
            Semantic::new(Role::Button).name_key(name_key),
            arrow,
            Name::new(name.to_owned()),
            ChildOf(column),
        ))
        .observe(on_spin_arrow)
        .id();
    commands.spawn((
        Text::default(),
        PseudoElementsSupport,
        UiFont::Sans.at(glyph_size),
        // The line box is the glyph, with no leading: two arrows then fit
        // beside a field of the font they are scaled from.
        LineHeight::RelativeToFont(1.0),
        ClassList::new_with_classes([SPINNER_ARROW_GLYPH_CLASS]),
        Pickable::IGNORE,
        ChildOf(entity),
    ));
    entity
}

/// What a step reads and writes of one spinner field: its step, its editor,
/// whether it is gated (disabled or read-only), and the setting it is bound to.
type SpinFieldData = (
    &'static Spinner,
    &'static mut EditableText,
    Has<InteractionDisabled>,
    Has<ReadOnlyField>,
    Option<&'static SettingBinding>,
);

/// The spinner fields a step reads and writes, and the contexts a
/// programmatic rewrite relays through.
#[derive(SystemParam)]
struct SpinFields<'w, 's> {
    /// Each spinner field: its step, its editor, whether it is gated, and the
    /// setting it is bound to.
    fields: Query<'w, 's, SpinFieldData>,
    /// The settings store a bound spinner writes, absent in a host without it.
    settings: Option<ResMut<'w, ViewerSettings>>,
    /// The held modifier keys, absent in a host with no keyboard.
    keys: Option<Res<'w, ButtonInput<KeyCode>>>,
    /// The font context the rewrite relays through.
    font_cx: ResMut<'w, FontCx>,
    /// The layout context the rewrite relays through.
    layout_cx: ResMut<'w, LayoutCx>,
    /// Where the step is announced.
    stepped: MessageWriter<'w, SpinnerStepped>,
}

impl SpinFields<'_, '_> {
    /// Step `field` once in `direction`, write the result into it and
    /// announce it. Nothing happens to a gated field or one holding no
    /// number.
    fn step(&mut self, field: Entity, direction: SpinDirection) {
        let scale = modifier_scale(self.keys.as_deref());
        let Ok((spinner, mut editor, disabled, read_only, binding)) = self.fields.get_mut(field)
        else {
            return;
        };
        if disabled || read_only {
            return;
        }
        let step = spinner.step;
        let Some(value) = stepped_value(&editor.value().to_string(), &step, direction, scale)
        else {
            return;
        };
        let text = format_spin_value(value, step.decimals);
        if editor.value().to_string() != text {
            set_editor_text(&mut editor, &text, &mut self.font_cx, &mut self.layout_cx);
        }
        // A field bound to a setting writes it here, with the step, so the
        // binding's sync pass never sees the new text against the old value.
        if let (Some(binding), Some(settings)) = (binding, self.settings.as_deref_mut()) {
            write_bound_number(settings, binding, value);
        }
        self.stepped.write(SpinnerStepped { field, text });
    }
}

/// An arrow was pressed, or is being held: step its field once.
fn on_spin_arrow(activate: On<Activate>, arrows: Query<&SpinArrow>, mut fields: SpinFields) {
    if let Ok(arrow) = arrows.get(activate.entity) {
        fields.step(arrow.field, arrow.direction);
    }
}

/// Step the focused spinner field once per Up / Down arrow key pressed (and
/// auto-repeated) this frame, and take those keys' caret moves out of its
/// queue, before `bevy_text` drains it.
///
/// With **Ctrl** held, `bevy_ui_widgets` queues the key as a jump to the
/// field's start or end rather than a line move, so those come out too —
/// only on a frame an arrow key actually arrived, so `Home` / `End` are left
/// alone.
fn step_spinners_from_arrow_keys(
    mut keys: MessageReader<KeyboardInput>,
    focus: Option<Res<InputFocus>>,
    mut fields: SpinFields,
) {
    let mut steps: Vec<SpinDirection> = Vec::new();
    for input in keys.read() {
        if input.state != ButtonState::Pressed {
            continue;
        }
        match &input.logical_key {
            Key::ArrowUp => steps.push(SpinDirection::Up),
            Key::ArrowDown => steps.push(SpinDirection::Down),
            _other => {}
        }
    }
    let Some(field) = focus.and_then(|focus| focus.get()) else {
        return;
    };
    if steps.is_empty() {
        return;
    }
    let Ok((_spinner, mut editor, _disabled, _read_only, _binding)) = fields.fields.get_mut(field)
    else {
        return;
    };
    editor.pending_edits.retain(|edit| {
        !matches!(
            edit,
            TextEdit::Up(_) | TextEdit::Down(_) | TextEdit::TextStart(_) | TextEdit::TextEnd(_)
        )
    });
    for direction in steps {
        fields.step(field, direction);
    }
}

/// Grey and disable both arrows of every spinner whose field is
/// [disabled](InteractionDisabled) or [read-only](ReadOnlyField), and give
/// them back when the field comes back — so a consumer gates the spinner by
/// gating its field, as it always has. Writes only on a change.
fn mirror_field_gate(
    fields: Query<(&Spinner, Has<InteractionDisabled>, Has<ReadOnlyField>)>,
    arrows: Query<Has<InteractionDisabled>, With<SpinArrow>>,
    mut commands: Commands,
) {
    for (spinner, disabled, read_only) in &fields {
        let gated = disabled || read_only;
        for arrow in [spinner.up, spinner.down] {
            let Ok(arrow_gated) = arrows.get(arrow) else {
                continue;
            };
            if arrow_gated == gated {
                continue;
            }
            if gated {
                commands.entity(arrow).insert(InteractionDisabled);
            } else {
                commands.entity(arrow).remove::<InteractionDisabled>();
            }
        }
    }
}

/// Keep a spin button's announced range current when a consumer re-ranges it.
fn sync_spin_accessibility(
    mut fields: Query<(&Spinner, &mut AccessibilityNode), Changed<Spinner>>,
) {
    for (spinner, mut node) in &mut fields {
        node.0 = spin_accessibility(&spinner.step);
    }
}

/// The spinner's runtime half: the [`SpinnerStepped`] message, the arrows'
/// hold-to-repeat, the arrow keys, the shared gate and the announced range.
/// Add it with [`ensure_spinner_widget`]; [`TextInputPlugin`] already does, so
/// every host with text fields has working spinners.
///
/// [`TextInputPlugin`]: crate::ui_text_input::TextInputPlugin
#[derive(Debug, Clone, Copy, Default)]
pub struct SpinnerPlugin;

impl Plugin for SpinnerPlugin {
    fn build(&self, app: &mut App) {
        ensure_hold_repeat(app);
        app.add_message::<SpinnerStepped>();
        // What the step and the commit input read, which a host without the
        // input or focus plugins (a headless test, the gallery) would not
        // otherwise have; each is a no-op where it already exists.
        app.add_message::<KeyboardInput>();
        app.init_resource::<ButtonInput<KeyCode>>();
        app.init_resource::<InputFocus>();
        app.init_resource::<FontCx>();
        app.init_resource::<LayoutCx>();
        app.add_systems(
            PostUpdate,
            (
                // Before `bevy_text` drains the frame's edits, so the arrow
                // keys' caret moves are gone before they apply.
                step_spinners_from_arrow_keys.before(EditableTextSystems),
                mirror_field_gate,
                sync_spin_accessibility,
            ),
        );
    }
}

/// Add [`SpinnerPlugin`] to `app` unless something already has — the one line
/// a plugin whose systems read [`SpinnerStepped`] calls from its own `build`.
pub fn ensure_spinner_widget(app: &mut App) {
    if !app.is_plugin_added::<SpinnerPlugin>() {
        app.add_plugins(SpinnerPlugin);
    }
}

/// Why a field is being committed this frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommitCause {
    /// `Enter` in the focused field.
    Enter,
    /// The keyboard focus left the field.
    Blur,
    /// A spinner arrow or arrow key stepped it.
    Step,
}

/// One field to commit, and why.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FieldCommit {
    /// The field.
    pub field: Entity,
    /// What asked for the commit.
    pub cause: CommitCause,
    /// What a step left in the field, for a [`CommitCause::Step`].
    stepped: Option<String>,
}

impl FieldCommit {
    /// The text to commit: what the step produced for a step, the editor's
    /// own text otherwise.
    #[must_use]
    pub fn text(&self, editor: &EditableText) -> String {
        self.stepped
            .clone()
            .unwrap_or_else(|| editor.value().to_string())
    }

    /// Whether this commit is an explicit one (`Enter` or a step) rather than
    /// focus merely moving on — a consumer that commits a blur only when the
    /// text changed commits these regardless.
    #[must_use]
    pub fn is_explicit(&self) -> bool {
        self.cause != CommitCause::Blur
    }
}

/// The commit input of a panel of numeric fields: which of them to commit
/// this frame — the focused one on `Enter`, the one focus just left, and every
/// one a spinner stepped — so each consumer's commit system shares one reading
/// of the three, the reference's `onCommit` from the line editor and from the
/// spinner alike.
///
/// It keeps its own "focused last frame", so each system using it tracks blur
/// independently.
#[derive(SystemParam)]
pub struct FieldCommits<'w, 's> {
    /// The keyboard focus now.
    focus: Res<'w, InputFocus>,
    /// The keys, for `Enter`.
    keyboard: Res<'w, ButtonInput<KeyCode>>,
    /// The panel field focused last frame.
    last: Local<'s, Option<Entity>>,
    /// The steps since this system last read.
    steps: MessageReader<'w, 's, SpinnerStepped>,
}

impl std::fmt::Debug for FieldCommits<'_, '_> {
    /// Manual, because the system parameters it holds are not `Debug`: what is
    /// worth seeing is the field it last saw focused.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FieldCommits")
            .field("last", &*self.last)
            .finish_non_exhaustive()
    }
}

impl FieldCommits<'_, '_> {
    /// The commits due this frame among the fields `is_field` accepts: the
    /// `Enter` or blur one first, then the steps in the order they happened.
    pub fn take(&mut self, is_field: impl Fn(Entity) -> bool) -> Vec<FieldCommit> {
        let focused = self.focus.get().filter(|entity| is_field(*entity));
        let enter = self.keyboard.just_pressed(KeyCode::Enter)
            || self.keyboard.just_pressed(KeyCode::NumpadEnter);
        let mut commits = Vec::new();
        if enter {
            if let Some(field) = focused {
                commits.push(FieldCommit {
                    field,
                    cause: CommitCause::Enter,
                    stepped: None,
                });
            }
        } else if *self.last != focused
            && let Some(field) = self.last.filter(|entity| is_field(*entity))
        {
            commits.push(FieldCommit {
                field,
                cause: CommitCause::Blur,
                stepped: None,
            });
        }
        *self.last = focused;
        for step in self.steps.read() {
            if is_field(step.field) {
                commits.push(FieldCommit {
                    field: step.field,
                    cause: CommitCause::Step,
                    stepped: Some(step.text.clone()),
                });
            }
        }
        commits
    }

    /// Forget the focus and any pending steps — for a panel that is not live
    /// (the build tool closed), so neither turns into a commit when it is.
    pub fn reset(&mut self) {
        *self.last = None;
        self.steps.clear();
    }
}

// ---------------------------------------------------------------------------
// Gallery specimen.
// ---------------------------------------------------------------------------

/// Spawn the spinner specimen: a decimal spinner stepping by a hundredth
/// within ±10, the build floater's position fields' shape. The value stays
/// literal — a number is not translated.
pub fn spawn_spinner_specimen(commands: &mut Commands, parent: Entity, cx: ElementCx) -> Entity {
    spawn_spinner(
        commands,
        parent,
        &SpinnerSpec {
            input: TextInputSpec {
                initial: "1.250".to_owned(),
                font_size: cx.font_size,
                width_glyphs: 8.0,
                name_key: Some("spinner-specimen-name"),
                ..TextInputSpec::new("spinner", TextInputKind::Float)
            },
            step: SpinStep::new(0.01, -10.0, 10.0, 3),
        },
    )
    .root
}

#[cfg(test)]
mod tests {
    use super::{SpinDirection, SpinStep, format_spin_value, round_to_decimals, stepped_value};
    use pretty_assertions::assert_eq;

    /// The position fields' step: a centimetre, three decimals.
    const POSITION: SpinStep = SpinStep::new(0.01, -256.0, 512.0, 3);

    /// One unmodified step moves by the increment, both ways, and rounds to
    /// the shown decimals rather than accumulating float error.
    #[test]
    fn a_step_moves_by_the_increment_and_rounds() {
        assert_eq!(
            stepped_value("128.000", &POSITION, SpinDirection::Up, 1.0),
            Some(128.01)
        );
        assert_eq!(
            stepped_value("128.000", &POSITION, SpinDirection::Down, 1.0),
            Some(127.99)
        );
        assert_eq!(
            stepped_value("0.1", &POSITION, SpinDirection::Up, 1.0)
                .map(|value| format_spin_value(value, 3)),
            Some("0.110".to_owned())
        );
    }

    /// The modifiers scale the step: ×10 and ×0.01 here.
    #[test]
    fn a_scaled_step_moves_by_the_scaled_increment() {
        assert_eq!(
            stepped_value("1.000", &POSITION, SpinDirection::Up, 10.0),
            Some(1.1)
        );
        // A finer step than the shown decimals rounds away entirely — as the
        // reference's does, whose `clamp_precision` runs after the add.
        assert_eq!(
            stepped_value("1.000", &POSITION, SpinDirection::Up, 0.01),
            Some(1.0)
        );
    }

    /// A step clamps to the range at both ends.
    #[test]
    fn a_step_clamps_to_the_range() {
        let unit = SpinStep::new(0.25, 0.0, 1.0, 2);
        assert_eq!(
            stepped_value("0.9", &unit, SpinDirection::Up, 1.0),
            Some(1.0)
        );
        assert_eq!(
            stepped_value("0.1", &unit, SpinDirection::Down, 1.0),
            Some(0.0)
        );
        // A value already out of range comes back into it.
        assert_eq!(
            stepped_value("5", &unit, SpinDirection::Down, 1.0),
            Some(1.0)
        );
    }

    /// A field holding no number does not step — the empty field of an empty
    /// selection, a lone `-` mid-typing.
    #[test]
    fn a_field_holding_no_number_does_not_step() {
        for text in ["", "-", ".", "abc"] {
            assert_eq!(
                stepped_value(text, &POSITION, SpinDirection::Up, 1.0),
                None,
                "{text:?}"
            );
        }
    }

    /// An integer spinner shows no decimals, and no value is shown as a
    /// negative zero.
    #[test]
    fn values_format_without_a_negative_zero() {
        assert_eq!(format_spin_value(42.0, 0), "42");
        assert_eq!(format_spin_value(-0.0004, 3), "0.000");
        assert_eq!(format_spin_value(-0.4, 0), "0");
        assert_eq!(format_spin_value(-1.5, 1), "-1.5");
    }

    /// Rounding is to the requested decimals.
    #[expect(
        clippy::float_cmp,
        reason = "each expected value is the exact f64 the rounding must produce"
    )]
    #[test]
    fn rounding_is_to_the_decimals() {
        assert_eq!(round_to_decimals(1.23456, 2), 1.23);
        assert_eq!(round_to_decimals(1.235_000_1, 2), 1.24);
        assert_eq!(round_to_decimals(7.5, 0), 8.0);
    }
}

#[cfg(test)]
mod typed_tests {
    use super::{SpinStep, SpinnerPlugin, SpinnerSpec, SpinnerStepped, spawn_spinner};
    use bevy::input::keyboard::Key;
    use bevy::prelude::*;
    use pretty_assertions::assert_eq;

    use crate::ui_test::interact::{self, InteractionTest};
    use crate::ui_test::{TestError, settle};
    use crate::ui_text_input::{TextInputKind, TextInputPlugin, TextInputSpec};
    use sl_viewer_ui_core::ui::{UiRoot, UiScaffoldSystems};

    /// The spinner's field.
    const FIELD: &str = "spin:field";

    /// Its up arrow.
    const UP: &str = "spin:up";

    /// Its down arrow.
    const DOWN: &str = "spin:down";

    /// Every step announced so far, in order.
    #[derive(Resource, Debug, Default)]
    struct Announced(Vec<String>);

    /// Record every [`SpinnerStepped`].
    fn record(mut steps: MessageReader<SpinnerStepped>, mut announced: ResMut<Announced>) {
        announced
            .0
            .extend(steps.read().map(|step| step.text.clone()));
    }

    /// An interaction app holding one spinner starting at `initial`, stepping
    /// by a quarter within 0..=2 at two decimals.
    fn spinner_app(initial: &str) -> App {
        let mut app = InteractionTest::new().build();
        app.add_plugins(TextInputPlugin);
        app.init_resource::<Announced>();
        app.add_systems(Update, record);
        let initial = initial.to_owned();
        app.add_systems(
            Startup,
            (move |mut commands: Commands, root: Res<UiRoot>| {
                spawn_spinner(
                    &mut commands,
                    root.0,
                    &SpinnerSpec {
                        input: TextInputSpec {
                            initial: initial.clone(),
                            ..TextInputSpec::new("spin", TextInputKind::Float)
                        },
                        step: SpinStep::new(0.25, 0.0, 2.0, 2),
                    },
                );
            })
            .after(UiScaffoldSystems::SpawnRoot),
        );
        settle(&mut app);
        app
    }

    /// The field's text now.
    fn value(app: &mut App) -> String {
        interact::text_of(app, FIELD).unwrap_or_default()
    }

    /// What has been announced.
    fn announced(app: &App) -> Vec<String> {
        app.world()
            .get_resource::<Announced>()
            .map(|announced| announced.0.clone())
            .unwrap_or_default()
    }

    /// **A click on an arrow steps the field once and announces it**, and the
    /// other arrow steps it back.
    #[test]
    fn an_arrow_click_steps_and_announces() -> Result<(), TestError> {
        let mut app = spinner_app("1.00");
        interact::click_node(&mut app, UP)?;
        settle(&mut app);
        assert_eq!(value(&mut app), "1.25");
        interact::click_node(&mut app, DOWN)?;
        interact::click_node(&mut app, DOWN)?;
        settle(&mut app);
        assert_eq!(value(&mut app), "0.75");
        assert_eq!(announced(&app), ["1.25", "1.00", "0.75"]);
        Ok(())
    }

    /// **A step stops at the range's end**, and still announces where it is.
    #[test]
    fn a_step_stops_at_the_end_of_the_range() -> Result<(), TestError> {
        let mut app = spinner_app("1.90");
        interact::click_node(&mut app, UP)?;
        settle(&mut app);
        assert_eq!(value(&mut app), "2.00");
        Ok(())
    }

    /// **The arrow keys step the focused field**, and do not move its caret
    /// instead.
    #[test]
    fn the_arrow_keys_step_the_focused_field() -> Result<(), TestError> {
        let mut app = spinner_app("0.50");
        interact::click_node(&mut app, FIELD)?;
        interact::tap(&mut app, KeyCode::ArrowUp, Key::ArrowUp);
        settle(&mut app);
        assert_eq!(value(&mut app), "0.75");
        interact::tap(&mut app, KeyCode::ArrowDown, Key::ArrowDown);
        interact::tap(&mut app, KeyCode::ArrowDown, Key::ArrowDown);
        settle(&mut app);
        assert_eq!(value(&mut app), "0.25");
        Ok(())
    }

    /// **A disabled field's arrows are disabled too**, so a click does
    /// nothing — and they come back with the field.
    #[test]
    fn the_arrows_share_the_fields_gate() -> Result<(), TestError> {
        let mut app = spinner_app("1.00");
        let field = crate::ui_test::find_by_name(&mut app, FIELD).ok_or("no field")?;
        app.world_mut()
            .entity_mut(field)
            .insert(bevy::ui::InteractionDisabled);
        settle(&mut app);
        interact::click_node(&mut app, UP)?;
        settle(&mut app);
        assert_eq!(value(&mut app), "1.00", "a gated spinner does not step");
        assert!(announced(&app).is_empty());

        app.world_mut()
            .entity_mut(field)
            .remove::<bevy::ui::InteractionDisabled>();
        settle(&mut app);
        interact::click_node(&mut app, UP)?;
        settle(&mut app);
        assert_eq!(value(&mut app), "1.25", "and steps again once ungated");
        Ok(())
    }

    /// The plugin is what [`TextInputPlugin`] brings, so a host never has to
    /// know about it.
    #[test]
    fn text_input_plugin_brings_the_spinner_plugin() {
        let app = spinner_app("0");
        assert!(app.is_plugin_added::<SpinnerPlugin>());
    }
}
