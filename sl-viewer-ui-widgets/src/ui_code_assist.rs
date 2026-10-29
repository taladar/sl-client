//! What a **code editor offers** while it is typed in
//! (`viewer-lsl-editor-completion`): a list of completions for the word at the
//! caret, the signature of the call the caret is in, and a description of the
//! word under the pointer.
//!
//! As with highlighting ([`crate::ui_code_syntax`]), the language is not this
//! module's business: a consumer puts a [`CodeAssistant`] on the editor's
//! field — an [`sl_highlight::CodeAssist`] — and the editor asks it at the
//! caret and draws what comes back.
//!
//! # Completions
//!
//! Typing a word opens the list, and every keystroke after refilters it; the
//! list closes when nothing matches, when the caret leaves the word, or on
//! `Escape`. `Ctrl+Space` opens it with nothing typed. While it is open `Up` and
//! `Down` move through it, and `Enter` or `Tab` — or a click — puts the
//! selected entry in place of the word. `Tab` indents again the moment the list
//! is closed. The selected entry's signature and documentation sit under the
//! list.
//!
//! # Signature help
//!
//! Typing `(` or `,` in a call shows the called function's signature over the
//! caret with the argument being typed emphasised, its documentation and what
//! else is worth knowing (a forced sleep, a deprecation). It follows the caret
//! while the caret stays in the call, and `Escape` (or
//! `Ctrl+Shift+Space` to bring it back) dismisses it.
//!
//! # Hover
//!
//! The pointer resting on a word for a moment shows what the language says
//! about it, as a tooltip under the word; it goes when the pointer moves off
//! the word or the buffer changes.
//!
//! Reference: Firestorm offers none of this; the shapes are the Language Server
//! Protocol's, which a later language server for Lua can fill the same way.

use core::fmt;
use core::ops::Range;
use std::sync::Arc;

use bevy::input::keyboard::{Key, KeyboardInput};
use bevy::input_focus::{FocusedInput, InputFocus};
use bevy::prelude::*;
use bevy::text::{EditableText, EditableTextSystems, TextEdit};
use bevy::ui::UiSystems;
use bevy::ui::widget::TextScroll;
use bevy::ui_widgets::popover::{Popover, PopoverAlign, PopoverPlacement, PopoverSide};
use bevy_flair::style::components::ClassList;
use sl_highlight::{CodeAssist, Completion, Completions, Hover, Note, Signature};

use sl_viewer_ui_core::i18n::{TransArgs, Translator};
use sl_viewer_ui_core::semantic::{Role, Semantic};
use sl_viewer_ui_core::skin::{
    LIST_ROW_CLASS, SELECTED_CLASS, TEXT_CLASS, TOOLTIP_TEXT, TOOLTIP_TEXT_CLASS, set_state_class,
    tooltip_box,
};
use sl_viewer_ui_core::skin_palette::SkinPalette;
use sl_viewer_ui_core::ui_font::UiFont;

use crate::ui_code_syntax::{DEPRECATED_CLASS, syntax_class};
use crate::ui_rich_text::{
    RichTextField, RichTextGeometry, backdrop_rects, narrow, rich_text_point,
};
use crate::ui_text_history::ChordModifiers;
use crate::ui_text_input::ReadOnlyField;

/// The skin class of the completion list's face, besides the drop-down list's
/// own (`sk-combo-list`): what re-roots a plain entry's text to the field
/// family, as a list's face does.
pub const ASSIST_LIST_CLASS: &str = "sk-code-assist-list";

/// The skin class of the line under the list that gives the selected entry's
/// signature (`--line-number`).
pub const ASSIST_DETAIL_CLASS: &str = "sk-code-assist-detail";

/// The skin class of the parameter being typed in a signature (`--accent`).
pub const ASSIST_ACTIVE_CLASS: &str = "sk-code-assist-active";

/// The drop-down list's face, which the completion list wears.
const DROP_DOWN_CLASS: &str = "sk-combo-list";

/// How many entries the list shows at once; it scrolls to keep the selected
/// one in view.
const VISIBLE_ROWS: usize = 10;

/// The completion list's z-order: above the combos' drop-downs (`9_500`), below
/// the menus (`10_000`).
const ASSIST_Z: i32 = 9_600;

/// How long the pointer rests on a word before its description shows, in
/// seconds.
const HOVER_DELAY: f32 = 0.6;

/// The widest the list's documentation and a tip grow before they wrap, in
/// logical pixels.
const DOC_WIDTH: f32 = 460.0;

/// The documentation's font size, in logical pixels.
const DOC_FONT_SIZE: f32 = 13.0;

/// A language's help, on a code editor's field.
#[derive(Component, Clone)]
pub struct CodeAssistant(Arc<dyn CodeAssist>);

impl CodeAssistant {
    /// The help of `assist`.
    pub fn new(assist: impl CodeAssist + 'static) -> Self {
        Self(Arc::new(assist))
    }
}

impl fmt::Debug for CodeAssistant {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("CodeAssistant(..)")
    }
}

/// The open completion list.
#[derive(Debug, Clone, PartialEq, Eq)]
struct OpenList {
    /// What is offered, and the word it replaces.
    completions: Completions,
    /// The selected entry.
    selected: usize,
    /// The first entry shown.
    top: usize,
}

impl OpenList {
    /// Select the entry `delta` away, wrapping at the ends, and scroll it into
    /// view.
    fn step(&mut self, down: bool) {
        let count = self.completions.items.len();
        if count == 0 {
            return;
        }
        self.selected = if down {
            let next = self.selected.saturating_add(1);
            if next >= count { 0 } else { next }
        } else {
            self.selected
                .checked_sub(1)
                .unwrap_or_else(|| count.saturating_sub(1))
        };
        self.reveal();
    }

