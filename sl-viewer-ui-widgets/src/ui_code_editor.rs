//! The **code editor** (`viewer-lsl-editor-widget`): the rich-text field set up
//! for source code, with the editor around it that a script needs and a
//! notecard does not.
//!
//! ```text
//! box (.sk-field-box)              — the well, framing the gutter too
//! ├─ gutter (.sk-code-gutter)      — line numbers, scrolled with the text
//! │  └─ number (.sk-line-number) …   one per visible line, recycled
//! └─ rich text root                — crate::ui_rich_text, bare, unwrapped, mono
//!    ├─ underlay                   — the caret line, find matches, backdrops
//!    ├─ field                      — the EditableText
//!    └─ overlay
//! ```
//!
//! # What the editor adds over the field
//!
//! The field ([`crate::ui_rich_text`]) already carries per-range style classes,
//! boxes behind ranges, an undo history ([`crate::ui_text_history`]) and the
//! caret, selection, clipboard, IME and bidi parley gives every field. On top of
//! it this module builds:
//!
//! - a **gutter** of line numbers, laid out from parley's own lines so it stays
//!   level with the text through scrolling, a tall glyph from a fallback font,
//!   or a wrapped line;
//! - **wheel scrolling** — an editable field otherwise scrolls only to follow
//!   its caret;
//! - **go to line**: [`CodeEditorGoTo`] for a caller (a compile error's row),
//!   and `Ctrl+G` for the resident;
//! - **find and replace** (`Ctrl+F`, `F3` / `Shift+F3` to repeat): find up or
//!   down, case-insensitive or not, replace one or all — the reference's
//!   *Search / Replace* window — with every match on screen washed while the
//!   window is open.
//!
//! The text is set in the monospace face and does not wrap, because code is not
//! prose: an indented block has to line up, and a long line is read by
//! scrolling, not by breaking it where the window happens to end.
//!
//! # The relayout cost (measured, 2026-10-29)
//!
//! `PlainEditor` used to lay the whole buffer out again on every edit — 22 ms a
//! keystroke in a highlighted script at the 64 KB limit, more than a 60 Hz
//! frame. The workspace's parley fork now keeps each paragraph's layout and
//! re-shapes only the paragraphs an edit touched, and moves the range styles
//! with the text so the relayout an edit forces can reuse everything after it.
//! A keystroke, with the highlighter's ranges recomputed over the whole buffer
//! (release build, `tests/editor_relayout.rs`):
//!
//! | script | lines | plain | highlighted | first layout |
//! |---|---|---|---|---|
//! | 16 KB | 677 | 0.14 ms | 0.64 ms | 15 ms |
//! | 32 KB | 1,366 | 0.27 ms | 1.3 ms | 26 ms |
//! | 64 KB (the limit) | 2,731 | 0.76 ms | 3.6 ms | 52 ms |
//!
//! What a keystroke still pays in proportion to the buffer is the join of the
//! paragraphs into one layout and Bevy's own pass over the finished glyphs,
//! which this does not time. Opening a script lays every paragraph out
//! separately, which costs about twice a whole layout, once.
//!
//! Reference (Firestorm, read-only): `llscripteditor` (the gutter),
//! `llfloatersearchreplace`, `llfloatergotoline`, `llpreviewscript`.

use core::ops::Range;

use bevy::ecs::system::SystemParam;
use bevy::input::keyboard::{Key, KeyboardInput};
use bevy::input::mouse::MouseScrollUnit;
use bevy::input_focus::{FocusCause, FocusedInput, InputFocus};
use bevy::prelude::*;
use bevy::text::{EditableText, FontCx, LayoutCx};
use bevy::ui::Checked;
use bevy::ui::widget::TextScroll;
use bevy_flair::style::components::ClassList;

use sl_viewer_ui_core::i18n::Translated;
use sl_viewer_ui_core::skin_palette::SkinPalette;
use sl_viewer_ui_core::ui::{UiPanelShown, column, row};
use sl_viewer_ui_core::ui_element::{ContentMayOverflow, ElementCx, TextMayClip};
use sl_viewer_ui_core::ui_font::UiFont;
use sl_viewer_ui_core::ui_spawn::{
    ButtonSpec, LabeledRowSpec, UiLabel, spawn_button, spawn_labeled_row,
};

use crate::floater::{
    Floater, FloaterCaps, FloaterHandle, FloaterOwner, FloaterSpec, FloaterSystems,
    KeyedFloaterOpen, KeyedFloaters, picker_identity,
};
use crate::text_search::{
    SearchQuery, find_all, find_next, find_previous, line_column_offset, line_count, replace_all,
};
use crate::ui_checkbox::{CheckboxSpec, spawn_checkbox};
use crate::ui_rich_text::{
    RichTextBackdrop, RichTextClass, RichTextHandle, RichTextMarks, RichTextSpec, narrow,
    spawn_rich_text,
};
use crate::ui_text_history::ChordModifiers;
use crate::ui_text_input::{
    ReadOnlyField, TextInputKind, TextInputSpec, decorate_field_box, spawn_text_input,
};

/// The skin class of the gutter column (`--gutter-bg`).
pub const GUTTER_CLASS: &str = "sk-code-gutter";

/// The skin class of a line number in the gutter (`--line-number`).
pub const LINE_NUMBER_CLASS: &str = "sk-line-number";

/// The skin class of a find match's wash (`--find-match`).
pub const FIND_MATCH_CLASS: &str = "sk-find-match";

/// The fewest digits the gutter leaves room for, so it does not widen at line
/// ten and again at line a hundred while a script is being written.
const MIN_GUTTER_DIGITS: usize = 3;

/// A digit's advance in the monospace face, as a fraction of the font size —
/// DejaVu Sans Mono's 1233 units of 2048, rounded up so a number never touches
/// the gutter's edge.
const MONO_ADVANCE_EM: f32 = 0.61;

/// The room either side of the numbers in the gutter, in logical pixels.
const GUTTER_PADDING: f32 = 6.0;

/// How many lines one notch of the wheel scrolls — the reference's
/// `LLTextEditor` scroll step.
const WHEEL_LINES: f32 = 3.0;

/// Where a revealed line lands, as a fraction of the view's height from its
/// top: a third down, so what follows a jump is in view with it.
const REVEAL_FRACTION: f32 = 1.0 / 3.0;

// ---------------------------------------------------------------------------
// The widget.
// ---------------------------------------------------------------------------

/// Everything a code editor is built from.
#[derive(Debug, Clone)]
pub struct CodeEditorSpec {
    /// The prefix of the editor's node [`Name`]s; the field itself is named
    /// `{element}:field`, as every text field is.
    pub element: &'static str,
    /// The source the editor starts with.
    pub initial: String,
    /// The field's focus stop.
    pub tab_index: i32,
    /// The text's font size, in logical pixels.
    pub font_size: f32,
    /// The height the editor opens at, in visible lines. It **fills** past that
    /// whatever room its container has spare.
    pub visible_lines: f32,
    /// Whether the source may be read, selected and copied but not changed.
    pub read_only: bool,
    /// The style classes the content's ranges name — a highlighter's token
    /// kinds, each normally [`RichTextClass::skinned`].
    pub classes: Vec<RichTextClass>,
    /// The Fluent key of what the editor is called.
    pub name_key: Option<&'static str>,
}

