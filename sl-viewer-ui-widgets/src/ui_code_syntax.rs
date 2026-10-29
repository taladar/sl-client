//! What a **code editor** draws from its language's structure
//! (`viewer-lsl-editor-highlight`): syntax colour, brace matching, folding,
//! auto-indent and `Tab`.
//!
//! The language is not this module's business. A consumer puts a
//! [`CodeHighlighter`] on the editor's field — a function from the buffer to an
//! [`sl_highlight::CodeStructure`] — and the editor calls it whenever the buffer
//! changes, in the frame it changes, so the colours never lag the text. The
//! structure is language-neutral, so the LSL editor and a later Lua one differ
//! only in the function.
//!
//! # Colour
//!
//! Each highlighted range is drawn in the **skin class of its capture**,
//! `sk-syntax-<capture>` (`sk-syntax-function-builtin`), which the stylesheet
//! resolves through a `--syntax-*` token like any other text role. So a scheme
//! is a skin decision — a light skin ships light-appropriate colours — and
//! bold keywords or italic comments are a stylesheet's choice, not this
//! module's. A deprecated symbol is drawn in `sk-syntax-deprecated` instead.
//!
//! # Brackets
//!
//! While the field has focus, the bracket at the caret and its partner get a
//! box behind them (`sk-bracket-match`); a bracket with no partner gets
//! `sk-bracket-unmatched`. The caret prefers the bracket after it.
//!
//! # Folds
//!
//! A folded range is **hidden, not removed**: its characters stay in the buffer
//! (a save sees them, an undo restores around them) and are drawn as
//! [`RichTextStyle::Hidden`] — no glyph, no advance, and lines of nothing but
//! hidden text collapse to no height. A `⋯` box sits where the text was, and a
//! click on it unfolds. The gutter carries a toggle on every line a fold starts
//! on, and `Ctrl+Shift+[` / `Ctrl+Shift+]` fold and unfold at the caret.
//!
//! The caret steps **over** a fold rather than into it: moved into one, it lands
//! on the far side in the direction it was going. An edit that lands inside one
//! (a Replace All, an undo) unfolds it instead, so a change is never invisible.
//!
//! # Indent
//!
//! `Enter` carries the line's indentation to the new line, one level deeper
//! after an opening bracket — and between a bracket pair (`{|}`) opens a blank
//! indented line with the closer below it. `}` typed into a line's indentation
//! takes the indentation of the line its partner is on. `Tab` inserts one level
//! (or indents every line a selection touches) and `Shift+Tab` takes one level
//! off; in a code editor `Tab` is an edit, never a focus move.

use core::fmt;
use core::ops::Range;
use std::sync::Arc;

use bevy::input::keyboard::{Key, KeyboardInput};
use bevy::input_focus::{FocusedInput, InputFocus};
use bevy::prelude::*;
use bevy::text::{EditableText, EditableTextSystems, FontCx, LayoutCx, TextEdit};
use bevy_flair::style::components::ClassList;
use sl_highlight::{BracketKind, Capture, CodeStructure, Fold};

use sl_viewer_ui_core::skin_palette::SkinPalette;
use sl_viewer_ui_core::ui_font::UiFont;

use crate::ui_code_assist::{CodeAssistState, accept_on_tab};
use crate::ui_code_editor::CodeEditor;
use crate::ui_rich_text::{
    RichTextBackdrop, RichTextClass, RichTextContent, RichTextField, RichTextMarks, RichTextObject,
    RichTextRange, RichTextStyle,
};
use crate::ui_text_history::{ChordModifiers, HistoryEdit, HistorySelection, apply_history_edit};
use crate::ui_text_input::ReadOnlyField;

/// The skin class of a deprecated symbol (`--syntax-deprecated`).
pub const DEPRECATED_CLASS: &str = "sk-syntax-deprecated";

/// The skin class of the box behind a matched bracket pair
/// (`--bracket-match`).
pub const BRACKET_MATCH_CLASS: &str = "sk-bracket-match";

/// The skin class of the box behind a bracket nothing matches
/// (`--bracket-unmatched`).
pub const BRACKET_UNMATCHED_CLASS: &str = "sk-bracket-unmatched";

/// The skin class of the `⋯` box a folded range is drawn as
/// (`--fold-placeholder`).
pub const FOLD_PLACEHOLDER_CLASS: &str = "sk-fold-placeholder";