    /// Scroll so the selected entry is shown.
    const fn reveal(&mut self) {
        if self.selected < self.top {
            self.top = self.selected;
        } else if self.selected >= self.top.saturating_add(VISIBLE_ROWS) {
            self.top = self.selected.saturating_sub(VISIBLE_ROWS.saturating_sub(1));
        }
    }

    /// The selected entry.
    fn selected(&self) -> Option<&Completion> {
        self.completions.items.get(self.selected)
    }
}

/// The pointer over a code editor, for its hover tip.
#[derive(Debug, Clone, Default)]
struct HoverState {
    /// Where the pointer is, in window coordinates, while it is over the text.
    pointer: Option<Vec2>,
    /// When it last moved, in seconds of [`Time`].
    moved_at: f32,
    /// Whether it moved since the last look.
    moved: bool,
    /// Whether the word it rests on has been looked up since it stopped.
    looked_up: bool,
    /// Whether it left the text this frame.
    left: bool,
    /// The description shown.
    shown: Option<Hover>,
}

/// The nodes a code editor's help is drawn in.
#[derive(Debug, Clone)]
struct AssistParts {
    /// A node at the caret the list and the signature hang from.
    caret_anchor: Entity,
    /// The completion list.
    list: Entity,
    /// Its rows, each with its label.
    rows: Vec<(Entity, Entity)>,
    /// The documentation box under the rows.
    list_doc: Entity,
    /// The selected entry's one-line detail.
    list_detail: Entity,
    /// The selected entry's documentation.
    list_text: Entity,
    /// The signature tip.
    signature: Entity,
    /// The signature's three spans: before the active parameter, it, after.
    signature_spans: [Entity; 3],
    /// The signature's documentation.
    signature_text: Entity,
    /// A node over the hovered word the hover tip hangs from.
    hover_anchor: Entity,
    /// The hover tip.
    hover: Entity,
    /// The hovered word's title.
    hover_title: Entity,
    /// Its documentation.
    hover_text: Entity,
}

/// A code editor's help: what is open, and what has been asked for — on the
/// field.
#[derive(Component, Debug)]
pub struct CodeAssistState {
    /// The nodes it is drawn in.
    parts: AssistParts,
    /// The open completion list.
    list: Option<OpenList>,
    /// The signature shown.
    signature: Option<Signature>,
    /// The pointer's description.
    hover: HoverState,
    /// How long the pointer rests before a description shows, in seconds.
    hover_delay: f32,
    /// The buffer the answers were worked out from.
    seen: String,
    /// The caret they were worked out at.
    caret: usize,
    /// Completions were asked for this frame, and how.
    completion_request: Option<CompletionRequest>,
    /// A `(` or `,` was typed, or `Ctrl+Shift+Space` pressed: show the
    /// signature.
    signature_requested: bool,
    /// `Enter`, `Tab` or a click accepted the selected completion.
    accept_requested: bool,
    /// Whether what is drawn has to be drawn again.
    dirty: bool,
}

/// How completions were asked for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CompletionRequest {
    /// A word character was typed: offer completions for the word.
    Typed,
    /// `Ctrl+Space`: offer them even with nothing typed.
    Manual,
}