impl CodeEditorSpec {
    /// A spec for `element` with an empty buffer and no classes.
    #[must_use]
    pub const fn new(element: &'static str) -> Self {
        Self {
            element,
            initial: String::new(),
            tab_index: 0,
            font_size: 14.0,
            visible_lines: 20.0,
            read_only: false,
            classes: Vec::new(),
            name_key: None,
        }
    }
}

/// The entities a code editor is made of.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CodeEditorHandle {
    /// The framed box around the gutter and the text.
    pub root: Entity,
    /// The line-number column.
    pub gutter: Entity,
    /// The field whose buffer is the source.
    pub field: Entity,
    /// The rich-text field's own entities, for its overlay and underlay.
    pub text: RichTextHandle,
}

/// A code editor's state, on its field.
#[derive(Component, Debug, Clone)]
pub struct CodeEditor {
    /// The line-number column.
    pub gutter: Entity,
    /// The font size the gutter's numbers are set at.
    font_size: f32,
    /// How many digits the gutter is wide for now.
    digits: usize,
    /// The last search, which `F3` repeats.
    last_search: Option<LastSearch>,
}

/// A search the resident made, kept so `F3` can make it again.
#[derive(Debug, Clone, PartialEq, Eq)]
struct LastSearch {
    /// What was looked for.
    needle: String,
    /// Whether case mattered.
    case_sensitive: bool,
}

/// One line number in a gutter, recycled as the text scrolls.
#[derive(Component, Debug, Clone, Copy)]
struct GutterNumber;

/// Spawn a code editor under `parent`.
pub fn spawn_code_editor(
    commands: &mut Commands,
    parent: Entity,
    spec: &CodeEditorSpec,
) -> CodeEditorHandle {
    let root = commands
        .spawn((
            Node {
                flex_direction: FlexDirection::Row,
                flex_grow: 1.0,
                ..default()
            },
            Name::new(format!("{}:code-editor", spec.element)),
            ChildOf(parent),
        ))
        .id();
    let digits = gutter_digits(line_count(&spec.initial));
    let gutter = commands
        .spawn((
            Node {
                width: Val::Px(gutter_width(spec.font_size, digits)),
                flex_shrink: 0.0,
                overflow: Overflow::clip(),
                border: UiRect::right(Val::Px(1.0)),
                ..column(Val::ZERO)
            },
            BackgroundColor(Color::NONE),
            BorderColor::all(SkinPalette::default().control_border),
            ClassList::new_with_classes([GUTTER_CLASS]),
            TextMayClip {
                reason: "the gutter scrolls with the text, so the numbers of lines scrolled past \
                         are cut at its edge by design",
            },
            ContentMayOverflow {
                reason: "the numbers are absolutely positioned from the text layout, not by taffy",
            },
            Name::new(format!("{}:gutter", spec.element)),
            ChildOf(root),
        ))
        .id();
    let text = spawn_rich_text(
        commands,
        root,
        &RichTextSpec {
            initial: spec.initial.clone(),
            tab_index: spec.tab_index,
            font_size: spec.font_size,
            visible_lines: spec.visible_lines,
            fill: true,
            read_only: spec.read_only,
            // The box frames the gutter as well, so the well is its.
            decorated: false,
            classes: spec.classes.clone(),
            name_key: spec.name_key,
            font: UiFont::Mono,
            wrap: false,
            caret_line: true,
            ..RichTextSpec::new(spec.element)
        },
    );
    decorate_field_box(commands, root, text.field);
    commands
        .entity(text.field)
        .insert((
            CodeEditor {
                gutter,
                font_size: spec.font_size,
                digits,
                last_search: None,
            },
            RichTextMarks::default(),
        ))
        .observe(on_code_scroll);
    commands.entity(gutter).observe(on_gutter_scroll);
    CodeEditorHandle {
        root,
        gutter,
        field: text.field,
        text,
    }
}

/// How many digits a gutter needs for `lines` lines.
fn gutter_digits(lines: usize) -> usize {
    let digits = lines
        .checked_ilog10()
        .map_or(1, |log| log.saturating_add(1));
    usize::try_from(digits).map_or(MIN_GUTTER_DIGITS, |digits| digits.max(MIN_GUTTER_DIGITS))
}

/// The gutter's width for `digits` digits at `font_size`, in logical pixels.
fn gutter_width(font_size: f32, digits: usize) -> f32 {
    let digits = u16::try_from(digits).map_or_else(|_wide| f32::from(u16::MAX), f32::from);
    digits * font_size * MONO_ADVANCE_EM + GUTTER_PADDING * 2.0
}

// ---------------------------------------------------------------------------
// The gutter.
// ---------------------------------------------------------------------------

