//! The reusable **chat-input widget** (`viewer-ui-text-input-emoji` +
//! `viewer-emoji-colon-autocomplete`): a single-line text field with an **emoji
//! button** beside it that opens the picker for *this* field, and an inline
//! **`:`-completer** — the integrated text entry every chat surface (nearby chat,
//! IM, the conversations floater) is built on.
//!
//! # What it composes
//!
//! Nothing new under the hood — it wires together landed pieces around one
//! [`crate::ui_text_input`] single-line field:
//!
//! - the field is spawned **bare and filling** inside a bordered box (the same
//!   `decorated: false` / `fill: true` embedding the search field uses), so the
//!   box owns the chrome;
//! - an **emoji button** ([`crate::emoji_picker`]) on the trailing edge that, on
//!   press, opens the picker anchored to the click and **targets this field** —
//!   the field-side affordance `viewer-ui-text-input-emoji` asked for, realised as
//!   a reusable widget rather than a bare-field flag;
//! - the inline **`:`-completer** ([`crate::emoji_complete`]) attached to the
//!   field, its popup hung above the box.
//!
//! # What it emits
//!
//! Per the scaffold rule ([`crate::ui_element`]) the widget reaches no session: it
//! **emits a [`ChatInputSubmit`]** when the user presses `Enter` on a non-empty
//! field (carrying the text and whether `Shift` / `Ctrl` were held, for a consumer
//! that maps those to whisper / shout), and clears the field. Ordinary consumers
//! read that; the local-chat variant ([`crate::local_chat_input`]) builds on it.
//! `Enter` is handled **after** [`crate::emoji_complete::ColonCompleteSet`], so a
//! press the completer accepted a suggestion with is not also a send.
//!
//! Reference (Firestorm, read-only): `llchatentry` (the chat line editor with its
//! emoji button), `llemojihelper`.

use bevy::prelude::*;
use bevy::text::{EditableText, FontCx, LayoutCx};

use crate::emoji_complete::{ColonCompleteSet, attach_colon_complete};
use crate::emoji_picker::OpenEmojiPicker;
use crate::ui::row;
use crate::ui_font::UiFont;
use crate::ui_text_input::{TextInputKind, TextInputSpec, spawn_text_input};

/// The box's border colour.
const BOX_BORDER: Color = Color::srgb(0.30, 0.36, 0.46);

/// The box's background colour.
const BOX_BACKGROUND: Color = Color::srgb(0.10, 0.12, 0.16);

/// The typed-text colour.
const TEXT_COLOR: Color = Color::srgb(0.92, 0.94, 0.98);

/// The emoji button's glyph — a smiling face, as the reference chat bar shows.
const EMOJI_GLYPH: &str = "\u{1f642}";

/// The emoji button's background.
const EMOJI_BUTTON_BACKGROUND: Color = Color::srgba(1.0, 1.0, 1.0, 0.06);

/// The default least width of the box, in logical pixels.
const DEFAULT_MIN_WIDTH: f32 = 200.0;

/// The default font size, in logical pixels.
const DEFAULT_FONT_SIZE: f32 = 15.0;

/// The box's inner horizontal padding, in logical pixels.
const BOX_PADDING_X: f32 = 6.0;

/// The box's inner vertical padding, in logical pixels.
const BOX_PADDING_Y: f32 = 3.0;

/// The gap between the field and the emoji button, in logical pixels.
const INNER_GAP: f32 = 4.0;

/// A marker on the widget's inner field, so the submit system finds a chat field
/// among all editable fields, and a consumer can tell it from any other.
#[derive(Component, Debug, Clone, Copy)]
pub(crate) struct ChatInputField;

/// What [`spawn_chat_input`] hands back: the box and the inner field.
#[derive(Debug, Clone, Copy)]
pub(crate) struct ChatInputHandle {
    /// The bordered container — the chat *box*. A consumer (the local-chat
    /// variant) can parent siblings (a volume selector) into it.
    pub(crate) container: Entity,
    /// The inner [`EditableText`], whose value is the draft message.
    pub(crate) field: Entity,
}

/// A submitted chat line: the field it came from, its text, and whether `Shift` /
/// `Ctrl` were held on the `Enter` (a consumer maps those to whisper / shout).
/// Emitted by [`send_chat_input`] on a non-empty `Enter`, after which the field is
/// cleared.
#[derive(Message, Debug, Clone)]
pub(crate) struct ChatInputSubmit {
    /// The field the line was typed in.
    pub(crate) field: Entity,
    /// The text as typed (not trimmed — a consumer decides what to strip).
    pub(crate) text: String,
    /// Whether a `Shift` key was held on the `Enter`.
    pub(crate) shift: bool,
    /// Whether a `Ctrl` key was held on the `Enter`.
    pub(crate) ctrl: bool,
}