/// Spawn the help's nodes for the editor whose field is `field` and whose
/// rich-text overlay is `overlay`, and put its state on the field.
pub(crate) fn attach_code_assist(
    commands: &mut Commands,
    field: Entity,
    overlay: Entity,
    font_size: f32,
) {
    let anchor = |commands: &mut Commands, name: &'static str| {
        commands
            .spawn((
                Node {
                    position_type: PositionType::Absolute,
                    width: Val::Px(1.0),
                    height: Val::Px(font_size),
                    ..default()
                },
                Pickable::IGNORE,
                Name::new(name),
                ChildOf(overlay),
            ))
            .id()
    };
    let caret_anchor = anchor(commands, "code-assist-caret");
    let hover_anchor = anchor(commands, "code-assist-hover-anchor");

    let list = commands
        .spawn((
            Node {
                display: Display::None,
                position_type: PositionType::Absolute,
                flex_direction: FlexDirection::Column,
                min_width: Val::Px(220.0),
                max_width: Val::Px(DOC_WIDTH),
                padding: UiRect::all(Val::Px(2.0)),
                border: UiRect::all(Val::Px(1.0)),
                ..default()
            },
            popover(PopoverSide::Bottom),
            BorderColor::all(SkinPalette::default().surface_border),
            ClassList::new_with_classes([DROP_DOWN_CLASS, ASSIST_LIST_CLASS]),
            GlobalZIndex(ASSIST_Z),
            OverrideClip,
            Semantic::new(Role::List),
            Pickable::default(),
            Name::new("code-assist-list"),
            ChildOf(caret_anchor),
        ))
        .id();
    let rows = (0..VISIBLE_ROWS)
        .map(|slot| {
            let label = commands
                .spawn((
                    Text::new(""),
                    UiFont::Mono.at(font_size),
                    TextColor(SkinPalette::default().text_primary),
                    ClassList::new_with_classes([TEXT_CLASS]),
                    Pickable::IGNORE,
                ))
                .id();
            let row = commands
                .spawn((
                    Node {
                        display: Display::None,
                        padding: UiRect::axes(Val::Px(6.0), Val::Px(1.0)),
                        ..default()
                    },
                    ClassList::new_with_classes([LIST_ROW_CLASS]),
                    Pickable::default(),
                    AssistRow { field, slot },
                    Name::new("code-assist-row"),
                    ChildOf(list),
                ))
                .add_child(label)
                .observe(on_assist_row_press)
                .id();
            (row, label)
        })
        .collect();
    let list_doc = commands
        .spawn((
            Node {
                display: Display::None,
                flex_direction: FlexDirection::Column,
                row_gap: Val::Px(2.0),
                margin: UiRect::top(Val::Px(2.0)),
                padding: UiRect::axes(Val::Px(6.0), Val::Px(3.0)),
                border: UiRect::top(Val::Px(1.0)),
                ..default()
            },
            BorderColor::all(SkinPalette::default().surface_border),
            Pickable::IGNORE,
            ChildOf(list),
        ))
        .id();
    let list_detail = commands
        .spawn((
            Text::new(""),
            UiFont::Mono.at(DOC_FONT_SIZE),
            TextColor(SkinPalette::default().text_muted),
            ClassList::new_with_classes([ASSIST_DETAIL_CLASS]),
            Pickable::IGNORE,
            ChildOf(list_doc),
        ))
        .id();
    let list_text = commands
        .spawn((
            Text::new(""),
            UiFont::Sans.at(DOC_FONT_SIZE),
            TextColor(SkinPalette::default().text_primary),
            ClassList::new_with_classes([TEXT_CLASS]),
            Pickable::IGNORE,
            ChildOf(list_doc),
        ))
        .id();

    let signature = spawn_tip(
        commands,
        caret_anchor,
        PopoverSide::Top,
        "code-assist-signature",
    );
    let signature_line = commands
        .spawn((
            Text::new(""),
            UiFont::Mono.at(DOC_FONT_SIZE),
            TextColor(TOOLTIP_TEXT),
            ClassList::new_with_classes([TOOLTIP_TEXT_CLASS]),
            ChildOf(signature),
        ))
        .id();
    let signature_spans = [false, true, false].map(|active| {
        commands
            .spawn((
                TextSpan::new(""),
                UiFont::Mono.at(DOC_FONT_SIZE),
                TextColor(TOOLTIP_TEXT),
                ClassList::new_with_classes([if active {
                    ASSIST_ACTIVE_CLASS
                } else {
                    TOOLTIP_TEXT_CLASS
                }]),
                ChildOf(signature_line),
            ))
            .id()
    });
    let signature_text = tip_text(commands, signature, UiFont::Sans);

    let hover = spawn_tip(
        commands,
        hover_anchor,
        PopoverSide::Bottom,
        "code-assist-hover",
    );
    let hover_title = tip_text(commands, hover, UiFont::Mono);
    let hover_text = tip_text(commands, hover, UiFont::Sans);

    commands.entity(field).insert(CodeAssistState {
        parts: AssistParts {
            caret_anchor,
            list,
            rows,
            list_doc,
            list_detail,
            list_text,
            signature,
            signature_spans,
            signature_text,
            hover_anchor,
            hover,
            hover_title,
            hover_text,
        },
        list: None,
        signature: None,
        hover: HoverState::default(),
        hover_delay: HOVER_DELAY,
        seen: String::new(),
        caret: 0,
        completion_request: None,
        signature_requested: false,
        accept_requested: false,
        dirty: false,
    });
}

/// A popover on `side` of its anchor, flipping to the other side where there
/// is no room.
fn popover(side: PopoverSide) -> Popover {
    let placement = |side: PopoverSide| PopoverPlacement {
        side,
        align: PopoverAlign::Start,
        gap: 2.0,
    };
    Popover {
        positions: vec![placement(side), placement(side.mirror())],
        window_margin: 4.0,
    }
}

/// Spawn a hidden tip under `anchor`, on `side` of it.
fn spawn_tip(
    commands: &mut Commands,
    anchor: Entity,
    side: PopoverSide,
    name: &'static str,
) -> Entity {
    let tip = commands
        .spawn((
            tooltip_box(),
            popover(side),
            OverrideClip,
            Name::new(name),
            ChildOf(anchor),
        ))
        .id();
    // Hidden by display, which the popover leaves alone: it only moves a tip.
    commands.entity(tip).entry::<Node>().and_modify(|mut node| {
        node.display = Display::None;
        node.row_gap = Val::Px(3.0);
    });
    tip
}

/// A line of text in a tip.
fn tip_text(commands: &mut Commands, tip: Entity, font: UiFont) -> Entity {
    commands
        .spawn((
            Text::new(""),
            font.at(DOC_FONT_SIZE),
            TextColor(TOOLTIP_TEXT),
            ClassList::new_with_classes([TOOLTIP_TEXT_CLASS]),
            ChildOf(tip),
        ))
        .id()
}

/// One row of a completion list: the field it completes in, and which of the
/// shown rows it is.
#[derive(Component, Debug, Clone, Copy)]
struct AssistRow {
    /// The editor's field.
    field: Entity,
    /// Its place among the shown rows.
    slot: usize,
}

// ---------------------------------------------------------------------------
// Keys.
// ---------------------------------------------------------------------------

/// Whether `character` belongs to a word a completion is offered for.
fn is_word_character(character: char) -> bool {
    character.is_alphanumeric() || character == '_'
}

/// The edits that put `completion` in place of the word over `replace` in
/// `text`, the caret at `caret` inside it — deleting forwards to the word's
/// end, back to its start, then typing the completion, so the field's own edit
/// path (and its undo) makes the change.
fn accept_edits(
    text: &str,
    replace: &Range<usize>,
    caret: usize,
    insert: &str,
) -> Option<Vec<TextEdit>> {
    let after = text.get(caret..replace.end)?.chars().count();
    let before = text.get(replace.start..caret)?.chars().count();
    let mut edits: Vec<TextEdit> = core::iter::repeat_n(TextEdit::Delete, after).collect();
    edits.extend(core::iter::repeat_n(TextEdit::Backspace, before));
    edits.push(TextEdit::Insert(insert.into()));
    Some(edits)
}