/// Number the lines a code editor shows, level with the text.
///
/// The numbers are placed from parley's own lines, not from a line height times
/// an index: a fallback font's taller glyph grows its line, and a number placed
/// by arithmetic would drift off its line from there down. Only a laid-out line
/// that *starts* a line of the source gets a number, so a wrapped line (which
/// this editor does not make, but the gutter does not assume) is numbered once.
#[expect(
    clippy::type_complexity,
    reason = "the field query is the editor's state, its layout and where it sits on screen"
)]
fn sync_code_gutters(
    mut fields: Query<(
        &mut CodeEditor,
        &EditableText,
        &ComputedNode,
        &UiGlobalTransform,
        &TextScroll,
    )>,
    mut gutters: Query<
        (
            &mut Node,
            &ComputedNode,
            &UiGlobalTransform,
            Option<&Children>,
        ),
        Without<GutterNumber>,
    >,
    mut numbers: Query<(&mut Node, &mut Text), (With<GutterNumber>, Without<CodeEditor>)>,
    mut commands: Commands,
) {
    for (mut editor, editable, node, transform, scroll) in &mut fields {
        let Some(layout) = editable.editor.try_layout() else {
            continue;
        };
        let text = editable.editor.raw_text();
        let Ok((mut gutter_node, gutter_computed, gutter_transform, children)) =
            gutters.get_mut(editor.gutter)
        else {
            continue;
        };
        let digits = gutter_digits(line_count(text));
        if digits != editor.digits {
            editor.digits = digits;
            gutter_node.width = Val::Px(gutter_width(editor.font_size, digits));
        }
        let existing: Vec<Entity> = children
            .map(|children| {
                children
                    .iter()
                    .filter(|child| numbers.contains(*child))
                    .collect()
            })
            .unwrap_or_default();
        let Some(gutter_inverse) = gutter_transform.try_inverse() else {
            continue;
        };
        let inverse_scale = node.inverse_scale_factor();
        let text_box = node.content_box();
        let gutter_top = gutter_computed.padding_box().min.y;
        let top = scroll.0.y;
        let bottom = scroll.0.y + text_box.height();

        let mut placed = 0_usize;
        let mut line_number = 1_usize;
        let mut counted_to = 0_usize;
        for line in layout.lines() {
            let range = line.text_range();
            let starts_line = range.start == 0
                || text
                    .as_bytes()
                    .get(range.start.saturating_sub(1))
                    .is_some_and(|byte| *byte == b'\n');
            let metrics = line.metrics();
            if metrics.block_max_coord < top {
                continue;
            }
            if metrics.block_min_coord > bottom {
                break;
            }
            if !starts_line {
                continue;
            }
            // Counted from where the last count stopped, so a screen of
            // numbers walks the buffer once.
            line_number = line_number.saturating_add(
                text.get(counted_to..range.start).map_or(0, |skipped| {
                    skipped.bytes().filter(|byte| *byte == b'\n').count()
                }),
            );
            counted_to = range.start;
            let in_field = Vec2::new(
                text_box.min.x,
                text_box.min.y - scroll.0.y + metrics.block_min_coord,
            );
            let in_world = transform.affine().transform_point2(in_field);
            let local = gutter_inverse.transform_point2(in_world);
            let number_top = Val::Px((local.y - gutter_top) * inverse_scale);
            let label = line_number.to_string();
            if let Some(entity) = existing.get(placed)
                && let Ok((mut number_node, mut number_text)) = numbers.get_mut(*entity)
            {
                if number_node.top != number_top || number_node.display != Display::Flex {
                    number_node.top = number_top;
                    number_node.display = Display::Flex;
                }
                if number_text.0 != label {
                    number_text.0 = label;
                }
            } else {
                commands.spawn((
                    Node {
                        position_type: PositionType::Absolute,
                        left: Val::Px(GUTTER_PADDING),
                        right: Val::Px(GUTTER_PADDING),
                        top: number_top,
                        ..default()
                    },
                    Text::new(label),
                    UiFont::Mono.at(editor.font_size),
                    TextLayout::justify(Justify::Right).with_no_wrap(),
                    TextColor(SkinPalette::default().text_muted),
                    ClassList::new_with_classes([LINE_NUMBER_CLASS]),
                    GutterNumber,
                    Pickable::IGNORE,
                    ChildOf(editor.gutter),
                ));
            }
            placed = placed.saturating_add(1);
        }
        for surplus in existing.iter().skip(placed) {
            if let Ok((mut number_node, _text)) = numbers.get_mut(*surplus)
                && number_node.display != Display::None
            {
                number_node.display = Display::None;
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Scrolling.
// ---------------------------------------------------------------------------

/// The furthest a field's text can scroll down, in its layout's pixels.
fn max_scroll(editable: &EditableText, node: &ComputedNode) -> Vec2 {
    let view = node.content_box().size();
    editable.editor.try_layout().map_or(Vec2::ZERO, |layout| {
        Vec2::new(
            (layout.full_width() - view.x).max(0.0),
            (layout.height() - view.y).max(0.0),
        )
    })
}

/// Scroll a code editor's text by the wheel.
///
/// A field scrolls itself only to follow its caret, so without this a script
/// longer than the window could be read only by walking the caret down it. The
/// scroll holds until the caret moves again, which is when the field brings the
/// caret back into view — every editor's behaviour.
fn scroll_code_editor(
    field: Entity,
    scroll: &Scroll,
    fields: &mut Query<(&EditableText, &ComputedNode, &mut TextScroll), With<CodeEditor>>,
    shift: bool,
) {
    let Ok((editable, node, mut text_scroll)) = fields.get_mut(field) else {
        return;
    };
    let line = editable
        .editor
        .try_layout()
        .and_then(|layout| layout.lines().next())
        .map_or(16.0, |line| line.metrics().line_height);
    let step = match scroll.unit {
        MouseScrollUnit::Line => line * WHEEL_LINES,
        MouseScrollUnit::Pixel => 1.0 / node.inverse_scale_factor(),
    };
    let limit = max_scroll(editable, node);
    let (dx, dy) = if shift {
        (scroll.y, 0.0)
    } else {
        (scroll.x, scroll.y)
    };
    let next = Vec2::new(
        (text_scroll.0.x - dx * step).clamp(0.0, limit.x),
        (text_scroll.0.y - dy * step).clamp(0.0, limit.y),
    );
    if text_scroll.0 != next {
        text_scroll.0 = next;
    }
}

/// The wheel over a code editor's text.
fn on_code_scroll(
    mut event: On<Pointer<Scroll>>,
    keys: Res<ButtonInput<KeyCode>>,
    mut fields: Query<(&EditableText, &ComputedNode, &mut TextScroll), With<CodeEditor>>,
) {
    let shift = keys.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]);
    scroll_code_editor(event.entity, &event.event, &mut fields, shift);
    event.propagate(false);
}

/// The wheel over a code editor's gutter scrolls its text.
fn on_gutter_scroll(
    mut event: On<Pointer<Scroll>>,
    keys: Res<ButtonInput<KeyCode>>,
    editors: Query<(Entity, &CodeEditor)>,
    mut fields: Query<(&EditableText, &ComputedNode, &mut TextScroll), With<CodeEditor>>,
) {
    let gutter = event.entity;
    let Some((field, _editor)) = editors
        .iter()
        .find(|(_field, editor)| editor.gutter == gutter)
    else {
        return;
    };
    let shift = keys.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]);
    scroll_code_editor(field, &event.event, &mut fields, shift);
    event.propagate(false);
}

/// Scroll a field so its caret is in view, a third of the way down when it was
/// not — what a jump does, from an unfocused find window as much as from the
/// field itself (the field follows its caret only while it has focus).
fn reveal_caret(editable: &EditableText, node: &ComputedNode, scroll: &mut TextScroll) {
    let Some(caret) = editable.editor.cursor_geometry(1.0) else {
        return;
    };
    let view = node.content_box().size();
    let limit = max_scroll(editable, node);
    let (x0, y0, y1) = (narrow(caret.x0), narrow(caret.y0), narrow(caret.y1));
    let mut next = scroll.0;
    if y0 < next.y || y1 > next.y + view.y {
        next.y = (y0 - view.y * REVEAL_FRACTION).clamp(0.0, limit.y);
    }
    if x0 < next.x || x0 > next.x + view.x {
        next.x = (x0 - view.x * 0.5).clamp(0.0, limit.x);
    }
    if scroll.0 != next {
        scroll.0 = next;
    }
}

// ---------------------------------------------------------------------------
// Go to line.
// ---------------------------------------------------------------------------

/// Put a code editor's caret at a line and column, focus it and scroll the line
/// into view — what clicking a compile error does.
#[derive(Message, Debug, Clone, Copy, PartialEq, Eq)]
pub struct CodeEditorGoTo {
    /// The editor's field.
    pub field: Entity,
    /// The 1-based line; one past the end lands on the last line.
    pub line: usize,
    /// The 1-based column, in characters; past the line's end lands on its end.
    pub column: usize,
}

/// Make each [`CodeEditorGoTo`].
fn go_to_line(
    mut requests: MessageReader<CodeEditorGoTo>,
    mut fields: Query<(&mut EditableText, &ComputedNode, &mut TextScroll), With<CodeEditor>>,
    mut engine: TextEngine,
) {
    for request in requests.read() {
        let Ok((mut editable, node, mut scroll)) = fields.get_mut(request.field) else {
            continue;
        };
        let offset = line_column_offset(editable.editor.raw_text(), request.line, request.column);
        editable
            .editor
            .driver(&mut engine.font_cx, &mut engine.layout_cx)
            .move_to_byte(offset);
        reveal_caret(&editable, node, &mut scroll);
        engine.focus.set(request.field, FocusCause::Navigated);
    }
}

