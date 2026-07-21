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
//! # Line-recall history (`viewer-chat-input-history`)
//!
//! Every field carries a per-field [`ChatInputHistory`]: each submitted line is
//! pushed onto it, and **`Ctrl+Up`** / **`Ctrl+Down`** walk back and forth through
//! it, replacing the field text (the reference's chat-bar recall). The `Ctrl`
//! modifier keeps a bare `Up`/`Down` free for the `:`-completer popup and any
//! caret movement. Stepping forward past the newest entry restores the draft that
//! was in progress when recall began, so a stray `Ctrl+Up` is undoable. Because it
//! lives on the base widget, every consumer — the nearby-chat bar, the local-chat
//! variant, each Conversations-floater tab — gets it for free.
//!
//! Reference (Firestorm, read-only): `llchatentry` (the chat line editor with its
//! emoji button), `llemojihelper`, `lllineeditor` (the up/down history recall).

use std::collections::VecDeque;

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

/// How many submitted lines a field's recall history keeps before the oldest is
/// evicted — a few dozen, the reference's `LINE_HISTORY_MAX` order of magnitude.
const HISTORY_CAP: usize = 32;

/// A field's **line-recall history**: the lines submitted from it, plus the recall
/// cursor and the saved in-progress draft. One per field (inserted by
/// [`spawn_chat_input`]); [`recall_chat_history`] drives it from `Ctrl+Up` /
/// `Ctrl+Down` and [`send_chat_input`] pushes each sent line.
///
/// The logic is pure and unit-tested — the system is only the keyboard-and-field
/// glue over it.
#[derive(Component, Debug, Default, Clone)]
pub(crate) struct ChatInputHistory {
    /// The submitted lines, oldest at the front, newest at the back.
    entries: VecDeque<String>,
    /// The recalled entry's index while walking history, or `None` when the field
    /// shows the live draft (recall not active).
    cursor: Option<usize>,
    /// The in-progress text saved when recall started, restored when the walk
    /// steps forward past the newest entry.
    draft: String,
}

impl ChatInputHistory {
    /// Record a submitted `line`, ending any active recall. A line identical to
    /// the newest entry is not duplicated (the reference skips consecutive
    /// repeats); the history is bounded to [`HISTORY_CAP`], evicting the oldest.
    fn push(&mut self, line: &str) {
        self.cursor = None;
        self.draft.clear();
        if self.entries.back().map(String::as_str) == Some(line) {
            return;
        }
        self.entries.push_back(line.to_owned());
        while self.entries.len() > HISTORY_CAP {
            self.entries.pop_front();
        }
    }

    /// Walk one step **back** (older) through the history, saving `current` as the
    /// draft when recall first starts. Returns the text to place in the field, or
    /// `None` when there is nothing older to show (empty history, or already at
    /// the oldest entry).
    fn recall_older(&mut self, current: &str) -> Option<String> {
        if self.entries.is_empty() {
            return None;
        }
        let index = match self.cursor {
            None => {
                current.clone_into(&mut self.draft);
                self.entries.len().saturating_sub(1)
            }
            Some(0) => return None,
            Some(current_index) => current_index.saturating_sub(1),
        };
        self.cursor = Some(index);
        self.entries.get(index).cloned()
    }

    /// Walk one step **forward** (newer) through the history. Returns the text to
    /// place in the field — the next entry, or the saved draft once the walk steps
    /// past the newest entry — or `None` when recall is not active.
    fn recall_newer(&mut self) -> Option<String> {
        let current_index = self.cursor?;
        let newest = self.entries.len().saturating_sub(1);
        if current_index < newest {
            let index = current_index.saturating_add(1);
            self.cursor = Some(index);
            self.entries.get(index).cloned()
        } else {
            self.cursor = None;
            Some(self.draft.clone())
        }
    }
}

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
    commands.entity(field).insert((
        ChatInputField,
        ChatInputHistory::default(),
        TextColor(TEXT_COLOR),
    ));

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
    /// Register the send message and the send / history-recall systems.
    fn build(&self, app: &mut App) {
        app.add_message::<ChatInputSubmit>().add_systems(
            Update,
            (send_chat_input, recall_chat_history).after(ColonCompleteSet),
        );
    }
}