/// The skin class of a fold toggle in the gutter (`--line-number`).
pub const FOLD_TOGGLE_CLASS: &str = "sk-fold-toggle";

/// What a folded range is drawn as.
const FOLD_PLACEHOLDER_TEXT: &str = "⋯";

/// The skin class a capture is drawn in: `sk-syntax-` and its
/// [`slug`](Capture::slug).
#[must_use]
pub const fn syntax_class(capture: Capture) -> &'static str {
    match capture {
        Capture::Comment => "sk-syntax-comment",
        Capture::String => "sk-syntax-string",
        Capture::StringEscape => "sk-syntax-string-escape",
        Capture::Number => "sk-syntax-number",
        Capture::Keyword => "sk-syntax-keyword",
        Capture::Type => "sk-syntax-type",
        Capture::Function => "sk-syntax-function",
        Capture::FunctionBuiltin => "sk-syntax-function-builtin",
        Capture::FunctionEvent => "sk-syntax-function-event",
        Capture::Constant => "sk-syntax-constant",
        Capture::ConstantBuiltin => "sk-syntax-constant-builtin",
        Capture::Label => "sk-syntax-label",
        Capture::Namespace => "sk-syntax-namespace",
        Capture::Operator => "sk-syntax-operator",
        Capture::PunctuationBracket => "sk-syntax-punctuation-bracket",
        Capture::PunctuationDelimiter => "sk-syntax-punctuation-delimiter",
        Capture::PunctuationSpecial => "sk-syntax-punctuation-special",
        Capture::Error => "sk-syntax-error",
    }
}

/// The style classes every code editor's field carries: one per capture, in
/// [`Capture::ALL`] order, then the deprecated one — so a capture's class index
/// is its [`Capture::index`].
#[must_use]
pub fn syntax_classes() -> Vec<RichTextClass> {
    let fallback = SkinPalette::default().text_primary;
    Capture::ALL
        .iter()
        .map(|capture| RichTextClass::skinned(syntax_class(*capture), fallback))
        .chain([RichTextClass::skinned(DEPRECATED_CLASS, fallback)])
        .collect()
}

/// The class index a deprecated symbol is drawn in: the one after the captures.
const fn deprecated_class_index() -> usize {
    Capture::ALL.len()
}

/// The function a code editor reads its buffer's structure from — the language.
///
/// Called with the whole buffer whenever it changes (and whenever this
/// component is replaced, which is how a consumer re-highlights after its
/// language's tables change). It runs inside the frame, so it has to be fast: a
/// lexer pass, not a parse.
#[derive(Component, Clone)]
pub struct CodeHighlighter(Arc<dyn Fn(&str) -> CodeStructure + Send + Sync>);

impl CodeHighlighter {
    /// A highlighter calling `highlight`.
    pub fn new(highlight: impl Fn(&str) -> CodeStructure + Send + Sync + 'static) -> Self {
        Self(Arc::new(highlight))
    }

    /// The structure of `source`.
    #[must_use]
    pub fn structure(&self, source: &str) -> CodeStructure {
        (self.0)(source)
    }
}

impl fmt::Debug for CodeHighlighter {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("CodeHighlighter(..)")
    }
}

/// What a code editor knows about its buffer's structure, and what of it the
/// resident has folded — on the field.
#[derive(Component, Debug, Default)]
pub struct CodeSyntax {
    /// The structure of [`source`](Self::source).
    structure: CodeStructure,
    /// The buffer the structure was made from.
    source: String,
    /// The headers of the folded folds.
    folded: Vec<usize>,
    /// The `⋯` box of each folded fold, by header.
    placeholders: Vec<(usize, Entity)>,
    /// Where the caret was at the end of the last frame, to tell which way it
    /// moved into a fold.
    caret: usize,
    /// Whether the field's content has to be drawn again.
    dirty: bool,
    /// A `Tab` / `Shift+Tab` over lines, waiting for the frame's edits.
    indent: Option<IndentRequest>,
}

impl CodeSyntax {
    /// The structure of the buffer as it last was.
    #[must_use]
    pub const fn structure(&self) -> &CodeStructure {
        &self.structure
    }