/// Everything a chat input is built from. Build one with [`ChatInputSpec::new`] and
/// override with struct-update syntax.
#[derive(Debug, Clone)]
pub(crate) struct ChatInputSpec {
    /// The prefix of the widget's node [`Name`]s, for the gallery and lookups.
    pub(crate) element: &'static str,
    /// The field's focus stop.
    pub(crate) tab_index: i32,
    /// The field text's font size, in logical pixels.
    pub(crate) font_size: f32,
    /// The box's least width, in logical pixels — a floor below which it will not
    /// shrink.
    pub(crate) min_width: f32,
    /// An explicit box width, or `None` (the default) to size to content above
    /// [`min_width`](Self::min_width). The nearby-chat bar sets a percentage so the
    /// bar spans a fraction of the screen.
    pub(crate) width: Option<Val>,
}

impl ChatInputSpec {
    /// A spec for `element` with the module defaults.
    pub(crate) const fn new(element: &'static str) -> Self {
        Self {
            element,
            tab_index: 0,
            font_size: DEFAULT_FONT_SIZE,
            min_width: DEFAULT_MIN_WIDTH,
            width: None,
        }
    }
}

/// Spawn a chat input under `parent`, returning the box and inner field.
///
/// The box is a bordered [`crate::ui::row`] of a bare, filling single-line field
/// and a trailing emoji button; the field carries [`ChatInputField`] and an
/// attached [`crate::emoji_complete`] popup (hung above the box). The emoji button
/// opens the picker for this field.
pub(crate) fn spawn_chat_input(
    commands: &mut Commands,
    parent: Entity,
    spec: &ChatInputSpec,
) -> ChatInputHandle {
    let container = commands
        .spawn((
            Node {
                align_items: AlignItems::Center,
                width: spec.width.unwrap_or(Val::Auto),
                min_width: Val::Px(spec.min_width),
                border: UiRect::all(Val::Px(1.0)),
                padding: UiRect::axes(Val::Px(BOX_PADDING_X), Val::Px(BOX_PADDING_Y)),
                // The completer popup is an absolute child positioned above the
                // box; a relative container gives it its origin.
                position_type: PositionType::Relative,
                ..row(Val::Px(INNER_GAP))
            },
            BorderColor::all(BOX_BORDER),
            BackgroundColor(BOX_BACKGROUND),
            Name::new(format!("{}:chat-input", spec.element)),
            ChildOf(parent),
        ))
        .id();

    // The field slot: fills the middle, scrolls its own text.
    let slot = commands
        .spawn((
            Node {
                flex_grow: 1.0,
                min_width: Val::Px(0.0),
                align_items: AlignItems::Center,
                ..default()
            },
            Name::new(format!("{}:chat-slot", spec.element)),
            ChildOf(container),
        ))
        .id();

    let field = spawn_text_input(
        commands,
        slot,
        &TextInputSpec {
            tab_index: spec.tab_index,
            font_size: spec.font_size,
            decorated: false,
            fill: true,
            ..TextInputSpec::new(spec.element, TextInputKind::Line)
        },
    );
    commands
        .entity(field)
        .insert((ChatInputField, TextColor(TEXT_COLOR)));

    // The completer popup hangs above the whole box.
    attach_colon_complete(commands, field, container);

    // The trailing emoji button — opens the picker anchored to the click, targeting
    // this field.
    spawn_emoji_button(commands, container, field, spec);

    ChatInputHandle { container, field }
}

/// Spawn the trailing emoji button and wire its press to open the picker for
/// `field`, anchored at the click point.
fn spawn_emoji_button(
    commands: &mut Commands,
    container: Entity,
    field: Entity,
    spec: &ChatInputSpec,
) {
    let button = commands
        .spawn((
            Node {
                flex_shrink: 0.0,
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                padding: UiRect::all(Val::Px(2.0)),
                ..default()
            },
            BackgroundColor(EMOJI_BUTTON_BACKGROUND),
            Pickable::default(),
            Name::new(format!("{}:chat-emoji-button", spec.element)),
            ChildOf(container),
        ))
        .with_child((
            Text::new(EMOJI_GLYPH),
            UiFont::Sans.at(spec.font_size),
            TextColor(TEXT_COLOR),
            Pickable::IGNORE,
        ))
        .id();
    commands.entity(button).observe(
        move |mut press: On<Pointer<Press>>, mut open: MessageWriter<OpenEmojiPicker>| {
            press.propagate(false);
            if press.button != PointerButton::Primary {
                return;
            }
            open.write(OpenEmojiPicker {
                field,
                near: press.pointer_location.position,
            });
        },
    );
}

/// The chat-input widget's runtime: the `Enter`-to-send. Ordered after
/// [`ColonCompleteSet`] so a press the completer accepted a suggestion with does
/// not also send.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct ChatInputPlugin;

impl Plugin for ChatInputPlugin {
    /// Register the send message and system.
    fn build(&self, app: &mut App) {
        app.add_message::<ChatInputSubmit>()
            .add_systems(Update, send_chat_input.after(ColonCompleteSet));
    }
}