/// Take the keys the help answers out of each code editor's edits before
/// `bevy_text` applies them, and note what typing asks for.
///
/// While the list is open `Up` and `Down` move through it rather than the
/// caret, `Enter` accepts rather than breaking the line, and `Escape` closes
/// it; with the list closed, `Escape` dismisses the signature.
fn assist_keystrokes(
    mut fields: Query<(&mut EditableText, &mut CodeAssistState), Without<ReadOnlyField>>,
) {
    for (mut editable, mut state) in &mut fields {
        let state = &mut *state;
        if let Some(list) = state.list.as_mut() {
            match editable.pending_edits.as_slice() {
                [TextEdit::Up(false)] => {
                    list.step(false);
                    editable.pending_edits.clear();
                    state.dirty = true;
                    continue;
                }
                [TextEdit::Down(false)] => {
                    list.step(true);
                    editable.pending_edits.clear();
                    state.dirty = true;
                    continue;
                }
                [TextEdit::Insert(inserted)] if inserted.as_str() == "\n" => {
                    editable.pending_edits.clear();
                    state.accept_requested = true;
                }
                [TextEdit::CollapseSelection] => {
                    state.list = None;
                    state.dirty = true;
                    continue;
                }
                _ => {}
            }
        } else if state.signature.is_some()
            && matches!(
                editable.pending_edits.as_slice(),
                [TextEdit::CollapseSelection]
            )
        {
            state.signature = None;
            state.dirty = true;
            continue;
        }
        if core::mem::take(&mut state.accept_requested) {
            let text = editable.editor.raw_text();
            let caret = editable.editor.raw_selection().focus().index();
            // Only over the buffer the list was offered for: another edit in
            // the same frame has moved the word.
            let edits = state
                .list
                .as_ref()
                .filter(|_list| editable.pending_edits.is_empty() && state.seen == text)
                .and_then(|list| {
                    let completion = list.selected()?;
                    accept_edits(text, &list.completions.replace, caret, &completion.insert)
                });
            if let Some(edits) = edits {
                editable.pending_edits = edits;
            }
            state.list = None;
            state.dirty = true;
            continue;
        }
        for edit in &editable.pending_edits {
            if let TextEdit::Insert(inserted) = edit {
                if inserted.chars().next_back().is_some_and(is_word_character) {
                    state
                        .completion_request
                        .get_or_insert(CompletionRequest::Typed);
                }
                if inserted.contains(['(', ',']) {
                    state.signature_requested = true;
                }
            }
        }
    }
}

/// `Tab` with the completion list open accepts the selected entry — called by
/// the code editor's `Tab` observer before it indents. Returns whether it did.
pub(crate) const fn accept_on_tab(state: Option<&mut CodeAssistState>) -> bool {
    match state {
        Some(state) if state.list.is_some() => {
            state.accept_requested = true;
            true
        }
        _ => false,
    }
}

/// `Ctrl+Space` offers completions with nothing typed; `Ctrl+Shift+Space`
/// brings the signature back.
fn on_code_assist_key(
    mut event: On<FocusedInput<KeyboardInput>>,
    keys: Res<ButtonInput<KeyCode>>,
    mut states: Query<&mut CodeAssistState, Without<ReadOnlyField>>,
) {
    if !event.input.state.is_pressed() || event.input.logical_key != Key::Space {
        return;
    }
    let held = ChordModifiers::read(&keys);
    if !held.ctrl || held.alt {
        return;
    }
    let Ok(mut state) = states.get_mut(event.focused_entity) else {
        return;
    };
    event.propagate(false);
    if held.shift {
        state.signature_requested = true;
    } else {
        state.completion_request = Some(CompletionRequest::Manual);
    }
}

/// A press on a list row accepts its entry, and gives the keyboard back to the
/// editor.
fn on_assist_row_press(
    mut press: On<Pointer<Press>>,
    rows: Query<&AssistRow>,
    mut states: Query<&mut CodeAssistState>,
    mut focus: ResMut<InputFocus>,
) {
    press.propagate(false);
    if press.button != PointerButton::Primary {
        return;
    }
    let Ok(row) = rows.get(press.entity) else {
        return;
    };
    let Ok(mut state) = states.get_mut(row.field) else {
        return;
    };
    let state = &mut *state;
    if let Some(list) = state.list.as_mut() {
        let index = list.top.saturating_add(row.slot);
        if index < list.completions.items.len() {
            list.selected = index;
            state.accept_requested = true;
        }
    }
    focus.set(row.field, bevy::input_focus::FocusCause::Navigated);
}

// ---------------------------------------------------------------------------
// The answers.
// ---------------------------------------------------------------------------