    /// The buffer the structure was made from.
    #[must_use]
    pub fn source(&self) -> &str {
        &self.source
    }

    /// The headers of the folded folds.
    #[must_use]
    pub fn folded(&self) -> &[usize] {
        &self.folded
    }

    /// The fold whose header is `header`, if the structure has one.
    fn fold(&self, header: usize) -> Option<&Fold> {
        self.structure
            .folds
            .iter()
            .find(|fold| fold.header == header)
    }

    /// The folded fold the caret at `caret` is strictly inside, if any.
    fn folded_around(&self, caret: usize) -> Option<&Fold> {
        self.folded
            .iter()
            .filter_map(|header| self.fold(*header))
            .find(|fold| fold.hidden.start < caret && caret < fold.hidden.end)
    }
}

/// Which way a `Tab` over lines goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum IndentRequest {
    /// One level more on every line.
    Indent,
    /// One level less on every line.
    Dedent,
}

/// Fold or unfold in a code editor.
#[derive(Message, Debug, Clone, Copy, PartialEq, Eq)]
pub struct CodeFold {
    /// The editor's field.
    pub field: Entity,
    /// What to do.
    pub action: FoldAction,
}

/// What a [`CodeFold`] does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FoldAction {
    /// Fold the fold with this header, or unfold it if it is folded.
    Toggle(usize),
    /// Fold the innermost fold around the caret.
    FoldAtCaret,
    /// Unfold every fold around the caret.
    UnfoldAtCaret,
}

/// A `⋯` box, naming the fold it stands for.
#[derive(Component, Debug, Clone, Copy)]
struct FoldPlaceholder {
    /// The editor's field.
    field: Entity,
    /// The fold's header.
    header: usize,
}

// ---------------------------------------------------------------------------
// The structure and the folds.
// ---------------------------------------------------------------------------

/// Where `old` and `new` differ: the byte range of `old` that was replaced and
/// how long its replacement is — a common prefix and suffix, which is exact for
/// one edit and safe (if wide) for several.
fn changed_span(old: &str, new: &str) -> (Range<usize>, usize) {
    let prefix = old
        .bytes()
        .zip(new.bytes())
        .take_while(|(a, b)| a == b)
        .count();
    let room = old.len().min(new.len()).saturating_sub(prefix);
    let suffix = old
        .bytes()
        .rev()
        .zip(new.bytes().rev())
        .take(room)
        .take_while(|(a, b)| a == b)
        .count();
    let old_end = old.len().saturating_sub(suffix);
    let new_len = new.len().saturating_sub(suffix).saturating_sub(prefix);
    (prefix..old_end, new_len)
}

/// Where a fold header at `header` in the old buffer is in the new one, or
/// `None` when the edit replaced it.
const fn remap(header: usize, changed: &Range<usize>, inserted: usize) -> Option<usize> {
    if header < changed.start {
        Some(header)
    } else if header >= changed.end {
        Some(
            header
                .saturating_sub(changed.end.saturating_sub(changed.start))
                .saturating_add(inserted),
        )
    } else {
        None
    }
}