// ---------------------------------------------------------------------------
// The keys.
// ---------------------------------------------------------------------------

/// Ask for the Search / Replace window over a code editor.
#[derive(Message, Debug, Clone, Copy, PartialEq, Eq)]
pub struct OpenFindReplace {
    /// The editor's field the window searches.
    pub field: Entity,
}

/// Ask for the Go to Line window over a code editor.
#[derive(Message, Debug, Clone, Copy, PartialEq, Eq)]
pub struct OpenGoToLine {
    /// The editor's field the window moves the caret in.
    pub field: Entity,
}

/// Repeat a code editor's last search, down or up — `F3` / `Shift+F3`.
#[derive(Message, Debug, Clone, Copy, PartialEq, Eq)]
struct RepeatSearch {
    /// The editor's field.
    field: Entity,
    /// Whether to search up.
    up: bool,
}

/// A code editor's own chords: `Ctrl+F` / `Ctrl+H` for Search / Replace,
/// `Ctrl+G` for Go to Line, `F3` / `Shift+F3` to find again.
///
/// A focused text field takes every chord before the menus see it
/// (`menu_accel`), so these cannot collide with a menu's accelerator.
fn on_code_editor_key(
    event: On<FocusedInput<KeyboardInput>>,
    keys: Res<ButtonInput<KeyCode>>,
    editors: Query<(), With<CodeEditor>>,
    mut find: MessageWriter<OpenFindReplace>,
    mut go_to: MessageWriter<OpenGoToLine>,
    mut repeat: MessageWriter<RepeatSearch>,
) {
    if !event.input.state.is_pressed() {
        return;
    }
    let field = event.focused_entity;
    if !editors.contains(field) {
        return;
    }
    let ChordModifiers { ctrl, shift, alt } = ChordModifiers::read(&keys);
    if alt {
        return;
    }
    match &event.input.logical_key {
        Key::F3 if !ctrl => {
            repeat.write(RepeatSearch { field, up: shift });
        }
        Key::Character(character) if ctrl && !shift => {
            if character.eq_ignore_ascii_case("f") || character.eq_ignore_ascii_case("h") {
                find.write(OpenFindReplace { field });
            } else if character.eq_ignore_ascii_case("g") {
                go_to.write(OpenGoToLine { field });
            }
        }
        _ => {}
    }
}

// ---------------------------------------------------------------------------
// Search / Replace.
// ---------------------------------------------------------------------------