/// Ask each code editor's language what to offer, after the frame's edits.
///
/// The list is asked again on every change while it is open, so it filters as
/// the word grows, and closes when the caret leaves the word or nothing
/// matches; the signature likewise follows the caret while it stays in the
/// call. Neither stays open on a field without the keyboard, or over a
/// selection.
fn update_code_assist(
    focus: Res<InputFocus>,
    mut fields: Query<(
        Entity,
        &EditableText,
        &CodeAssistant,
        &mut CodeAssistState,
        Has<ReadOnlyField>,
    )>,
) {
    for (field, editable, assistant, mut state, read_only) in &mut fields {
        let state = &mut *state;
        let text = editable.editor.raw_text();
        let selection = editable.editor.raw_selection();
        let caret = selection.focus().index();
        let changed = state.seen != text;
        let moved = state.caret != caret;
        let completion_request = state.completion_request.take();
        let manual = completion_request == Some(CompletionRequest::Manual);
        let requested_signature = core::mem::take(&mut state.signature_requested);
        if changed {
            text.clone_into(&mut state.seen);
            if state.hover.shown.take().is_some() {
                state.dirty = true;
            }
        }
        state.caret = caret;
        if focus.get() != Some(field) || !selection.is_collapsed() || read_only {
            if state.list.take().is_some() | state.signature.take().is_some() {
                state.dirty = true;
            }
            continue;
        }

        let left_word = state.list.as_ref().is_some_and(|list| {
            !changed
                && moved
                && !(list.completions.replace.start..=list.completions.replace.end).contains(&caret)
        });
        if left_word {
            state.list = None;
            state.dirty = true;
        } else if completion_request.is_some() || (state.list.is_some() && (changed || moved)) {
            let previous = state
                .list
                .as_ref()
                .and_then(|list| list.selected().map(|item| item.label.clone()));
            state.list = assistant
                .0
                .completions(text, caret)
                // Typing offers completions for what was typed; only
                // `Ctrl+Space` offers them for nothing.
                .filter(|found| manual || !found.replace.is_empty())
                .filter(|found| !found.items.is_empty())
                .map(|completions| {
                    let selected = previous
                        .and_then(|label| {
                            completions
                                .items
                                .iter()
                                .position(|item| item.label == label)
                        })
                        .unwrap_or(0);
                    let mut list = OpenList {
                        completions,
                        selected,
                        top: 0,
                    };
                    list.reveal();
                    list
                });
            state.dirty = true;
        }

        if requested_signature || (state.signature.is_some() && (changed || moved)) {
            let signature = assistant.0.signature(text, caret);
            if signature != state.signature {
                state.signature = signature;
                state.dirty = true;
            }
        }
    }
}

/// Follow the pointer over a code editor's text.
fn on_assist_pointer_move(
    event: On<Pointer<Move>>,
    time: Res<Time>,
    mut states: Query<&mut CodeAssistState>,
) {
    let Ok(mut state) = states.get_mut(event.entity) else {
        return;
    };
    let hover = &mut state.hover;
    hover.pointer = Some(event.pointer_location.position);
    hover.moved_at = time.elapsed_secs();
    hover.moved = true;
    hover.looked_up = false;
    hover.left = false;
}

/// The pointer coming onto a code editor's text, or one of its styled runs —
/// which is a move off the text and back on in the same frame.
fn on_assist_pointer_over(event: On<Pointer<Over>>, mut states: Query<&mut CodeAssistState>) {
    if let Ok(mut state) = states.get_mut(event.entity) {
        state.hover.left = false;
    }
}

/// The pointer leaving a code editor's text.
fn on_assist_pointer_out(event: On<Pointer<Out>>, mut states: Query<&mut CodeAssistState>) {
    if let Ok(mut state) = states.get_mut(event.entity) {
        state.hover.left = true;
    }
}

/// Show what the word under a resting pointer is, and take it away when the
/// pointer moves off it or leaves the text.
fn update_code_hover(
    time: Res<Time>,
    ui_scale: Res<UiScale>,
    mut fields: Query<(
        &EditableText,
        &CodeAssistant,
        &mut CodeAssistState,
        &ComputedNode,
        &UiGlobalTransform,
        &ComputedUiRenderTargetInfo,
        &TextScroll,
    )>,
) {
    let now = time.elapsed_secs();
    for (editable, assistant, mut state, node, transform, target, scroll) in &mut fields {
        let state = &mut *state;
        if core::mem::take(&mut state.hover.left) {
            state.hover.pointer = None;
            if state.hover.shown.take().is_some() {
                state.dirty = true;
            }
            continue;
        }
        let Some(pointer) = state.hover.pointer else {
            continue;
        };
        let moved = core::mem::take(&mut state.hover.moved);
        let rested = now - state.hover.moved_at >= state.hover_delay;
        if !moved && (state.hover.looked_up || !rested) {
            continue;
        }
        let point = rich_text_point(
            RichTextGeometry {
                editable,
                node,
                transform,
                target,
                scroll,
            },
            ui_scale.0,
            pointer,
        );
        let index = point.filter(|point| point.inside).map(|point| point.index);
        // Off the word described: the description goes at once.
        let still_on = |hover: &Hover| index.is_some_and(|index| hover.range.contains(&index));
        if state
            .hover
            .shown
            .as_ref()
            .is_some_and(|shown| !still_on(shown))
        {
            state.hover.shown = None;
            state.dirty = true;
        }
        if !rested || state.hover.looked_up {
            continue;
        }
        state.hover.looked_up = true;
        let found = index.and_then(|index| assistant.0.hover(editable.editor.raw_text(), index));
        if found != state.hover.shown {
            state.hover.shown = found;
            state.dirty = true;
        }
    }
}

// ---------------------------------------------------------------------------
// Drawing.
// ---------------------------------------------------------------------------

/// The words of `notes`, in the resident's language, joined on one line.
fn notes_line(notes: &[Note], translator: &Translator) -> Option<String> {
    let words: Vec<String> = notes
        .iter()
        .map(|note| match note {
            Note::Sleep(seconds) => {
                let value = f64::from(*seconds);
                let digits = if (value * 10.0).fract().abs() < 1e-6 {
                    1
                } else {
                    2
                };
                translator.format(
                    "code-assist-sleep",
                    &TransArgs::new().text("seconds", &translator.decimal(value, digits)),
                )
            }
            Note::Deprecated => translator.get("code-assist-deprecated"),
            Note::GodMode => translator.get("code-assist-god-mode"),
        })
        .collect();
    (!words.is_empty()).then(|| words.join(" · "))
}