/// Bring each code editor's structure up to its buffer, keep its folds in
/// step, step the caret over a fold, and draw the result into its content.
///
/// After the frame's edits (and an undo or an indent made after them), so the
/// structure is the buffer's as it now stands; before the rich-text model is
/// handed to parley, so this frame's layout has this frame's colours.
#[expect(
    clippy::type_complexity,
    reason = "the field query is the editor, its language, what it knows and what it draws"
)]
pub(crate) fn highlight_code_editors(
    mut fields: Query<(
        Entity,
        &mut EditableText,
        &RichTextField,
        &CodeEditor,
        Ref<CodeHighlighter>,
        &mut CodeSyntax,
        &mut RichTextContent,
    )>,
    mut folds: MessageReader<CodeFold>,
    mut font_cx: ResMut<FontCx>,
    mut layout_cx: ResMut<LayoutCx>,
    mut commands: Commands,
) {
    let requests: Vec<CodeFold> = folds.read().copied().collect();
    for (field, mut editable, rich, editor, highlighter, mut syntax, mut content) in &mut fields {
        let syntax = &mut *syntax;
        let text_changed = syntax.source != editable.editor.raw_text();
        if text_changed || highlighter.is_changed() {
            let text = editable.editor.raw_text();
            let (changed, inserted) = changed_span(&syntax.source, text);
            syntax.structure = highlighter.structure(text);
            let structure = &syntax.structure;
            syntax.folded = syntax
                .folded
                .iter()
                .filter_map(|header| remap(*header, &changed, inserted))
                .filter(|header| {
                    // A fold the edit landed inside unfolds: a change is never
                    // made out of sight.
                    let new_change = changed.start..changed.start.saturating_add(inserted);
                    structure.folds.iter().any(|fold| {
                        fold.header == *header
                            && (new_change.end <= fold.hidden.start
                                || new_change.start >= fold.hidden.end
                                || !text_changed)
                    })
                })
                .collect();
            text.clone_into(&mut syntax.source);
            syntax.dirty = true;
        }
        let caret = editable.editor.raw_selection().focus().index();
        for request in requests.iter().filter(|request| request.field == field) {
            apply_fold_action(syntax, request.action, caret);
        }
        if let Some(fold) = syntax.folded_around(caret).cloned() {
            if text_changed {
                syntax.folded.retain(|header| *header != fold.header);
                syntax.dirty = true;
            } else {
                let target = if caret >= syntax.caret {
                    fold.hidden.end
                } else {
                    fold.hidden.start
                };
                editable
                    .editor
                    .driver(&mut font_cx, &mut layout_cx)
                    .move_to_byte(target);
            }
        }
        syntax.caret = editable.editor.raw_selection().focus().index();
        if !syntax.dirty {
            continue;
        }
        syntax.dirty = false;
        sync_placeholders(
            syntax,
            field,
            rich.overlay,
            editor.font_size(),
            &mut commands,
        );
        let wanted = drawn_content(syntax, &content);
        if *content != wanted {
            *content = wanted;
        }
    }
}

/// Make one fold action on `syntax`, the caret at `caret`.
fn apply_fold_action(syntax: &mut CodeSyntax, action: FoldAction, caret: usize) {
    match action {
        FoldAction::Toggle(header) => {
            if syntax.folded.contains(&header) {
                syntax.folded.retain(|folded| *folded != header);
            } else if syntax.fold(header).is_some() {
                syntax.folded.push(header);
            }
        }
        FoldAction::FoldAtCaret => {
            let innermost = syntax
                .structure
                .folds
                .iter()
                .filter(|fold| {
                    fold.header <= caret
                        && caret <= fold.hidden.end
                        && !syntax.folded.contains(&fold.header)
                })
                .map(|fold| fold.header)
                .max();
            if let Some(header) = innermost {
                syntax.folded.push(header);
            }
        }
        FoldAction::UnfoldAtCaret => {
            let structure = &syntax.structure;
            syntax.folded.retain(|header| {
                !structure.folds.iter().any(|fold| {
                    fold.header == *header && *header <= caret && caret <= fold.hidden.end
                })
            });
        }
    }
    syntax.dirty = true;
}

/// Spawn a `⋯` box for every folded fold that has none, and despawn the boxes
/// of folds no longer folded.
fn sync_placeholders(
    syntax: &mut CodeSyntax,
    field: Entity,
    overlay: Entity,
    font_size: f32,
    commands: &mut Commands,
) {
    let folded = syntax.folded.clone();
    syntax.placeholders.retain(|(header, entity)| {
        let keep = folded.contains(header);
        if !keep {
            commands.entity(*entity).despawn();
        }
        keep
    });
    for header in folded {
        if syntax
            .placeholders
            .iter()
            .any(|(placed, _entity)| *placed == header)
        {
            continue;
        }
        let entity = commands
            .spawn((
                Node {
                    position_type: PositionType::Absolute,
                    padding: UiRect::horizontal(Val::Px(3.0)),
                    ..default()
                },
                // Hidden but laid out, as every rich-text object starts, so it
                // is measured before parley places it.
                Visibility::Hidden,
                Text::new(FOLD_PLACEHOLDER_TEXT),
                UiFont::Mono.at(font_size),
                TextColor(SkinPalette::default().text_muted),
                BackgroundColor(Color::NONE),
                ClassList::new_with_classes([FOLD_PLACEHOLDER_CLASS]),
                FoldPlaceholder { field, header },
                Name::new("code-fold-placeholder"),
                ChildOf(overlay),
            ))
            .observe(on_placeholder_click)
            .id();
        syntax.placeholders.push((header, entity));
    }
}