/// Send the focused chat field's line on a non-empty `Enter`: emit a
/// [`ChatInputSubmit`] carrying the text and the `Shift` / `Ctrl` modifiers, then
/// clear the field. A blank line is a no-op (no send, no clear).
fn send_chat_input(
    mut keyboard: ResMut<ButtonInput<KeyCode>>,
    focus: Res<bevy::input_focus::InputFocus>,
    mut fields: Query<&mut EditableText, With<ChatInputField>>,
    mut out: MessageWriter<ChatInputSubmit>,
    mut font_cx: ResMut<FontCx>,
    mut layout_cx: ResMut<LayoutCx>,
) {
    if !keyboard.just_pressed(KeyCode::Enter) {
        return;
    }
    let Some(focused) = focus.get() else {
        return;
    };
    let Ok(mut editable) = fields.get_mut(focused) else {
        return;
    };
    let text = editable.value().to_string();
    if text.trim().is_empty() {
        return;
    }
    let shift = keyboard.pressed(KeyCode::ShiftLeft) || keyboard.pressed(KeyCode::ShiftRight);
    let ctrl = keyboard.pressed(KeyCode::ControlLeft) || keyboard.pressed(KeyCode::ControlRight);
    out.write(ChatInputSubmit {
        field: focused,
        text,
        shift,
        ctrl,
    });
    // Clear the field for the next line, caret at the start.
    editable.editor.set_text("");
    let mut driver = editable.editor.driver(&mut font_cx, &mut layout_cx);
    driver.refresh_layout();
    driver.move_to_text_start();
    // Consume the Enter so nothing downstream re-sends it.
    keyboard.clear_just_pressed(KeyCode::Enter);
}

// ---------------------------------------------------------------------------
// Registry specimen
// ---------------------------------------------------------------------------

/// Spawn the **live** chat-input specimen for the gallery / harness (the pattern
/// [`crate::ui_text_input`] and [`crate::ui_search`] use): the real widget, so its
/// box, emoji button and completer layout are swept across every script, size and
/// direction, and it is genuinely usable in the gallery. Its runtime is inert in
/// the harness (which adds none of the widget plugins) and live in the gallery
/// (which adds them all).
pub(crate) fn spawn_chat_input_specimen(
    commands: &mut Commands,
    parent: Entity,
    cx: crate::ui_element::ElementCx,
) -> Entity {
    spawn_chat_input(
        commands,
        parent,
        &ChatInputSpec {
            font_size: cx.font_size,
            ..ChatInputSpec::new("chat-input")
        },
    )
    .container
}

#[cfg(test)]
mod tests {
    use super::{
        ChatInputField, ChatInputPlugin, ChatInputSpec, ChatInputSubmit, spawn_chat_input,
    };
    use crate::emoji_complete::ColonCompletePlugin;
    use crate::ui::{UiRoot, UiScaffoldSystems};
    use crate::ui_test::{LayoutTest, TestError, find_by_name, settle};
    use bevy::input_focus::{FocusCause, InputFocus};
    use bevy::prelude::*;
    use bevy::text::EditableText;
    use pretty_assertions::assert_eq;

    /// Build a layout-test app with a chat input and the widget systems, plus the
    /// keyboard resource the layout harness omits and the emoji-picker message the
    /// button writes.
    fn build_app() -> App {
        let mut app = LayoutTest::new().build();
        app.init_resource::<ButtonInput<KeyCode>>()
            .add_message::<crate::emoji_picker::OpenEmojiPicker>()
            .add_plugins((ColonCompletePlugin, ChatInputPlugin))
            .add_systems(
                Startup,
                (|mut commands: Commands, root: Res<UiRoot>| {
                    spawn_chat_input(&mut commands, root.0, &ChatInputSpec::new("test-chat"));
                })
                .after(UiScaffoldSystems::SpawnRoot),
            );
        settle(&mut app);
        app
    }

    /// Pressing `Enter` on a non-empty focused field emits a submit carrying the
    /// text and modifiers, and clears the field; a blank line does neither.
    #[test]
    fn enter_sends_a_non_empty_line_and_clears() -> Result<(), TestError> {
        let mut app = build_app();
        let field = find_by_name(&mut app, "test-chat:field").ok_or("field did not spawn")?;

        // Type into the field and focus it.
        {
            let mut entity = app.world_mut().entity_mut(field);
            let mut editable = entity
                .get_mut::<EditableText>()
                .ok_or("field lost EditableText")?;
            editable.editor.set_text("hello");
        }
        app.world_mut()
            .resource_mut::<InputFocus>()
            .set(field, FocusCause::Navigated);
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::Enter);
        settle(&mut app);

        let submitted: Vec<String> = app
            .world_mut()
            .resource_mut::<Messages<ChatInputSubmit>>()
            .drain()
            .map(|submit| submit.text)
            .collect();
        assert_eq!(submitted, vec!["hello".to_owned()]);
        let value = app
            .world()
            .entity(field)
            .get::<EditableText>()
            .ok_or("field lost EditableText")?
            .value()
            .to_string();
        assert!(value.is_empty(), "the field is cleared after send");
        Ok(())
    }

    /// The inner field carries the [`ChatInputField`] marker so the send system
    /// finds it.
    #[test]
    fn field_is_marked() -> Result<(), TestError> {
        let mut app = build_app();
        let field = find_by_name(&mut app, "test-chat:field").ok_or("field did not spawn")?;
        assert!(app.world().entity(field).contains::<ChatInputField>());
        Ok(())
    }
}