/// The Search / Replace window's [`FloaterSpec`].
#[must_use]
pub fn find_replace_floater_spec() -> FloaterSpec {
    FloaterSpec {
        id: "find-replace",
        title: "Search / Replace".to_owned(),
        position: Vec2::new(420.0, 160.0),
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

/// The Go to Line window's [`FloaterSpec`].
#[must_use]
pub fn go_to_line_floater_spec() -> FloaterSpec {
    FloaterSpec {
        id: "go-to-line",
        title: "Go to Line".to_owned(),
        position: Vec2::new(440.0, 200.0),
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

/// A Search / Replace window's controls, on its root.
#[derive(Component, Debug, Clone, Copy)]
struct FindReplaceWindow {
    /// The editor's field it searches.
    field: Entity,
    /// What to search for.
    search: Entity,
    /// What to replace a match with.
    replace: Entity,
    /// The Case Insensitive box.
    case_insensitive: Entity,
    /// The Search Up box.
    search_up: Entity,
}

/// What a Search / Replace button does.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
enum FindAction {
    /// Find the next match in the window's direction.
    Search,
    /// Replace the selected match, then find the next.
    Replace,
    /// Replace every match.
    ReplaceAll,
}

/// The parts of a Search / Replace window, before it knows its field.
#[derive(Debug, Clone, Copy)]
struct FindReplaceParts {
    /// The search field.
    search: Entity,
    /// The replace field.
    replace: Entity,
    /// The Case Insensitive box.
    case_insensitive: Entity,
    /// The Search Up box.
    search_up: Entity,
}

/// Build the Search / Replace window's content under `parent`: the two fields,
/// the two boxes and the three buttons — the reference's layout — with `seed`
/// in the search field.
fn build_find_replace(
    commands: &mut Commands,
    parent: Entity,
    cx: ElementCx,
    seed: &str,
) -> FindReplaceParts {
    let font_size = cx.font_size;
    let body = commands
        .spawn((
            Node {
                padding: UiRect::all(Val::Px(8.0)),
                ..column(Val::Px(6.0))
            },
            ChildOf(parent),
        ))
        .id();
    let field_row =
        |commands: &mut Commands, key: &'static str, element: &'static str, initial: &str| {
            let row = spawn_labeled_row(
                commands,
                body,
                LabeledRowSpec::new(UiLabel::key(key)).label_min_width(Val::Px(64.0)),
            )
            .row;
            spawn_text_input(
                commands,
                row,
                &TextInputSpec {
                    initial: initial.to_owned(),
                    font_size,
                    width_glyphs: 24.0,
                    ..TextInputSpec::new(element, TextInputKind::Line)
                },
            )
        };
    let search = field_row(commands, "find-replace-search", "find-replace-search", seed);
    let replace = field_row(commands, "find-replace-replace", "find-replace-replace", "");
    let checks = commands
        .spawn((
            Node {
                ..row(Val::Px(12.0))
            },
            ChildOf(body),
        ))
        .id();
    let check = |commands: &mut Commands, element: &'static str| {
        spawn_checkbox(
            commands,
            checks,
            &CheckboxSpec {
                element,
                label: element.to_owned(),
                tab_index: 0,
                font_size,
                translate_label: true,
            },
        )
        .checkbox
    };
    let case_insensitive = check(commands, "find-replace-case-insensitive");
    let search_up = check(commands, "find-replace-search-up");
    let buttons = commands
        .spawn((
            Node {
                ..row(Val::Px(8.0))
            },
            ChildOf(body),
        ))
        .id();
    for (key, action) in [
        ("find-replace-search-button", FindAction::Search),
        ("find-replace-replace-button", FindAction::Replace),
        ("find-replace-replace-all-button", FindAction::ReplaceAll),
    ] {
        let button = spawn_button(
            commands,
            buttons,
            ButtonSpec::bordered(UiLabel::key(key), key)
                .tab_index(0)
                .font_size(font_size),
        )
        .button;
        commands
            .entity(button)
            .insert(action)
            .observe(on_find_action);
    }
    FindReplaceParts {
        search,
        replace,
        case_insensitive,
        search_up,
    }
}

/// Open the Search / Replace window for each [`OpenFindReplace`], seeded with
/// the editor's selection when it has one — what the reference does with a
/// selected word.
fn open_find_replace(
    mut opens: MessageReader<OpenFindReplace>,
    mut floaters: KeyedFloaters,
    mut windows: Query<&mut FindReplaceWindow>,
    mut fields: Query<&mut EditableText>,
    host: HostLookup,
    mut engine: TextEngine,
    mut commands: Commands,
) {
    for open in opens.read() {
        let selected = fields
            .get(open.field)
            .ok()
            .and_then(|field| field.editor.selected_text())
            .filter(|text| !text.contains('\n'))
            .map(str::to_owned);
        let (owner, key) =
            picker_identity(open.field, "find-replace", &host.parents, &host.floaters);
        let opened = floaters.open(find_replace_floater_spec(), key);
        let search = match opened {
            KeyedFloaterOpen::Spawned(handle) => {
                let parts = build_find_replace(
                    &mut commands,
                    handle.content,
                    ElementCx::new(),
                    selected.as_deref().unwrap_or_default(),
                );
                title(&mut commands, &handle, "find-replace-title");
                commands.entity(handle.root).insert(FindReplaceWindow {
                    field: open.field,
                    search: parts.search,
                    replace: parts.replace,
                    case_insensitive: parts.case_insensitive,
                    search_up: parts.search_up,
                });
                if let Some(owner) = owner {
                    commands.entity(handle.root).insert(FloaterOwner(owner));
                }
                parts.search
            }
            KeyedFloaterOpen::Existing(root) => {
                let Ok(mut window) = windows.get_mut(root) else {
                    continue;
                };
                window.field = open.field;
                // A window already up is re-seeded in place; a new one was
                // built with the seed, its field not existing until now.
                if let Some(selected) = selected.as_deref()
                    && let Ok(mut field) = fields.get_mut(window.search)
                {
                    set_field_text(
                        &mut field,
                        selected,
                        &mut engine.font_cx,
                        &mut engine.layout_cx,
                    );
                }
                window.search
            }
        };
        engine.focus.set(search, FocusCause::Navigated);
    }
}

/// The text engine's two contexts, and the keyboard focus a jump moves — what
/// every system that edits a field from outside it needs together.
#[derive(SystemParam)]
struct TextEngine<'w> {
    /// Parley's font context.
    font_cx: ResMut<'w, FontCx>,
    /// Parley's layout context.
    layout_cx: ResMut<'w, LayoutCx>,
    /// Which node has the keyboard.
    focus: ResMut<'w, InputFocus>,
}

/// What finds the window a node sits in.
#[derive(SystemParam)]
struct HostLookup<'w, 's> {
    /// Every node's parent.
    parents: Query<'w, 's, &'static ChildOf>,
    /// Every window.
    floaters: Query<'w, 's, (Entity, &'static Floater)>,
}

impl HostLookup<'_, '_> {
    /// The window `entity` sits in.
    fn host(&self, entity: Entity) -> Option<Entity> {
        crate::floater::host_floater(entity, &self.parents, &self.floaters)
    }
}

/// What a Search / Replace window's controls hold.
#[derive(SystemParam)]
struct WindowReads<'w, 's> {
    /// The two fields' text.
    texts: Query<'w, 's, &'static EditableText>,
    /// Which boxes are ticked.
    checked: Query<'w, 's, (), With<Checked>>,
}

impl WindowReads<'_, '_> {
    /// What `window` asks of its editor when `action` is pressed.
    fn request(&self, window: &FindReplaceWindow, action: FindAction) -> FindRequest {
        let read = |entity: Entity| {
            self.texts
                .get(entity)
                .map(|field| field.value().to_string())
                .unwrap_or_default()
        };
        FindRequest {
            field: window.field,
            needle: read(window.search),
            replacement: read(window.replace),
            case_sensitive: !self.checked.contains(window.case_insensitive),
            up: self.checked.contains(window.search_up),
            action,
        }
    }
}

/// Bind a new window's title to its Fluent key.
fn title(commands: &mut Commands, handle: &FloaterHandle, key: &'static str) {
    commands
        .entity(handle.title_text)
        .insert(Translated::new(key));
}

/// Replace a single-line field's text and put the caret at its end — the
/// programmatic set that does not race a queued edit (see the `sl-cef` notes on
/// `set_editor_text`).
fn set_field_text(
    editable: &mut EditableText,
    text: &str,
    font_cx: &mut FontCx,
    layout_cx: &mut LayoutCx,
) {
    editable.editor.set_text(text);
    let mut driver = editable.editor.driver(font_cx, layout_cx);
    driver.refresh_layout();
    driver.move_to_text_end();
}

/// What a Search / Replace window asks of its editor.
#[derive(Debug, Clone, PartialEq, Eq)]
struct FindRequest {
    /// The editor's field.
    field: Entity,
    /// What to look for.
    needle: String,
    /// What to put in its place.
    replacement: String,
    /// Whether case matters.
    case_sensitive: bool,
    /// Whether to search up.
    up: bool,
    /// What to do.
    action: FindAction,
}

/// A Search / Replace button was pressed: read the window and act on its
/// editor.
fn on_find_action(
    press: On<Pointer<Press>>,
    actions: Query<&FindAction>,
    host: HostLookup,
    windows: Query<&FindReplaceWindow>,
    reads: WindowReads,
    mut requests: MessageWriter<FindReplaceRequest>,
) {
    if press.button != PointerButton::Primary {
        return;
    }
    let Ok(action) = actions.get(press.entity) else {
        return;
    };
    let Some(window) = host
        .host(press.entity)
        .and_then(|root| windows.get(root).ok())
    else {
        return;
    };
    requests.write(FindReplaceRequest(reads.request(window, *action)));
}

/// `Enter` in a Search / Replace window's search field searches.
fn on_find_enter(
    event: On<FocusedInput<KeyboardInput>>,
    windows: Query<&FindReplaceWindow>,
    reads: WindowReads,
    mut requests: MessageWriter<FindReplaceRequest>,
) {
    if !event.input.state.is_pressed() || event.input.logical_key != Key::Enter {
        return;
    }
    let Some(window) = windows
        .iter()
        .find(|window| window.search == event.focused_entity)
    else {
        return;
    };
    requests.write(FindReplaceRequest(
        reads.request(window, FindAction::Search),
    ));
}

/// A search or replace to make in an editor.
#[derive(Message, Debug, Clone, PartialEq, Eq)]
struct FindReplaceRequest(FindRequest);

/// The selection of `editable` as a byte range, lowest first.
fn selection_range(editable: &EditableText) -> Range<usize> {
    let selection = editable.editor.raw_selection();
    let (anchor, focus) = (selection.anchor().index(), selection.focus().index());
    anchor.min(focus)..anchor.max(focus)
}

/// Select the match of `query` after (or, `up`, before) the selection in
/// `editable` and scroll it into view; `false` when there is none.
fn select_next_match(
    editable: &mut EditableText,
    node: &ComputedNode,
    scroll: &mut TextScroll,
    query: SearchQuery<'_>,
    up: bool,
    fonts: (&mut FontCx, &mut LayoutCx),
) -> bool {
    let selected = selection_range(editable);
    let text = editable.editor.raw_text();
    let found = if up {
        find_previous(text, query, selected.start)
    } else {
        find_next(text, query, selected.end)
    };
    let Some(found) = found else {
        return false;
    };
    let (font_cx, layout_cx) = fonts;
    editable
        .editor
        .driver(font_cx, layout_cx)
        .select_byte_range(found.start, found.end);
    reveal_caret(editable, node, scroll);
    true
}

/// The editors a search reads and moves through: the buffer, where it sits,
/// how far it is scrolled, its last search, and whether it may be changed.
type SearchedEditors<'world, 'state> = Query<
    'world,
    'state,
    (
        &'static mut EditableText,
        &'static ComputedNode,
        &'static mut TextScroll,
        &'static mut CodeEditor,
        Has<ReadOnlyField>,
    ),
>;

/// Make each search or replace a window or `F3` asked for.
///
/// A replace goes through the buffer like any edit, so the field's undo history
/// records it — Replace All as one step, which is what undoing it should take.
fn make_find_requests(
    mut requests: MessageReader<FindReplaceRequest>,
    mut repeats: MessageReader<RepeatSearch>,
    mut fields: SearchedEditors,
    mut font_cx: ResMut<FontCx>,
    mut layout_cx: ResMut<LayoutCx>,
) {
    let repeated: Vec<FindRequest> = repeats
        .read()
        .filter_map(|repeat| {
            let (_text, _node, _scroll, editor, _read_only) = fields.get(repeat.field).ok()?;
            let last = editor.last_search.clone()?;
            Some(FindRequest {
                field: repeat.field,
                needle: last.needle,
                replacement: String::new(),
                case_sensitive: last.case_sensitive,
                up: repeat.up,
                action: FindAction::Search,
            })
        })
        .collect();
    for request in requests
        .read()
        .map(|request| request.0.clone())
        .chain(repeated)
    {
        let Ok((mut editable, node, mut scroll, mut editor, read_only)) =
            fields.get_mut(request.field)
        else {
            continue;
        };
        if request.needle.is_empty() {
            continue;
        }
        editor.last_search = Some(LastSearch {
            needle: request.needle.clone(),
            case_sensitive: request.case_sensitive,
        });
        let query = SearchQuery::new(&request.needle, request.case_sensitive);
        match request.action {
            FindAction::Search => {
                let _found = select_next_match(
                    &mut editable,
                    node,
                    &mut scroll,
                    query,
                    request.up,
                    (&mut font_cx, &mut layout_cx),
                );
            }
            FindAction::Replace => {
                if read_only {
                    continue;
                }
                // The selection is replaced only when it *is* a match, as the
                // reference does: Replace on a fresh window first finds.
                let selected = selection_range(&editable);
                let is_match = editable
                    .editor
                    .raw_text()
                    .get(selected.clone())
                    .is_some_and(|text| find_all(text, query).first() == Some(&(0..text.len())));
                if is_match {
                    editable
                        .editor
                        .driver(&mut font_cx, &mut layout_cx)
                        .insert_or_replace_selection(&request.replacement);
                }
                let _found = select_next_match(
                    &mut editable,
                    node,
                    &mut scroll,
                    query,
                    request.up,
                    (&mut font_cx, &mut layout_cx),
                );
            }
            FindAction::ReplaceAll => {
                if read_only {
                    continue;
                }
                let (replaced, count) =
                    replace_all(editable.editor.raw_text(), query, &request.replacement);
                if count == 0 {
                    continue;
                }
                let end = editable.editor.raw_text().len();
                let mut driver = editable.editor.driver(&mut font_cx, &mut layout_cx);
                driver.select_byte_range(0, end);
                driver.insert_or_replace_selection(&replaced);
                driver.move_to_text_start();
            }
        }
    }
}

/// Wash every match of an open Search / Replace window's query in its editor,
/// and clear the wash when the window closes.
fn mark_find_matches(
    windows: Query<(&FindReplaceWindow, &UiPanelShown)>,
    texts: Query<Ref<EditableText>>,
    checked: Query<(), With<Checked>>,
    mut marks: Query<(Entity, &mut RichTextMarks), With<CodeEditor>>,
) {
    for (field, mut field_marks) in &mut marks {
        let window = windows
            .iter()
            .find(|(window, shown)| window.field == field && shown.0)
            .map(|(window, _shown)| *window);
        let wanted = window.and_then(|window| {
            let needle = texts.get(window.search).ok()?.value().to_string();
            let text = texts.get(field).ok()?;
            let query = SearchQuery::new(&needle, !checked.contains(window.case_insensitive));
            Some(
                find_all(text.editor.raw_text(), query)
                    .into_iter()
                    .map(|range| RichTextBackdrop {
                        range,
                        class: FIND_MATCH_CLASS,
                    })
                    .collect::<Vec<_>>(),
            )
        });
        let wanted = wanted.unwrap_or_default();
        if field_marks.backdrops != wanted {
            field_marks.backdrops = wanted;
        }
    }
}

// ---------------------------------------------------------------------------
// Go to Line window.
// ---------------------------------------------------------------------------

/// A Go to Line window's controls, on its root.
#[derive(Component, Debug, Clone, Copy)]
struct GoToLineWindow {
    /// The editor's field it moves the caret in.
    field: Entity,
    /// The line-number field.
    line: Entity,
}

/// Build the Go to Line window's content under `parent`: the number and OK.
/// Returns the number field.
fn build_go_to_line(commands: &mut Commands, parent: Entity, cx: ElementCx) -> Entity {
    let row = commands
        .spawn((
            Node {
                padding: UiRect::all(Val::Px(8.0)),
                align_items: AlignItems::Center,
                ..row(Val::Px(8.0))
            },
            ChildOf(parent),
        ))
        .id();
    let line = spawn_labeled_row(
        commands,
        row,
        LabeledRowSpec::new(UiLabel::key("go-to-line-label")),
    )
    .row;
    let field = spawn_text_input(
        commands,
        line,
        &TextInputSpec {
            font_size: cx.font_size,
            width_glyphs: 8.0,
            ..TextInputSpec::new("go-to-line-number", TextInputKind::NonNegativeInteger)
        },
    );
    let button = spawn_button(
        commands,
        row,
        ButtonSpec::bordered(UiLabel::key("go-to-line-ok"), "go-to-line-ok")
            .tab_index(0)
            .font_size(cx.font_size),
    )
    .button;
    commands.entity(button).observe(on_go_to_ok);
    field
}

/// Open the Go to Line window for each [`OpenGoToLine`].
fn open_go_to_line(
    mut opens: MessageReader<OpenGoToLine>,
    mut floaters: KeyedFloaters,
    mut windows: Query<&mut GoToLineWindow>,
    host: HostLookup,
    mut focus: ResMut<InputFocus>,
    mut commands: Commands,
) {
    for open in opens.read() {
        let (owner, key) = picker_identity(open.field, "go-to-line", &host.parents, &host.floaters);
        let line = match floaters.open(go_to_line_floater_spec(), key) {
            KeyedFloaterOpen::Spawned(handle) => {
                let line = build_go_to_line(&mut commands, handle.content, ElementCx::new());
                title(&mut commands, &handle, "go-to-line-title");
                commands.entity(handle.root).insert(GoToLineWindow {
                    field: open.field,
                    line,
                });
                if let Some(owner) = owner {
                    commands.entity(handle.root).insert(FloaterOwner(owner));
                }
                line
            }
            KeyedFloaterOpen::Existing(root) => {
                let Ok(mut window) = windows.get_mut(root) else {
                    continue;
                };
                window.field = open.field;
                window.line
            }
        };
        focus.set(line, FocusCause::Navigated);
    }
}

/// Go to the typed line from a Go to Line window, and close it.
fn go_from_window(
    window_root: Entity,
    windows: &Query<&GoToLineWindow>,
    texts: &Query<&EditableText>,
    go_to: &mut MessageWriter<CodeEditorGoTo>,
    shown: &mut Query<&mut UiPanelShown>,
) {
    let Ok(window) = windows.get(window_root) else {
        return;
    };
    let Some(line) = texts
        .get(window.line)
        .ok()
        .and_then(|field| field.value().to_string().parse::<usize>().ok())
    else {
        return;
    };
    go_to.write(CodeEditorGoTo {
        field: window.field,
        line: line.max(1),
        column: 1,
    });
    if let Ok(mut panel) = shown.get_mut(window_root) {
        panel.0 = false;
    }
}

/// The OK button of a Go to Line window.
fn on_go_to_ok(
    press: On<Pointer<Press>>,
    host: HostLookup,
    windows: Query<&GoToLineWindow>,
    texts: Query<&EditableText>,
    mut go_to: MessageWriter<CodeEditorGoTo>,
    mut shown: Query<&mut UiPanelShown>,
) {
    if press.button != PointerButton::Primary {
        return;
    }
    if let Some(root) = host.host(press.entity) {
        go_from_window(root, &windows, &texts, &mut go_to, &mut shown);
    }
}

/// `Enter` in a Go to Line window's number field goes.
fn on_go_to_enter(
    event: On<FocusedInput<KeyboardInput>>,
    host: HostLookup,
    windows: Query<&GoToLineWindow>,
    texts: Query<&EditableText>,
    mut go_to: MessageWriter<CodeEditorGoTo>,
    mut shown: Query<&mut UiPanelShown>,
) {
    if !event.input.state.is_pressed() || event.input.logical_key != Key::Enter {
        return;
    }
    if !windows
        .iter()
        .any(|window| window.line == event.focused_entity)
    {
        return;
    }
    if let Some(root) = host.host(event.focused_entity) {
        go_from_window(root, &windows, &texts, &mut go_to, &mut shown);
    }
}

// ---------------------------------------------------------------------------
// Specimens.
// ---------------------------------------------------------------------------

/// The sample source the specimen shows.
const SPECIMEN_SOURCE: &str = "default\n{\n    state_entry()\n    {\n        \
                               llSay(0, \"Hello, Avatar!\");\n    }\n}\n";

/// A code editor holding a short script, for the gallery and the layout sweep.
pub fn spawn_code_editor_specimen(
    commands: &mut Commands,
    parent: Entity,
    cx: ElementCx,
) -> Entity {
    let column = commands
        .spawn((
            Node {
                ..column(Val::ZERO)
            },
            ChildOf(parent),
        ))
        .id();
    let _editor = spawn_code_editor(
        commands,
        column,
        &CodeEditorSpec {
            initial: SPECIMEN_SOURCE.to_owned(),
            font_size: cx.font_size,
            visible_lines: 8.0,
            name_key: Some("code-editor-specimen-name"),
            ..CodeEditorSpec::new("code-editor")
        },
    );
    column
}

/// The Search / Replace window's content, for the floater sweep.
pub fn spawn_find_replace_specimen(
    commands: &mut Commands,
    parent: Entity,
    cx: ElementCx,
) -> Entity {
    let _parts = build_find_replace(commands, parent, cx, "");
    parent
}

/// The Go to Line window's content, for the floater sweep.
pub fn spawn_go_to_line_specimen(commands: &mut Commands, parent: Entity, cx: ElementCx) -> Entity {
    let _line = build_go_to_line(commands, parent, cx);
    parent
}

// ---------------------------------------------------------------------------
// Plugin.
// ---------------------------------------------------------------------------

/// The plugin owning the code editor: its gutter, its keys, go-to and the
/// Search / Replace and Go to Line windows.
#[derive(Debug, Clone, Copy, Default)]
pub struct CodeEditorPlugin;

impl Plugin for CodeEditorPlugin {
    fn build(&self, app: &mut App) {
        if !app.is_plugin_added::<crate::ui_rich_text::RichTextPlugin>() {
            app.add_plugins(crate::ui_rich_text::RichTextPlugin);
        }
        // Its two windows are keyed floaters, opened through the manager.
        if !app.is_plugin_added::<crate::floater::FloaterPlugin>() {
            app.add_plugins(crate::floater::FloaterPlugin);
        }
        app.add_message::<CodeEditorGoTo>()
            .add_message::<OpenFindReplace>()
            .add_message::<OpenGoToLine>()
            .add_message::<RepeatSearch>()
            .add_message::<FindReplaceRequest>()
            .add_systems(
                Update,
                (
                    // After the manager's command pass, for the reason every
                    // opener is: the raise of the new window must win.
                    (open_find_replace, open_go_to_line).after(FloaterSystems::Commands),
                    make_find_requests,
                    go_to_line,
                    mark_find_matches,
                )
                    .chain(),
            )
            .add_systems(
                PostUpdate,
                sync_code_gutters.after(bevy::ui::UiSystems::PostLayout),
            )
            .add_observer(on_code_editor_key)
            .add_observer(on_find_enter)
            .add_observer(on_go_to_enter);
    }
}

#[cfg(test)]
mod tests {
    use super::{
        CodeEditorGoTo, CodeEditorHandle, CodeEditorPlugin, CodeEditorSpec, FIND_MATCH_CLASS,
        FindAction, FindReplaceRequest, FindReplaceWindow, FindRequest, GutterNumber,
        OpenFindReplace, gutter_digits, gutter_width, spawn_code_editor,
    };
    use crate::floater::FloaterPlugin;
    use crate::text_search::line_column_offset;
    use crate::ui_rich_text::RichTextMarks;
    use crate::ui_test::interact::{InteractionTest, focus, tap, with_modifier};
    use crate::ui_test::{settle, spawn_under_root};
    use bevy::input::keyboard::Key;
    use bevy::input_focus::InputFocus;
    use bevy::prelude::*;
    use bevy::text::EditableText;
    use pretty_assertions::assert_eq;

    /// A boxed error so a test can use `?` instead of `unwrap`.
    type TestError = Box<dyn core::error::Error>;

    /// The script the fixtures edit: five lines, `llSay` on two of them.
    const SCRIPT: &str = "default\n{\n    llSay(0, \"a\");\n    llSay(1, \"b\");\n}";

    /// An app with the interaction stack, the floater manager and a code
    /// editor holding [`SCRIPT`].
    fn editor_app() -> (App, CodeEditorHandle) {
        let mut app = InteractionTest::new().build();
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
                    initial: SCRIPT.to_owned(),
                    visible_lines: 10.0,
                    ..CodeEditorSpec::new("code-editor-test")
                },
            )
        };
        app.world_mut().flush();
        settle(&mut app);
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

    /// The field's selection, lowest offset first.
    fn selection_of(app: &App, field: Entity) -> Result<core::ops::Range<usize>, TestError> {
        let selection = app
            .world()
            .get::<EditableText>(field)
            .ok_or("the field lost its editor")?
            .editor
            .raw_selection();
        let (anchor, focus) = (selection.anchor().index(), selection.focus().index());
        Ok(anchor.min(focus)..anchor.max(focus))
    }

    /// Ask the editor for a search or replace, as the window's buttons do.
    fn request(app: &mut App, field: Entity, needle: &str, replacement: &str, action: FindAction) {
        app.world_mut()
            .write_message(FindReplaceRequest(FindRequest {
                field,
                needle: needle.to_owned(),
                replacement: replacement.to_owned(),
                case_sensitive: true,
                up: false,
                action,
            }));
        settle(app);
    }

    /// The gutter is three digits wide until the script outgrows them.
    #[test]
    fn the_gutter_grows_with_the_line_count() {
        assert_eq!(gutter_digits(1), 3);
        assert_eq!(gutter_digits(999), 3);
        assert_eq!(gutter_digits(1000), 4);
        assert_eq!(gutter_digits(10_000), 5);
        assert!(gutter_width(14.0, 4) > gutter_width(14.0, 3));
    }

    /// **Every line gets its number, top to bottom, level with its line.**
    #[test]
    fn the_gutter_numbers_every_line_in_order() -> Result<(), TestError> {
        let (mut app, _handle) = editor_app();
        // Read top to bottom by where each number is drawn.
        let mut shown: Vec<(f32, String)> = app
            .world_mut()
            .query_filtered::<(&Text, &UiGlobalTransform, &Node), With<GutterNumber>>()
            .iter(app.world())
            .filter(|(_text, _transform, node)| node.display != Display::None)
            .map(|(text, transform, _node)| (transform.translation.y, text.0.clone()))
            .collect();
        shown.sort_by(|a, b| a.0.total_cmp(&b.0));
        let labels: Vec<&str> = shown.iter().map(|(_y, label)| label.as_str()).collect();
        assert_eq!(labels, vec!["1", "2", "3", "4", "5"]);
        let tops: Vec<f32> = shown.iter().map(|(y, _label)| *y).collect();
        assert!(
            tops.windows(2).all(|pair| pair.first() < pair.get(1)),
            "two numbers share a line: {tops:?}"
        );
        Ok(())
    }

    /// **A go-to puts the caret on the line and column, and the keyboard on the
    /// editor** — a compile error's row is clicked from outside the field.
    #[test]
    fn a_go_to_moves_the_caret_and_focuses_the_editor() -> Result<(), TestError> {
        let (mut app, handle) = editor_app();
        app.world_mut().write_message(CodeEditorGoTo {
            field: handle.field,
            line: 4,
            column: 5,
        });
        settle(&mut app);
        let wanted = line_column_offset(SCRIPT, 4, 5);
        assert_eq!(selection_of(&app, handle.field)?, wanted..wanted);
        assert_eq!(
            app.world().resource::<InputFocus>().get(),
            Some(handle.field)
        );
        Ok(())
    }

    /// **Replace All replaces every match and undoes as one step.**
    #[test]
    fn replace_all_is_one_undo_step() -> Result<(), TestError> {
        let (mut app, handle) = editor_app();
        request(
            &mut app,
            handle.field,
            "llSay",
            "llOwnerSay",
            FindAction::ReplaceAll,
        );
        assert_eq!(
            text_of(&app, handle.field)?,
            SCRIPT.replace("llSay", "llOwnerSay")
        );

        focus(&mut app, handle.field);
        with_modifier(&mut app, KeyCode::ControlLeft, Key::Control, |app| {
            tap(app, KeyCode::KeyZ, Key::Character("z".into()));
        });
        settle(&mut app);
        assert_eq!(text_of(&app, handle.field)?, SCRIPT);

        with_modifier(&mut app, KeyCode::ControlLeft, Key::Control, |app| {
            tap(app, KeyCode::KeyY, Key::Character("y".into()));
        });
        settle(&mut app);
        assert_eq!(
            text_of(&app, handle.field)?,
            SCRIPT.replace("llSay", "llOwnerSay")
        );
        Ok(())
    }

    /// **Search selects the next match, and `F3` finds the one after it.**
    #[test]
    fn f3_repeats_the_last_search() -> Result<(), TestError> {
        let (mut app, handle) = editor_app();
        request(&mut app, handle.field, "llSay", "", FindAction::Search);
        let first = SCRIPT.find("llSay").ok_or("no first match")?;
        assert_eq!(selection_of(&app, handle.field)?, first..first + 5);

        focus(&mut app, handle.field);
        tap(&mut app, KeyCode::F3, Key::F3);
        settle(&mut app);
        let second = SCRIPT.rfind("llSay").ok_or("no second match")?;
        assert_eq!(selection_of(&app, handle.field)?, second..second + 5);
        Ok(())
    }

    /// **Replace replaces the selected match only, then moves on.**
    #[test]
    fn replace_takes_the_selected_match_and_finds_the_next() -> Result<(), TestError> {
        let (mut app, handle) = editor_app();
        // Nothing selected: the first press only finds.
        request(
            &mut app,
            handle.field,
            "llSay",
            "llWhisper",
            FindAction::Replace,
        );
        assert_eq!(text_of(&app, handle.field)?, SCRIPT);
        // Now the first match is selected, and it is replaced.
        request(
            &mut app,
            handle.field,
            "llSay",
            "llWhisper",
            FindAction::Replace,
        );
        assert_eq!(
            text_of(&app, handle.field)?,
            SCRIPT.replacen("llSay", "llWhisper", 1)
        );
        Ok(())
    }

    /// **The Search / Replace window opens seeded with the selection, and every
    /// match is washed while it is up.**
    #[test]
    fn the_find_window_seeds_its_search_and_washes_the_matches() -> Result<(), TestError> {
        let (mut app, handle) = editor_app();
        let first = SCRIPT.find("llSay").ok_or("no match")?;
        let field = handle.field;
        app.world_mut()
            .resource_scope(|world, mut font_cx: Mut<bevy::text::FontCx>| {
                world.resource_scope(|world, mut layout_cx: Mut<bevy::text::LayoutCx>| {
                    if let Some(mut editable) = world.get_mut::<EditableText>(field) {
                        editable
                            .editor
                            .driver(&mut font_cx, &mut layout_cx)
                            .select_byte_range(first, first + 5);
                    }
                });
            });
        app.world_mut().write_message(OpenFindReplace {
            field: handle.field,
        });
        settle(&mut app);
        settle(&mut app);

        let search = app
            .world_mut()
            .query::<&FindReplaceWindow>()
            .iter(app.world())
            .next()
            .ok_or("no Search / Replace window opened")?
            .search;
        assert_eq!(text_of(&app, search)?, "llSay");
        let marks = app
            .world()
            .get::<RichTextMarks>(handle.field)
            .ok_or("the editor has no marks")?;
        assert_eq!(marks.backdrops.len(), 2);
        assert!(
            marks
                .backdrops
                .iter()
                .all(|backdrop| backdrop.class == FIND_MATCH_CLASS)
        );
        Ok(())
    }
}