/// What a code editor's content is: its highlights in their classes, its
/// folded ranges hidden behind their boxes, and whatever backdrops the content
/// already carried.
fn drawn_content(syntax: &CodeSyntax, current: &RichTextContent) -> RichTextContent {
    let mut ranges: Vec<RichTextRange> = syntax
        .structure
        .highlights
        .iter()
        .map(|highlight| RichTextRange {
            range: highlight.range.clone(),
            style: RichTextStyle::Class(if highlight.deprecated {
                deprecated_class_index()
            } else {
                highlight.capture.index()
            }),
        })
        .collect();
    let mut objects = Vec::new();
    for (header, entity) in &syntax.placeholders {
        let Some(fold) = syntax.fold(*header) else {
            continue;
        };
        // After the classes, so the hidden style is the last word on its
        // range.
        ranges.push(RichTextRange {
            range: fold.hidden.clone(),
            style: RichTextStyle::Hidden,
        });
        objects.push(RichTextObject {
            entity: *entity,
            index: fold.hidden.start,
        });
    }
    RichTextContent {
        objects,
        ranges,
        backdrops: current.backdrops.clone(),
    }
}

/// A click on a `⋯` box unfolds its fold.
fn on_placeholder_click(
    mut click: On<Pointer<Click>>,
    placeholders: Query<&FoldPlaceholder>,
    mut folds: MessageWriter<CodeFold>,
) {
    let Ok(placeholder) = placeholders.get(click.entity) else {
        return;
    };
    click.propagate(false);
    folds.write(CodeFold {
        field: placeholder.field,
        action: FoldAction::Toggle(placeholder.header),
    });
}

// ---------------------------------------------------------------------------
// Brackets.
// ---------------------------------------------------------------------------

/// The boxes a code editor draws behind its text: the find matches its find
/// window asked for, then the bracket pair at the caret while it has focus.
fn mark_code_brackets(
    focus: Res<InputFocus>,
    mut fields: Query<(
        Entity,
        &EditableText,
        &CodeEditor,
        Option<&CodeSyntax>,
        &mut RichTextMarks,
    )>,
) {
    for (field, editable, editor, syntax, mut marks) in &mut fields {
        let mut wanted = editor.find_marks().to_vec();
        let selection = editable.editor.raw_selection();
        if focus.get() == Some(field)
            && selection.is_collapsed()
            && let Some(syntax) = syntax
            && syntax.source == editable.editor.raw_text()
        {
            wanted.extend(bracket_marks(syntax.structure(), selection.focus().index()));
        }
        if marks.backdrops != wanted {
            marks.backdrops = wanted;
        }
    }
}

/// The bracket boxes for a caret at `caret`: the pair it is beside, or the
/// unmatched bracket it is beside.
fn bracket_marks(structure: &CodeStructure, caret: usize) -> Vec<RichTextBackdrop> {
    if let Some(pair) = structure.brackets.pair_at_caret(caret) {
        return vec![
            RichTextBackdrop {
                range: pair.open.clone(),
                class: BRACKET_MATCH_CLASS,
            },
            RichTextBackdrop {
                range: pair.close.clone(),
                class: BRACKET_MATCH_CLASS,
            },
        ];
    }
    let beside = |index: usize| {
        structure
            .brackets
            .unmatched()
            .iter()
            .find(|bracket| bracket.range.contains(&index))
    };
    beside(caret)
        .or_else(|| caret.checked_sub(1).and_then(beside))
        .map(|bracket| RichTextBackdrop {
            range: bracket.range.clone(),
            class: BRACKET_UNMATCHED_CLASS,
        })
        .into_iter()
        .collect()
}

// ---------------------------------------------------------------------------
// Indent.
// ---------------------------------------------------------------------------

/// The indentation a line opens with: its leading spaces and tabs.
fn leading_indent(line: &str) -> &str {
    let end = line
        .char_indices()
        .find(|(_offset, character)| *character != ' ' && *character != '\t')
        .map_or(line.len(), |(offset, _character)| offset);
    line.get(..end).unwrap_or_default()
}