/// Send the focused chat field's line on a non-empty `Enter`: emit a
/// [`ChatInputSubmit`] carrying the text and the `Shift` / `Ctrl` modifiers, push
/// the line onto the field's recall history, then clear the field. A blank line is
/// a no-op (no send, no clear).
fn send_chat_input(
    mut keyboard: ResMut<ButtonInput<KeyCode>>,
    focus: Res<bevy::input_focus::InputFocus>,
    mut fields: Query<(&mut EditableText, &mut ChatInputHistory), With<ChatInputField>>,
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
    let Ok((mut editable, mut history)) = fields.get_mut(focused) else {
        return;
    };
    let text = editable.value().to_string();
    if text.trim().is_empty() {
        return;
    }
    let shift = keyboard.pressed(KeyCode::ShiftLeft) || keyboard.pressed(KeyCode::ShiftRight);
    let ctrl = keyboard.pressed(KeyCode::ControlLeft) || keyboard.pressed(KeyCode::ControlRight);
    history.push(&text);
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

/// Recall the focused chat field's history: `Ctrl+Up` steps back to older
/// submitted lines, `Ctrl+Down` forward toward the live draft, replacing the field
/// text and parking the caret at the end. The `Ctrl` modifier leaves a bare
/// `Up`/`Down` for the `:`-completer popup and caret movement; a step that has
/// nothing to show is a no-op.
fn recall_chat_history(
    mut keyboard: ResMut<ButtonInput<KeyCode>>,
    focus: Res<bevy::input_focus::InputFocus>,
    mut fields: Query<(&mut EditableText, &mut ChatInputHistory), With<ChatInputField>>,
    mut font_cx: ResMut<FontCx>,
    mut layout_cx: ResMut<LayoutCx>,
) {
    let ctrl = keyboard.pressed(KeyCode::ControlLeft) || keyboard.pressed(KeyCode::ControlRight);
    if !ctrl {
        return;
    }
    let older = keyboard.just_pressed(KeyCode::ArrowUp);
    let newer = keyboard.just_pressed(KeyCode::ArrowDown);
    if !(older || newer) {
        return;
    }
    let Some(focused) = focus.get() else {
        return;
    };
    let Ok((mut editable, mut history)) = fields.get_mut(focused) else {
        return;
    };
    let replacement = if older {
        let current = editable.value().to_string();
        history.recall_older(&current)
    } else {
        history.recall_newer()
    };
    let Some(text) = replacement else {
        return;
    };
    editable.editor.set_text(&text);
    let mut driver = editable.editor.driver(&mut font_cx, &mut layout_cx);
    driver.refresh_layout();
    driver.move_to_text_end();
    // Consume the arrow so the completer / caret does not also act on it.
    keyboard.clear_just_pressed(if older {
        KeyCode::ArrowUp
    } else {
        KeyCode::ArrowDown
    });
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
        ChatInputField, ChatInputHistory, ChatInputPlugin, ChatInputSpec, ChatInputSubmit,
        HISTORY_CAP, spawn_chat_input,
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

    /// A fresh recall walks the newest-first history and back to the draft.
    #[test]
    fn history_recall_walks_back_and_forward_to_the_draft() {
        let mut history = ChatInputHistory::default();
        history.push("first");
        history.push("second");
        // Ctrl+Up from a live draft saves it and shows the newest line, then older.
        assert_eq!(history.recall_older("draft"), Some("second".to_owned()));
        assert_eq!(history.recall_older("draft"), Some("first".to_owned()));
        // Nothing older than the oldest entry.
        assert_eq!(history.recall_older("draft"), None);
        // Ctrl+Down walks forward, and past the newest restores the saved draft.
        assert_eq!(history.recall_newer(), Some("second".to_owned()));
        assert_eq!(history.recall_newer(), Some("draft".to_owned()));
        // Recall is no longer active.
        assert_eq!(history.recall_newer(), None);
    }

    /// Recall on an empty history, or forward with no active recall, is a no-op.
    #[test]
    fn history_recall_no_ops_when_nothing_to_show() {
        let mut history = ChatInputHistory::default();
        assert_eq!(history.recall_older("draft"), None);
        assert_eq!(history.recall_newer(), None);
    }

    /// A submitted line is not duplicated when it repeats the newest entry, and the
    /// history is bounded to the cap (oldest evicted).
    #[test]
    fn history_dedupes_repeats_and_is_bounded() {
        let mut history = ChatInputHistory::default();
        history.push("same");
        history.push("same");
        // The consecutive repeat did not grow the history: one step back, then none.
        assert_eq!(history.recall_older(""), Some("same".to_owned()));
        assert_eq!(history.recall_older(""), None);

        let mut history = ChatInputHistory::default();
        for index in 0..(HISTORY_CAP + 10) {
            history.push(&format!("line {index}"));
        }
        // Only the cap's worth survive: the newest is the last pushed, and walking
        // all the way back reaches exactly HISTORY_CAP entries.
        assert_eq!(
            history.recall_older(""),
            Some(format!("line {}", HISTORY_CAP + 9))
        );
        let mut count = 1;
        while history.recall_older("").is_some() {
            count += 1;
        }
        assert_eq!(count, HISTORY_CAP);
    }

    /// A new submission ends any active recall (cursor resets to the live draft).
    #[test]
    fn pushing_ends_active_recall() {
        let mut history = ChatInputHistory::default();
        history.push("old");
        assert_eq!(history.recall_older("draft"), Some("old".to_owned()));
        // Submitting while recalling resets to draft-mode, so a forward step now
        // finds no active recall.
        history.push("new");
        assert_eq!(history.recall_newer(), None);
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