/// Paragraphs joined by blank lines, the empty ones left out.
fn paragraphs(parts: impl IntoIterator<Item = Option<String>>) -> String {
    parts
        .into_iter()
        .flatten()
        .filter(|part| !part.is_empty())
        .collect::<Vec<String>>()
        .join("\n\n")
}

/// Set a node's display, writing only on a change.
fn show(nodes: &mut Query<&mut Node>, entity: Entity, shown: bool) {
    let wanted = if shown { Display::Flex } else { Display::None };
    if let Ok(mut node) = nodes.get_mut(entity)
        && node.display != wanted
    {
        node.display = wanted;
    }
}

/// Set a text node's text, writing only on a change.
fn write_text(texts: &mut Query<&mut Text>, entity: Entity, wanted: &str) {
    if let Ok(mut text) = texts.get_mut(entity)
        && text.0 != wanted
    {
        wanted.clone_into(&mut text.0);
    }
}

/// The class a completion's label is drawn in.
fn label_class(completion: &Completion) -> &'static str {
    if completion.deprecated {
        DEPRECATED_CLASS
    } else {
        completion.capture.map_or(TEXT_CLASS, syntax_class)
    }
}

/// Draw what each code editor's help has open.
fn draw_code_assist(
    mut fields: Query<&mut CodeAssistState>,
    translator: Translator,
    mut nodes: Query<&mut Node>,
    mut texts: Query<&mut Text>,
    mut spans: Query<&mut TextSpan>,
    mut classes: Query<&mut ClassList>,
) {
    let relocalised = translator.changed();
    for mut state in &mut fields {
        if !state.dirty && !relocalised {
            continue;
        }
        state.dirty = false;
        let parts = &state.parts;

        show(&mut nodes, parts.list, state.list.is_some());
        let list = state.list.as_ref();
        for (slot, (row, label)) in parts.rows.iter().enumerate() {
            let index = list.map(|list| list.top.saturating_add(slot));
            let item = list
                .zip(index)
                .and_then(|(list, index)| list.completions.items.get(index));
            show(&mut nodes, *row, item.is_some());
            let Some(item) = item else {
                continue;
            };
            write_text(&mut texts, *label, &item.label);
            if let Ok(mut row_classes) = classes.get_mut(*row) {
                set_state_class(
                    &mut row_classes,
                    SELECTED_CLASS,
                    list.is_some_and(|list| Some(list.selected) == index),
                );
            }
            let wanted = ClassList::new_with_classes([label_class(item)]);
            if let Ok(mut label_classes) = classes.get_mut(*label)
                && *label_classes != wanted
            {
                *label_classes = wanted;
            }
        }
        let selected = list.and_then(OpenList::selected);
        let detail = selected.and_then(|item| item.detail.clone());
        let documentation = selected.and_then(|item| item.documentation.clone());
        show(
            &mut nodes,
            parts.list_doc,
            detail.is_some() || documentation.is_some(),
        );
        show(&mut nodes, parts.list_detail, detail.is_some());
        write_text(&mut texts, parts.list_detail, &detail.unwrap_or_default());
        show(&mut nodes, parts.list_text, documentation.is_some());
        write_text(
            &mut texts,
            parts.list_text,
            &documentation.unwrap_or_default(),
        );

        show(&mut nodes, parts.signature, state.signature.is_some());
        if let Some(signature) = &state.signature {
            let active = signature.active_range().unwrap_or(0..0);
            let pieces = [
                signature.label.get(..active.start),
                signature.label.get(active.clone()),
                signature.label.get(active.end..),
            ];
            for (span, piece) in parts.signature_spans.iter().zip(pieces) {
                let wanted = piece.unwrap_or_default();
                if let Ok(mut span) = spans.get_mut(*span)
                    && span.0 != wanted
                {
                    wanted.clone_into(&mut span.0);
                }
            }
            let parameter = signature
                .active
                .and_then(|active| signature.parameters.get(active))
                .and_then(|parameter| {
                    let name = signature.label.get(parameter.label.clone())?;
                    parameter
                        .documentation
                        .as_ref()
                        .map(|documentation| format!("{name}: {documentation}"))
                });
            let text = paragraphs([
                parameter,
                signature.documentation.clone(),
                notes_line(&signature.notes, &translator),
            ]);
            show(&mut nodes, parts.signature_text, !text.is_empty());
            write_text(&mut texts, parts.signature_text, &text);
        }

        show(&mut nodes, parts.hover, state.hover.shown.is_some());
        if let Some(hover) = &state.hover.shown {
            write_text(&mut texts, parts.hover_title, &hover.title);
            let text = paragraphs([
                hover.documentation.clone(),
                notes_line(&hover.notes, &translator),
            ]);
            show(&mut nodes, parts.hover_text, !text.is_empty());
            write_text(&mut texts, parts.hover_text, &text);
        }
    }
}