/// Where the line holding byte `index` of `text` starts.
fn line_start_of(text: &str, index: usize) -> usize {
    text.get(..index)
        .and_then(|head| head.rfind('\n'))
        .map_or(0, |newline| newline.saturating_add(1))
}

/// The closer that ends what `opener` opens.
const fn closer_of(opener: char) -> Option<char> {
    match opener {
        '{' => Some('}'),
        '(' => Some(')'),
        '[' => Some(']'),
        _ => None,
    }
}

/// The edits an `Enter` at `selection` in `text` comes to, carrying the line's
/// indentation and adding `unit` after an opener — or `None` when a bare
/// newline is right.
fn newline_edits(text: &str, selection: Range<usize>, unit: &str) -> Option<Vec<TextEdit>> {
    let line_start = line_start_of(text, selection.start);
    let before = text.get(line_start..selection.start)?;
    let indent = leading_indent(before);
    let opener = before
        .trim_end()
        .chars()
        .next_back()
        .filter(|last| closer_of(*last).is_some());
    let after = text
        .get(selection.end..)
        .map(|rest| rest.split('\n').next().unwrap_or_default().trim_start())
        .unwrap_or_default();
    let between = opener
        .and_then(closer_of)
        .is_some_and(|closer| after.starts_with(closer));
    if opener.is_none() && indent.is_empty() {
        return None;
    }
    let inner = if opener.is_some() {
        format!("{indent}{unit}")
    } else {
        indent.to_owned()
    };
    if between {
        let insert = format!("\n{inner}\n{indent}");
        let back = indent.chars().count().saturating_add(1);
        let mut edits = vec![TextEdit::Insert(insert.into())];
        edits.extend(core::iter::repeat_n(TextEdit::Left(false), back));
        return Some(edits);
    }
    Some(vec![TextEdit::Insert(format!("\n{inner}").into())])
}

/// The edits a `}` typed at `caret` in `text` comes to when the caret is in the
/// line's indentation: the indentation replaced by its partner's line's, then
/// the `}` — or `None` when the line has code before the caret or already has
/// the right indentation.
fn closer_edits(text: &str, caret: usize, structure: &CodeStructure) -> Option<Vec<TextEdit>> {
    let line_start = line_start_of(text, caret);
    let before = text.get(line_start..caret)?;
    if !before
        .chars()
        .all(|character| character == ' ' || character == '\t')
    {
        return None;
    }
    let partner = structure.brackets.open_at(caret, BracketKind::Curly)?;
    let partner_line = line_start_of(text, partner.start);
    let target = leading_indent(text.get(partner_line..partner.start)?);
    if target == before {
        return None;
    }
    let mut edits: Vec<TextEdit> =
        core::iter::repeat_n(TextEdit::Backspace, before.chars().count()).collect();
    edits.push(TextEdit::Insert(format!("{target}}}").into()));
    Some(edits)
}

/// Rewrite a code editor's `Enter` and `}` into their indenting forms, before
/// `bevy_text` applies them.
///
/// Only a lone edit is rewritten — the common case of one key in a frame —
/// because the offsets it is worked out from are the buffer's before *any* of
/// the frame's edits; two keys in one frame type plainly.
pub(crate) fn indent_code_keystrokes(
    mut fields: Query<
        (&mut EditableText, &CodeEditor, Option<&CodeSyntax>),
        Without<ReadOnlyField>,
    >,
) {
    for (mut editable, editor, syntax) in &mut fields {
        let rewritten = {
            let field = editable.as_ref();
            let [TextEdit::Insert(inserted)] = field.pending_edits.as_slice() else {
                continue;
            };
            if field.is_composing() {
                continue;
            }
            let text = field.editor.raw_text();
            let selection = field.editor.raw_selection().text_range();
            match inserted.as_str() {
                "\n" => newline_edits(text, selection, editor.indent_unit()),
                "}" if selection.is_empty() => syntax
                    .filter(|syntax| syntax.source == text)
                    .and_then(|syntax| closer_edits(text, selection.start, syntax.structure())),
                _ => None,
            }
        };
        if let Some(edits) = rewritten {
            editable.pending_edits = edits;
        }
    }
}

/// `Tab` and `Shift+Tab` in a code editor: an edit, never a focus move.
///
/// An entity observer on the field, so it runs before the window's tab
/// navigation the key would otherwise bubble up to — and stops it there. A
/// read-only editor lets `Tab` move focus on, as any field does. While the
/// completion list is open, `Tab` accepts its entry instead
/// ([`crate::ui_code_assist`]).
pub(crate) fn on_code_tab(
    mut event: On<FocusedInput<KeyboardInput>>,
    keys: Res<ButtonInput<KeyCode>>,
    mut fields: Query<
        (
            &mut EditableText,
            &CodeEditor,
            &mut CodeSyntax,
            Option<&mut CodeAssistState>,
        ),
        Without<ReadOnlyField>,
    >,
) {
    if event.input.logical_key != Key::Tab {
        return;
    }
    let Ok((mut editable, editor, mut syntax, assist)) = fields.get_mut(event.focused_entity)
    else {
        return;
    };
    event.propagate(false);
    if !event.input.state.is_pressed() || editable.is_composing() {
        return;
    }
    let held = ChordModifiers::read(&keys);
    if held.ctrl || held.alt {
        return;
    }
    if !held.shift && accept_on_tab(assist.map(Mut::into_inner)) {
        return;
    }
    let selection = editable.editor.raw_selection().text_range();
    let spans_lines = editable
        .editor
        .raw_text()
        .get(selection.clone())
        .is_some_and(|selected| selected.contains('\n'));
    if held.shift {
        syntax.indent = Some(IndentRequest::Dedent);
    } else if spans_lines {
        syntax.indent = Some(IndentRequest::Indent);
    } else {
        editable.queue_edit(TextEdit::Insert(editor.indent_unit().into()));
    }
}

/// The lines a selection over `selection` touches, as the byte range from the
/// first one's start to the last one's end. A selection that ends at the very
/// start of a line does not touch that line.
fn touched_lines(text: &str, selection: &Range<usize>) -> Range<usize> {
    let start = line_start_of(text, selection.start);
    let last =
        if selection.end > selection.start && selection.end == line_start_of(text, selection.end) {
            selection.end.saturating_sub(1)
        } else {
            selection.end
        };
    let end = text
        .get(last..)
        .and_then(|rest| rest.find('\n'))
        .map_or(text.len(), |offset| last.saturating_add(offset));
    start..end.max(start)
}

/// `lines` with one `unit` more (`Indent`) or one level less (`Dedent`) on
/// each. A blank line gains nothing; a level less is up to one unit's worth of
/// spaces, or one tab.
fn reindent(lines: &str, unit: &str, request: IndentRequest) -> String {
    lines
        .split('\n')
        .map(|line| match request {
            IndentRequest::Indent if line.trim().is_empty() => line.to_owned(),
            IndentRequest::Indent => format!("{unit}{line}"),
            IndentRequest::Dedent => {
                let indent = leading_indent(line);
                let take = if indent.starts_with('\t') {
                    1
                } else {
                    indent
                        .chars()
                        .take_while(|character| *character == ' ')
                        .take(unit.chars().count().max(1))
                        .count()
                };
                line.get(take..).unwrap_or_default().to_owned()
            }
        })
        .collect::<Vec<String>>()
        .join("\n")
}

/// Make each code editor's waiting `Tab` / `Shift+Tab` over lines: re-indent
/// every line its selection touches, as one edit (one undo step), and select
/// the lines.
fn apply_indent_requests(
    mut fields: Query<(&mut EditableText, &CodeEditor, &mut CodeSyntax)>,
    mut font_cx: ResMut<FontCx>,
    mut layout_cx: ResMut<LayoutCx>,
) {
    for (mut editable, editor, mut syntax) in &mut fields {
        let Some(request) = syntax.indent.take() else {
            continue;
        };
        let text = editable.editor.raw_text();
        let selection = editable.editor.raw_selection().text_range();
        let lines = touched_lines(text, &selection);
        let Some(original) = text.get(lines.clone()) else {
            continue;
        };
        let replaced = reindent(original, editor.indent_unit(), request);
        if replaced == original {
            continue;
        }
        let collapsed = selection.is_empty();
        let end = lines.start.saturating_add(replaced.len());
        // A lone caret keeps its place in its text; a selection becomes the
        // lines, so the next Tab moves the same block.
        let selected = if collapsed {
            let moved = selection
                .start
                .saturating_add(replaced.len())
                .saturating_sub(original.len())
                .max(lines.start);
            HistorySelection {
                anchor: moved,
                focus: moved,
            }
        } else {
            HistorySelection {
                anchor: lines.start,
                focus: end,
            }
        };
        let edit = HistoryEdit {
            range: lines,
            text: replaced,
            selection: selected,
        };
        apply_history_edit(&mut editable, &edit, &mut font_cx, &mut layout_cx);
    }
}