/// Move each code editor's anchors to the caret and to the hovered word, which
/// the list and the tips hang from.
///
/// After the layout, as the field's own objects are placed; the write lands in
/// the next frame's layout, a frame behind a caret that moved.
fn place_code_assist(
    fields: Query<(
        &EditableText,
        &RichTextField,
        &CodeAssistState,
        &ComputedNode,
        &UiGlobalTransform,
        &TextScroll,
    )>,
    layers: Query<(&ComputedNode, &UiGlobalTransform)>,
    mut nodes: Query<&mut Node>,
) {
    for (editable, rich, state, node, transform, scroll) in &fields {
        let caret_open = state.list.is_some() || state.signature.is_some();
        if !caret_open && state.hover.shown.is_none() {
            continue;
        }
        let Ok((overlay_node, overlay_transform)) = layers.get(rich.overlay) else {
            continue;
        };
        let Some(overlay_inverse) = overlay_transform.try_inverse() else {
            continue;
        };
        let inverse_scale = node.inverse_scale_factor();
        let text_box = node.content_box().min;
        let text_origin = Vec2::new(text_box.x - scroll.0.x, text_box.y - scroll.0.y);
        let overlay_origin = overlay_node.padding_box().min;
        // Text space → field → world → overlay, as an object makes the trip.
        let to_overlay = |point: Vec2| {
            let in_field = Vec2::new(text_origin.x + point.x, text_origin.y + point.y);
            let in_world = transform.affine().transform_point2(in_field);
            let local = overlay_inverse.transform_point2(in_world);
            Vec2::new(
                (local.x - overlay_origin.x) * inverse_scale,
                (local.y - overlay_origin.y) * inverse_scale,
            )
        };
        let place = |nodes: &mut Query<&mut Node>, entity: Entity, rect: Rect| {
            let min = to_overlay(rect.min);
            let max = to_overlay(rect.max);
            let (left, top) = (Val::Px(min.x), Val::Px(min.y));
            let (width, height) = (Val::Px((max.x - min.x).max(1.0)), Val::Px(max.y - min.y));
            if let Ok(mut anchor) = nodes.get_mut(entity)
                && (anchor.left != left
                    || anchor.top != top
                    || anchor.width != width
                    || anchor.height != height)
            {
                anchor.left = left;
                anchor.top = top;
                anchor.width = width;
                anchor.height = height;
            }
        };
        if caret_open && let Some(caret) = editable.editor.cursor_geometry(1.0) {
            let rect = Rect::new(
                narrow(caret.x0),
                narrow(caret.y0),
                narrow(caret.x1),
                narrow(caret.y1),
            );
            place(&mut nodes, state.parts.caret_anchor, rect);
        }
        if let Some(hover) = &state.hover.shown
            && let Some(layout) = editable.editor.try_layout()
            && let Some(rect) = backdrop_rects(layout, editable.editor.raw_text(), &hover.range)
                .into_iter()
                .next()
        {
            place(&mut nodes, state.parts.hover_anchor, rect);
        }
    }
}

/// Register the help's systems and observers on `app` — called by the code
/// editor's plugin.
pub(crate) fn register(app: &mut App) {
    app.add_systems(
        PostUpdate,
        (
            assist_keystrokes
                .before(EditableTextSystems)
                .before(crate::ui_code_syntax::indent_code_keystrokes),
            (update_code_assist, update_code_hover, draw_code_assist)
                .chain()
                .after(crate::ui_code_syntax::highlight_code_editors)
                .before(UiSystems::PostLayout),
            place_code_assist.after(UiSystems::PostLayout),
        ),
    )
    .add_observer(on_code_assist_key);
}

/// Observe the pointer over a code editor's field, for its hover tip.
pub(crate) fn observe_hover(commands: &mut Commands, field: Entity) {
    commands
        .entity(field)
        .observe(on_assist_pointer_move)
        .observe(on_assist_pointer_over)
        .observe(on_assist_pointer_out);
}

#[cfg(test)]
mod tests {
    use super::{CodeAssistState, CodeAssistant, accept_edits};
    use crate::floater::FloaterPlugin;
    use crate::ui_code_editor::{
        CodeEditorGoTo, CodeEditorHandle, CodeEditorPlugin, CodeEditorSpec, spawn_code_editor,
    };
    use crate::ui_test::interact::{InteractionTest, tap, type_str};
    use crate::ui_test::{settle, spawn_under_root};
    use bevy::input::keyboard::Key;
    use bevy::prelude::*;
    use bevy::text::{EditableText, TextEdit};
    use pretty_assertions::assert_eq;
    use sl_highlight::{CodeAssist, Completion, Completions, Hover, Parameter, Signature, rank};

    /// A boxed error so a test can use `?` instead of `unwrap`.
    type TestError = Box<dyn core::error::Error>;

    /// A stand-in language: the words `alpha`, `alder`, `beta` complete, and
    /// `f(` takes two arguments.
    struct Toy;

    /// The word ending at `caret` in `source`.
    fn word_before(source: &str, caret: usize) -> core::ops::Range<usize> {
        let head = source.get(..caret).unwrap_or_default();
        let start = head
            .char_indices()
            .rev()
            .take_while(|(_offset, character)| character.is_alphanumeric())
            .last()
            .map_or(caret, |(offset, _character)| offset);
        start..caret
    }

    impl CodeAssist for Toy {
        fn completions(&self, source: &str, caret: usize) -> Option<Completions> {
            let replace = word_before(source, caret);
            let typed = source.get(replace.clone())?;
            let items = rank(
                typed,
                ["alpha", "alder", "beta"]
                    .iter()
                    .map(|word| Completion::word(*word, None)),
                10,
            );
            Some(Completions { replace, items })
        }

        fn signature(&self, source: &str, caret: usize) -> Option<Signature> {
            let head = source.get(..caret)?;
            let open = head.rfind("f(")?;
            let inside = head.get(open..)?;
            if inside.contains(')') {
                return None;
            }
            Some(Signature {
                label: String::from("f(a, b)"),
                parameters: vec![
                    Parameter {
                        label: 2..3,
                        documentation: None,
                    },
                    Parameter {
                        label: 5..6,
                        documentation: None,
                    },
                ],
                active: Some(inside.matches(',').count()),
                documentation: None,
                notes: Vec::new(),
            })
        }

        fn hover(&self, _source: &str, _index: usize) -> Option<Hover> {
            None
        }
    }