/// Register the systems and messages of this module on `app` — called by the
/// code editor's plugin.
pub(crate) fn register(app: &mut App) {
    app.add_message::<CodeFold>().add_systems(
        PostUpdate,
        (
            indent_code_keystrokes.before(EditableTextSystems),
            (
                apply_indent_requests,
                highlight_code_editors,
                mark_code_brackets,
            )
                .chain()
                .after(EditableTextSystems)
                .after(crate::ui_text_history::apply_history_requests)
                .before(crate::ui_rich_text::RichTextSystems::Model)
                .before(bevy::ui::UiSystems::PostLayout),
        ),
    );
}

#[cfg(test)]
mod tests {
    use super::{
        IndentRequest, changed_span, closer_edits, newline_edits, reindent, remap, touched_lines,
    };
    use bevy::text::TextEdit;
    use pretty_assertions::assert_eq;
    use sl_highlight::{Bracket, BracketKind, BracketPairs, CodeStructure};

    /// The structure of `text` with every bracket character paired.
    fn brackets_of(text: &str) -> CodeStructure {
        CodeStructure {
            brackets: BracketPairs::new(text.char_indices().filter_map(|(index, character)| {
                BracketKind::of(character).map(|(kind, open)| Bracket {
                    range: index..index.saturating_add(1),
                    kind,
                    open,
                })
            })),
            ..CodeStructure::default()
        }
    }

    /// `Enter` keeps the indentation, adds a level after an opener, and between
    /// a pair opens a line with the closer below it.
    #[test]
    fn enter_indents() {
        assert_eq!(
            newline_edits("    x;", 6..6, "    "),
            Some(vec![TextEdit::Insert("\n    ".into())])
        );
        assert_eq!(
            newline_edits("  f() {", 7..7, "    "),
            Some(vec![TextEdit::Insert("\n      ".into())])
        );
        assert_eq!(
            newline_edits("{}", 1..1, "    "),
            Some(vec![
                TextEdit::Insert("\n    \n".into()),
                TextEdit::Left(false)
            ])
        );
        assert_eq!(newline_edits("x;", 2..2, "    "), None);
    }

    /// `}` in a line's indentation takes its partner's line's indentation.
    #[test]
    fn a_closer_dedents_to_its_partner() {
        let text = "  f() {\n      x;\n      ";
        let structure = brackets_of(text);
        assert_eq!(
            closer_edits(text, text.len(), &structure),
            Some(
                core::iter::repeat_n(TextEdit::Backspace, 6)
                    .chain([TextEdit::Insert("  }".into())])
                    .collect()
            )
        );
        assert_eq!(closer_edits("{\n  x; ", 7, &brackets_of("{\n  x; ")), None);
    }

    /// Indenting skips blank lines; dedenting takes up to one level, or a tab.
    #[test]
    fn lines_reindent() {
        assert_eq!(
            reindent("a\n\n  b", "    ", IndentRequest::Indent),
            "    a\n\n      b"
        );
        assert_eq!(
            reindent("      a\n\tb\nc", "    ", IndentRequest::Dedent),
            "  a\nb\nc"
        );
        assert_eq!(touched_lines("ab\ncd\nef", &(1..4)), 0..5);
        assert_eq!(touched_lines("ab\ncd\nef", &(1..3)), 0..2);
    }

    /// An edit before a fold moves it, one inside its header drops it.
    #[test]
    fn folds_follow_edits() {
        let (changed, inserted) = changed_span("abcdef", "abXXcdef");
        assert_eq!((changed.clone(), inserted), (2..2, 2));
        assert_eq!(remap(4, &changed, inserted), Some(6));
        assert_eq!(remap(1, &changed, inserted), Some(1));
        let (changed, inserted) = changed_span("abcdef", "abef");
        assert_eq!(remap(3, &changed, inserted), None);
        assert_eq!(remap(5, &changed, inserted), Some(3));
    }
}