    /// An app with an empty code editor that speaks [`Toy`], focused.
    fn assisted_app() -> (App, CodeEditorHandle) {
        let mut app = InteractionTest::new().build();
        sl_viewer_ui_core::i18n::install_untranslated(&mut app);
        app.add_plugins((FloaterPlugin, CodeEditorPlugin));
        settle(&mut app);
        let column = spawn_under_root(
            &mut app,
            Node {
                width: Val::Px(480.0),
                flex_direction: FlexDirection::Column,
                ..default()
            },
        );
        let handle = {
            let mut commands = app.world_mut().commands();
            spawn_code_editor(
                &mut commands,
                column,
                &CodeEditorSpec {
                    visible_lines: 10.0,
                    ..CodeEditorSpec::new("code-assist-test")
                },
            )
        };
        app.world_mut().flush();
        app.world_mut()
            .entity_mut(handle.field)
            .insert(CodeAssistant::new(Toy));
        settle(&mut app);
        app.world_mut().write_message(CodeEditorGoTo {
            field: handle.field,
            line: 1,
            column: 1,
        });
        settle(&mut app);
        (app, handle)
    }

    /// The field's text.
    fn text_of(app: &App, field: Entity) -> Result<String, TestError> {
        Ok(app
            .world()
            .get::<EditableText>(field)
            .ok_or("the field lost its editor")?
            .value()
            .to_string())
    }

    /// The labels the open list offers, and the selected one.
    fn offered(app: &App, field: Entity) -> Result<Option<(Vec<String>, usize)>, TestError> {
        let state = app
            .world()
            .get::<CodeAssistState>(field)
            .ok_or("the editor has no help")?;
        Ok(state.list.as_ref().map(|list| {
            (
                list.completions
                    .items
                    .iter()
                    .map(|item| item.label.clone())
                    .collect(),
                list.selected,
            )
        }))
    }

    /// What the signature tip shows emphasised: the active parameter's text,
    /// `-` for none, or `None` when no signature shows.
    fn active_parameter(app: &App, field: Entity) -> Result<Option<String>, TestError> {
        Ok(app
            .world()
            .get::<CodeAssistState>(field)
            .ok_or("the editor has no help")?
            .signature
            .as_ref()
            .map(|signature| {
                signature
                    .active_range()
                    .and_then(|range| signature.label.get(range))
                    .unwrap_or("-")
                    .to_owned()
            }))
    }

    /// Accepting deletes the word around the caret and types the completion.
    #[test]
    fn accepting_replaces_the_whole_word() {
        assert_eq!(
            accept_edits("x alp y", &(2..5), 4, "alpha"),
            Some(vec![
                TextEdit::Delete,
                TextEdit::Backspace,
                TextEdit::Backspace,
                TextEdit::Insert("alpha".into()),
            ])
        );
    }

    /// **Typing a word opens the list and filters it; `Down` and `Enter` put
    /// the selected entry in place of the word.**
    #[test]
    fn typing_offers_and_enter_accepts() -> Result<(), TestError> {
        let (mut app, handle) = assisted_app();
        type_str(&mut app, "a");
        settle(&mut app);
        assert_eq!(
            offered(&app, handle.field)?,
            Some((vec![String::from("alder"), String::from("alpha")], 0))
        );
        tap(&mut app, KeyCode::ArrowDown, Key::ArrowDown);
        settle(&mut app);
        assert_eq!(
            offered(&app, handle.field)?.map(|(_labels, selected)| selected),
            Some(1)
        );
        tap(&mut app, KeyCode::Enter, Key::Enter);
        settle(&mut app);
        assert_eq!(text_of(&app, handle.field)?, "alpha");
        assert_eq!(offered(&app, handle.field)?, None);
        // With the list closed, Enter breaks the line again.
        tap(&mut app, KeyCode::Enter, Key::Enter);
        settle(&mut app);
        assert_eq!(text_of(&app, handle.field)?, "alpha\n");
        Ok(())
    }

    /// **`Tab` accepts while the list is open, and `Escape` closes it without
    /// touching the text.**
    #[test]
    fn tab_accepts_and_escape_dismisses() -> Result<(), TestError> {
        let (mut app, handle) = assisted_app();
        type_str(&mut app, "be");
        settle(&mut app);
        tap(&mut app, KeyCode::Tab, Key::Tab);
        settle(&mut app);
        assert_eq!(text_of(&app, handle.field)?, "beta");
        type_str(&mut app, " al");
        settle(&mut app);
        assert!(offered(&app, handle.field)?.is_some());
        tap(&mut app, KeyCode::Escape, Key::Escape);
        settle(&mut app);
        assert_eq!(offered(&app, handle.field)?, None);
        assert_eq!(text_of(&app, handle.field)?, "beta al");
        // Typing on re-filters; nothing matching closes the list.
        type_str(&mut app, "d");
        settle(&mut app);
        assert_eq!(
            offered(&app, handle.field)?.map(|(labels, _selected)| labels),
            Some(vec![String::from("alder")])
        );
        type_str(&mut app, "q");
        settle(&mut app);
        assert_eq!(offered(&app, handle.field)?, None);
        Ok(())
    }

    /// **`(` shows the signature, each `,` moves the active argument, and
    /// leaving the call takes it away.**
    #[test]
    fn the_signature_follows_the_argument() -> Result<(), TestError> {
        let (mut app, handle) = assisted_app();
        type_str(&mut app, "f(");
        settle(&mut app);
        assert_eq!(active_parameter(&app, handle.field)?.as_deref(), Some("a"));
        type_str(&mut app, "1, ");
        settle(&mut app);
        assert_eq!(active_parameter(&app, handle.field)?.as_deref(), Some("b"));
        type_str(&mut app, "2)");
        settle(&mut app);
        assert_eq!(active_parameter(&app, handle.field)?, None);
        Ok(())
    }
}
