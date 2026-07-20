//! The **reusable text-input widget** (`viewer-ui-text-input-widget`): a
//! single-line and a multi-line field wrapping `bevy_text`'s [`EditableText`],
//! plus the three numeric variants of the single-line field — a signed decimal
//! (float), a signed integer, and a non-negative (unsigned) integer.
//!
//! This is the widget every text-entry surface consumes — chat input, IM, search
//! fields, the key-rebinding editor, the build window's numeric fields. It is the
//! plain-text counterpart to the syntax-highlighted, undo-capable editor of
//! [[viewer-lsl-editor-widget]](crate) (which cannot build on this, because
//! `parley::PlainEditor` under [`EditableText`] carries **one** style for the
//! whole buffer — that editor forks parley instead).
//!
//! # What is inherited, and what is built here
//!
//! The hard text behaviours are **inherited** from the foundation
//! ([`crate::ui_text`]) and parley, and are not re-implemented:
//!
//! - **Bidi** — the caret moves in visual order and the selection geometry splits
//!   across runs, from parley's Unicode Bidirectional Algorithm. The caret and
//!   selection API is already expressed logically (`move_left` / `move_right` map
//!   to *leading* / *trailing* under the run's direction), so an RTL field needs
//!   nothing here.
//! - **Grapheme-correct editing** — backspace deletes one grapheme cluster (the
//!   patched parley of the workspace `Cargo.toml`). Caret *motion* still steps one
//!   codepoint, a pre-existing upstream limitation tracked by
//!   [[viewer-ui-text-caret-grapheme-motion]] — nothing here depends on it.
//! - **IME** — `bevy_ui_widgets`' `EditableTextInputPlugin` (in `DefaultPlugins`)
//!   transports `Ime::Preedit` / `Ime::Commit`, drives `Window::ime_enabled` and
//!   the candidate-window position, and excludes the preedit from
//!   [`EditableText::value`] until commit. The *richer* clause-segmented preedit
//!   the reference viewer draws is blocked on winit exposing more than a single
//!   cursor range and on an IME-capable host — tracked by
//!   [[viewer-ui-text-ime-verification]], not undertaken here.
//! - **No tofu / colour emoji** — from [`crate::ui_font`]'s bundled stack.
//!
//! So what this module actually adds over a bare [`EditableText`] is threefold:
//! the **chrome** a field needs to read and behave as a field (a border, a
//! background, an intrinsic width, keyboard reachability), the **single- vs
//! multi-line** distinction, and — the real work — **numeric validation**.
//!
//! # Numeric validation: a character set plus a whole-string prevalidate
//!
//! The reference viewer's `LLLineEditor` rejects a keystroke whenever the
//! *resulting whole string* would not be a valid (or valid-*intermediate*)
//! number — its `LLTextValidate::validateFloat` / `validateInt` /
//! `validateNonNegativeS32`. A number is not a per-character property: `1.2.3` is
//! all legal float characters in an illegal arrangement, and `-` is fine only at
//! the front. So validation has to see the whole candidate string, which a
//! per-character predicate cannot.
//!
//! `bevy_text` gives us only half of that: [`EditableTextFilter`] is a
//! per-**character** filter, applied to every inserted or pasted char. We use it
//! for the cheap, flicker-free half — it blocks a letter in a number field the
//! instant it is typed, before it ever enters the buffer. The structural half —
//! at most one `.`, a `-` only at the front — is enforced by
//! [`enforce_numeric_intermediate`], which after each edit checks the whole value
//! against [`TextInputKind::accepts`] and, if it has become structurally invalid,
//! **reverts** it to the last valid value ([`NumericField::last_valid`]). The
//! revert runs after `bevy_text`'s `apply_text_edits`
//! ([`EditableTextSystems`](bevy::text::EditableTextSystems)) but *before* the
//! editable-text glyph layout (`UiSystems::PostLayout`), so the corrected buffer
//! is what gets laid out — the rejected keystroke never reaches the screen.
//!
//! The validators accept **intermediate** editing states a complete number is
//! reached through — an empty field, a lone `-`, a trailing `.` (`1.`) — because
//! a field the user cannot pass an intermediate state through is a field they
//! cannot type into. Reading a committed value out is [`TextInputKind::parse`]'s
//! job, and it returns `None` for those incomplete states.
//!
//! # Constructible without wiring
//!
//! Per the registry rule ([`crate::ui_element`]): a field holds and edits its own
//! text and reaches no session, so nothing here emits a [`UiAction`]. A consumer
//! that must react to a change reads [`EditableText::value`] (or reacts to
//! `Changed<EditableText>`); a consumer that wants the typed number calls
//! [`TextInputKind::parse`]. The gallery registers one element per variant
//! (`spawn_line_specimen` and friends) so every field is swept by
//! [`crate::ui_test`], and an `F8` demo panel exercises live typing, rejection
//! and the IME by hand.
//!
//! Reference (Firestorm, read-only): `lllineeditor`, `lltexteditor`,
//! `lltextvalidate` (`LLTextValidate::validate*`), `llpreeditor` (the IME model).

use bevy::input_focus::tab_navigation::TabIndex;
use bevy::prelude::*;
use bevy::text::{
    EditableText, EditableTextFilter, EditableTextSystems, FontCx, LayoutCx, TextCursorStyle,
};
use bevy::ui::UiSystems;

use crate::ui::{LogicalMargin, LogicalRect, UiPanelShown, UiRoot, column, row};
use crate::ui_element::{ElementCx, TextMayClip};
use crate::ui_font::UiFont;

/// A field's text colour.
const FIELD_TEXT_COLOR: Color = Color::WHITE;

/// A field's recessed background — darker than the surrounding panel, so the
/// editable area reads as a well the text sits in.
const FIELD_BACKGROUND: Color = Color::srgb(0.10, 0.12, 0.16);

/// A field's border.
const FIELD_BORDER: Color = Color::srgb(0.40, 0.50, 0.62);

/// A field's border width, in logical pixels.
const FIELD_BORDER_WIDTH: f32 = 2.0;

/// A field's inner padding, in logical pixels — a little breathing room between
/// the border and the first glyph / the caret.
const FIELD_PADDING: f32 = 6.0;

/// The default width of a single-line field, in `"0"`-glyph advances — the
/// idiomatic "N characters wide" sizing a text field wants. A field's intrinsic
/// control size is the sanctioned exception to the scaffold's no-fixed-width
/// convention (like [`TextMayClip`] is for its clipping check): it is *not* a
/// container of translatable prose, and it scrolls its content horizontally past
/// this width rather than growing.
const DEFAULT_WIDTH_GLYPHS: f32 = 16.0;

/// The default height of a multi-line field, in visible text lines.
const DEFAULT_VISIBLE_LINES: f32 = 3.0;

/// The widest a multi-line field's text is allowed to get before it wraps, in
/// logical pixels — a *bound*, not a size (the content-driven convention): the
/// field wraps its prose here rather than overflowing.
const MULTILINE_MAX_WIDTH: f32 = 360.0;

/// The default font size of a field's text, in logical pixels.
const DEFAULT_FONT_SIZE: f32 = 15.0;

/// Which kind of field this is: the free-text single- and multi-line fields, and
/// the three numeric single-line variants.
///
/// The numeric variants differ from each other in exactly two things a
/// [`TextInputSpec`] reads off this enum — the set of characters that may be
/// typed ([`Self::char_filter`]) and the whole-string shape that is a valid
/// intermediate ([`Self::accepts`]) — so a new variant is those two functions and
/// nothing else.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TextInputKind {
    /// Free-form single-line text: one line, no newlines, scrolls horizontally.
    Line,
    /// Free-form multi-line text: newlines allowed, soft-wraps, scrolls
    /// vertically.
    Multiline,
    /// A signed decimal number — an optional leading `-`, digits, and at most one
    /// decimal point (`-3.5`, `42`, `0.25`).
    Float,
    /// A signed integer — an optional leading `-` then digits (`-7`, `128`).
    Integer,
    /// A non-negative integer — digits only, no sign, so it accepts zero and up
    /// (`0`, `128`). The `-` key is rejected outright, the way a count or a
    /// dimension field wants.
    NonNegativeInteger,
}

impl TextInputKind {
    /// Whether this is the multi-line field: newlines allowed, soft-wrapping, and
    /// sized in visible lines rather than glyph-widths.
    const fn is_multiline(self) -> bool {
        matches!(self, Self::Multiline)
    }

    /// Whether this is one of the numeric variants — the ones that carry a
    /// [`NumericField`] and are enforced by [`enforce_numeric_intermediate`].
    const fn is_numeric(self) -> bool {
        matches!(self, Self::Float | Self::Integer | Self::NonNegativeInteger)
    }

    /// The per-character filter this kind installs as an [`EditableTextFilter`],
    /// or `None` for the free-text kinds, which accept any character.
    ///
    /// This is the cheap, flicker-free half of numeric validation: it blocks a
    /// disallowed *character* (a letter, a stray sign) the instant it is typed,
    /// before it enters the buffer. It cannot enforce *arrangement* (one decimal
    /// point, a sign only at the front) — that is [`Self::accepts`]'s job.
    fn char_filter(self) -> Option<fn(char) -> bool> {
        match self {
            Self::Line | Self::Multiline => None,
            Self::Float => Some(is_float_char),
            Self::Integer => Some(is_integer_char),
            Self::NonNegativeInteger => Some(is_digit_char),
        }
    }

    /// Whether `value` is a valid **intermediate** editing state for this kind —
    /// the whole-string prevalidate.
    ///
    /// The free-text kinds accept anything. The numeric kinds accept the states a
    /// complete number is reached *through* as well as complete ones: an empty
    /// field, a lone `-`, a trailing `.`. See the per-kind helpers for the exact
    /// shape. This is what [`enforce_numeric_intermediate`] holds the field to,
    /// reverting anything it rejects.
    fn accepts(self, value: &str) -> bool {
        match self {
            Self::Line | Self::Multiline => true,
            Self::Float => accepts_float_intermediate(value),
            Self::Integer => accepts_integer_intermediate(value),
            Self::NonNegativeInteger => accepts_unsigned_integer(value),
        }
    }

    /// Parse a **committed** value out of a field's text, or `None` when the text
    /// is not (yet) a complete number of this kind.
    ///
    /// Distinct from [`Self::accepts`], which admits the intermediate states a
    /// user types through: `parse` of a lone `-`, a bare `.`, or an empty field is
    /// `None`, because none is a number yet. A consumer reads the typed value with
    /// this when it needs one (on `Enter`, on focus loss). The free-text kinds
    /// have no numeric value and always return `None`.
    pub(crate) fn parse(self, value: &str) -> Option<TextInputValue> {
        match self {
            Self::Line | Self::Multiline => None,
            Self::Float => value.parse::<f64>().ok().map(TextInputValue::Float),
            Self::Integer => value.parse::<i64>().ok().map(TextInputValue::Integer),
            Self::NonNegativeInteger => value.parse::<u64>().ok().map(TextInputValue::Unsigned),
        }
    }
}

/// A committed numeric value read out of a field by [`TextInputKind::parse`],
/// typed to match the field's kind.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum TextInputValue {
    /// A [`TextInputKind::Float`] field's value.
    Float(f64),
    /// A [`TextInputKind::Integer`] field's value.
    Integer(i64),
    /// A [`TextInputKind::NonNegativeInteger`] field's value.
    Unsigned(u64),
}

/// Whether `c` may be typed into a [`TextInputKind::Float`] field — a digit, the
/// minus sign, or the decimal point. Arrangement is checked separately.
const fn is_float_char(c: char) -> bool {
    c.is_ascii_digit() || c == '-' || c == '.'
}

/// Whether `c` may be typed into a [`TextInputKind::Integer`] field — a digit or
/// the minus sign.
const fn is_integer_char(c: char) -> bool {
    c.is_ascii_digit() || c == '-'
}

/// Whether `c` may be typed into a [`TextInputKind::NonNegativeInteger`] field —
/// a digit and nothing else (no sign).
const fn is_digit_char(c: char) -> bool {
    c.is_ascii_digit()
}

/// Whether `value` is a valid intermediate signed-decimal string: an optional
/// leading `-`, then ASCII digits with **at most one** decimal point anywhere
/// among them.
///
/// Accepts the partial states a float is typed through — `""`, `"-"`, `"."`,
/// `"-."`, `"1."`, `"-.5"` — as well as complete ones. Rejects a second point
/// (`"1.2.3"`), an interior or trailing sign (`"1-2"`, `"5-"`), or any non-digit.
fn accepts_float_intermediate(value: &str) -> bool {
    let digits = value.strip_prefix('-').unwrap_or(value);
    let mut seen_point = false;
    for c in digits.chars() {
        if c == '.' {
            if seen_point {
                return false;
            }
            seen_point = true;
        } else if !c.is_ascii_digit() {
            return false;
        }
    }
    true
}

/// Whether `value` is a valid intermediate signed-integer string: an optional
/// leading `-` then ASCII digits.
///
/// Accepts `""`, `"-"`, `"5"`, `"-5"`; rejects an interior or trailing sign, a
/// decimal point, or any non-digit.
fn accepts_integer_intermediate(value: &str) -> bool {
    let digits = value.strip_prefix('-').unwrap_or(value);
    digits.chars().all(|c| c.is_ascii_digit())
}

/// Whether `value` is a valid non-negative integer string: ASCII digits only, no
/// sign. Accepts `""` and any run of digits; rejects a sign or a decimal point.
fn accepts_unsigned_integer(value: &str) -> bool {
    value.chars().all(|c| c.is_ascii_digit())
}

/// Everything a field is built from — a struct rather than a long positional call,
/// matching the neighbouring widgets ([`crate::ui_tab::TabSpec`]).
///
/// Build one with [`TextInputSpec::new`] and override fields with struct-update
/// syntax:
///
/// ```ignore
/// TextInputSpec {
///     initial: "128".to_owned(),
///     tab_index: 3,
///     ..TextInputSpec::new("build-pos-x", TextInputKind::Float)
/// }
/// ```
#[derive(Debug, Clone)]
pub(crate) struct TextInputSpec {
    /// The prefix of the field's node [`Name`], for the gallery and a test's
    /// lookups. Numeric fields do not emit a [`UiAction`], so this is not an
    /// action id.
    pub(crate) element: &'static str,
    /// Which kind of field this is — free text, or one of the numeric variants.
    pub(crate) kind: TextInputKind,
    /// The text the field starts with. Sanitised at spawn: an initial value the
    /// kind rejects is replaced by an empty field rather than seeding an invalid
    /// `last_valid`.
    pub(crate) initial: String,
    /// The field's focus stop, for slotting it into the surrounding tab order.
    pub(crate) tab_index: i32,
    /// The field text's font size, in logical pixels.
    pub(crate) font_size: f32,
    /// A single-line field's width, in `"0"`-glyph advances. Ignored for the
    /// multi-line kind, which sizes by [`visible_lines`](Self::visible_lines).
    pub(crate) width_glyphs: f32,
    /// A multi-line field's height, in visible text lines. Ignored for the
    /// single-line kinds.
    pub(crate) visible_lines: f32,
    /// A cap on the number of characters the field will hold, or `None` for no
    /// cap. Enforced by `bevy_text` itself ([`EditableText::max_characters`]).
    pub(crate) max_characters: Option<usize>,
    /// Whether the field draws its own border and background (`true`, the
    /// default). Set `false` to spawn a **bare** field for embedding inside a
    /// container that carries the chrome itself — the search-field widget
    /// ([`crate::ui_search`]) does this, decorating the box around the field
    /// rather than the field.
    pub(crate) decorated: bool,
    /// Whether a single-line field **flex-grows to fill** its parent instead of
    /// taking its intrinsic glyph-width (`false`, the default). A filled field
    /// has no `visible_width`; it takes the room its container gives it and
    /// scrolls. Ignored for the multi-line kind. Used by the search-field widget,
    /// whose box sets the width and lets the field fill it up to the clear button.
    pub(crate) fill: bool,
}

impl TextInputSpec {
    /// A spec for `element` of `kind`, with an empty initial value and the module
    /// defaults for size, width and height. Override the rest with struct-update
    /// syntax — see the [type documentation](Self).
    pub(crate) const fn new(element: &'static str, kind: TextInputKind) -> Self {
        Self {
            element,
            kind,
            initial: String::new(),
            tab_index: 0,
            font_size: DEFAULT_FONT_SIZE,
            width_glyphs: DEFAULT_WIDTH_GLYPHS,
            visible_lines: DEFAULT_VISIBLE_LINES,
            max_characters: None,
            decorated: true,
            fill: false,
        }
    }

    /// The initial text the field actually starts with: [`initial`](Self::initial)
    /// if the kind accepts it, otherwise empty — so a numeric field never seeds a
    /// `last_valid` its own validator would reject.
    fn sanitised_initial(&self) -> String {
        if self.kind.accepts(&self.initial) {
            self.initial.clone()
        } else {
            String::new()
        }
    }
}

/// A numeric field's structural-validation state: its kind and the last value that
/// passed [`TextInputKind::accepts`], which [`enforce_numeric_intermediate`]
/// reverts to when an edit makes the field structurally invalid.
///
/// Present only on the numeric variants; the free-text fields carry no filter and
/// no validator, so they never grow one.
#[derive(Component, Debug, Clone)]
pub(crate) struct NumericField {
    /// Which numeric kind this field is, so the enforcer knows which validator to
    /// apply.
    kind: TextInputKind,
    /// The most recent value that passed the validator — the field's fallback when
    /// the next edit is rejected.
    last_valid: String,
}

/// Spawn a text-input field of `spec`'s kind under `parent`, returning the field
/// entity (the [`EditableText`] node itself, which carries the chrome).
///
/// The field is reachable by `Tab`, draws a caret and selection, and — for the
/// numeric kinds — carries the [`EditableTextFilter`] (character set) and the
/// [`NumericField`] (structural validation) that together enforce the number
/// format. It holds and edits its own text and reaches no session; a consumer
/// reads the value with [`EditableText::value`] or [`TextInputKind::parse`].
pub(crate) fn spawn_text_input(
    commands: &mut Commands,
    parent: Entity,
    spec: &TextInputSpec,
) -> Entity {
    let multiline = spec.kind.is_multiline();
    let initial = spec.sanitised_initial();

    let mut editor = EditableText::new(&initial);
    editor.allow_newlines = multiline;
    editor.max_characters = spec.max_characters;
    if multiline {
        editor.visible_lines = Some(spec.visible_lines);
    } else {
        // A single line high. A filling field takes the width its container gives
        // it (no intrinsic width); an ordinary one is sized by glyph-width — a
        // fixed-size control that scrolls, not a label that grows.
        editor.visible_lines = Some(1.0);
        if !spec.fill {
            editor.visible_width = Some(spec.width_glyphs);
        }
    }

    // Numeric fields read better in the monospaced face (digits share an advance,
    // so a column of them lines up), matching the build window's numeric cells;
    // free text uses the proportional sans.
    let font = if spec.kind.is_numeric() {
        UiFont::Mono
    } else {
        UiFont::Sans
    };

    // Padding always (breathing room for the caret); the border only on a
    // decorated field, because a bare one is decorated by the container it sits in.
    let mut node = Node {
        padding: UiRect::all(Val::Px(FIELD_PADDING)),
        ..default()
    };
    if spec.decorated {
        node.border = UiRect::all(Val::Px(FIELD_BORDER_WIDTH));
    }
    // A multi-line field wraps its prose at a bound (convention 2); a single-line
    // field's width is its intrinsic control size (set on the editor above) unless
    // it fills, in which case it grows to its container and shrinks below its
    // content so the container's width — not the text — decides the field's.
    if multiline {
        node.max_width = Val::Px(MULTILINE_MAX_WIDTH);
    } else if spec.fill {
        node.flex_grow = 1.0;
        node.min_width = Val::Px(0.0);
    }

    // A field always slices its own text at the edge — a single-line field scrolls
    // horizontally to follow the caret, a multi-line one scrolls vertically — so it
    // claims the harness's clipping exception rather than being special-cased.
    let clip_reason = if multiline {
        "a multi-line field scrolls its content vertically to follow the caret, so text past \
         the last visible line is cut by design"
    } else {
        "a single-line field scrolls its content horizontally to follow the caret, so text past \
         the field width is cut by design"
    };

    let mut field = commands.spawn((
        editor,
        font.at(spec.font_size),
        TextColor(FIELD_TEXT_COLOR),
        TextCursorStyle::default(),
        TabIndex(spec.tab_index),
        node,
        TextMayClip {
            reason: clip_reason,
        },
        Name::new(format!("{}:field", spec.element)),
        ChildOf(parent),
    ));

    // A decorated field draws its own border and background; a bare one leaves
    // both to the container it is embedded in.
    if spec.decorated {
        field.insert((
            BorderColor::all(FIELD_BORDER),
            BackgroundColor(FIELD_BACKGROUND),
        ));
    }
    if let Some(filter) = spec.kind.char_filter() {
        field.insert(EditableTextFilter::new(filter));
    }
    if spec.kind.is_numeric() {
        field.insert(NumericField {
            kind: spec.kind,
            last_valid: initial,
        });
    }

    field.id()
}

/// The plugin for the widget's runtime half: the numeric structural validator.
///
/// A no-op where there are no numeric fields, so adding it is always safe — the
/// gallery and the viewer both add it. The character-set half needs no system
/// (`bevy_text` applies the [`EditableTextFilter`] itself); this plugin is only
/// the whole-string prevalidate the per-character filter cannot express.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct TextInputPlugin;

impl Plugin for TextInputPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            PostUpdate,
            enforce_numeric_intermediate
                // After `bevy_text` applies the frame's edits, but before the
                // editable-text glyph layout (`UiSystems::PostLayout`), so a
                // reverted buffer is what gets laid out and the rejected keystroke
                // never reaches the screen.
                .after(EditableTextSystems)
                .before(UiSystems::PostLayout),
        );
    }
}

/// Hold every numeric field to its kind's whole-string shape: after an edit, if
/// the field's value is no longer a valid intermediate
/// ([`TextInputKind::accepts`]), revert it to [`NumericField::last_valid`];
/// otherwise remember the new value as the fallback.
///
/// This is the structural half of numeric validation — the part
/// [`EditableTextFilter`]'s per-character check cannot do, because a second
/// decimal point or a misplaced sign is only visible in the whole string. Runs
/// only for fields whose [`EditableText`] changed this frame, and reverts by
/// [`set_text`](parley::PlainEditor::set_text) plus a caret-to-end, which is
/// picked up by the same frame's glyph layout.
fn enforce_numeric_intermediate(
    mut fields: Query<(&mut EditableText, &mut NumericField), Changed<EditableText>>,
    mut font_cx: ResMut<FontCx>,
    mut layout_cx: ResMut<LayoutCx>,
) {
    for (mut editable, mut field) in &mut fields {
        reconcile_numeric_field(&mut editable, &mut field, &mut font_cx, &mut layout_cx);
    }
}

/// Reconcile one numeric field after an edit: remember a valid value as the new
/// fallback, or revert an invalid one to [`NumericField::last_valid`] with the
/// caret at its end — the per-entity body of [`enforce_numeric_intermediate`],
/// split out so it can be driven by a headless test.
fn reconcile_numeric_field(
    editable: &mut EditableText,
    field: &mut NumericField,
    font_cx: &mut FontCx,
    layout_cx: &mut LayoutCx,
) {
    let current = editable.value().to_string();
    if field.kind.accepts(&current) {
        // A valid state (including a valid intermediate) becomes the new
        // fallback. Guarded so an unchanged value is not rewritten.
        if field.last_valid != current {
            field.last_valid = current;
        }
        return;
    }
    // The edit made the field structurally invalid — restore the last good value
    // and put the caret at its end, as if the keystroke had been rejected.
    let restore = field.last_valid.clone();
    editable.editor.set_text(&restore);
    let mut driver = editable.editor.driver(font_cx, layout_cx);
    driver.refresh_layout();
    driver.move_to_text_end();
}

// ---------------------------------------------------------------------------
// Gallery specimens — one registered element per variant, so every field is
// swept by `crate::ui_test` across every script, direction, scale and font size.
//
// The numeric specimens keep their literal digits (a number is not translated,
// exactly as `crate::ui_element::spawn_field_grid`'s cells are), while the
// free-text specimens take the matrix's sample string so their layout is checked
// in every writing system.
// ---------------------------------------------------------------------------

/// The sample prose a free-text field's specimen shows — long enough that a
/// multi-line field wraps and a single-line field scrolls.
const SAMPLE_TEXT: &str = "The quick brown fox jumps over the lazy dog.";

/// Spawn the single-line free-text field specimen.
pub(crate) fn spawn_line_specimen(
    commands: &mut Commands,
    parent: Entity,
    cx: ElementCx,
) -> Entity {
    spawn_text_input(
        commands,
        parent,
        &TextInputSpec {
            initial: cx.text(SAMPLE_TEXT),
            font_size: cx.font_size,
            ..TextInputSpec::new("text-input-line", TextInputKind::Line)
        },
    )
}

/// Spawn the multi-line free-text field specimen.
pub(crate) fn spawn_multiline_specimen(
    commands: &mut Commands,
    parent: Entity,
    cx: ElementCx,
) -> Entity {
    spawn_text_input(
        commands,
        parent,
        &TextInputSpec {
            initial: cx.text(SAMPLE_TEXT),
            font_size: cx.font_size,
            ..TextInputSpec::new("text-input-multiline", TextInputKind::Multiline)
        },
    )
}

/// Spawn the signed-decimal (float) field specimen. The value stays literal — a
/// number is not translated.
pub(crate) fn spawn_float_specimen(
    commands: &mut Commands,
    parent: Entity,
    cx: ElementCx,
) -> Entity {
    spawn_text_input(
        commands,
        parent,
        &TextInputSpec {
            initial: "-3.5".to_owned(),
            font_size: cx.font_size,
            ..TextInputSpec::new("text-input-float", TextInputKind::Float)
        },
    )
}

/// Spawn the signed-integer field specimen.
pub(crate) fn spawn_integer_specimen(
    commands: &mut Commands,
    parent: Entity,
    cx: ElementCx,
) -> Entity {
    spawn_text_input(
        commands,
        parent,
        &TextInputSpec {
            initial: "-42".to_owned(),
            font_size: cx.font_size,
            ..TextInputSpec::new("text-input-integer", TextInputKind::Integer)
        },
    )
}

/// Spawn the non-negative-integer field specimen.
pub(crate) fn spawn_unsigned_specimen(
    commands: &mut Commands,
    parent: Entity,
    cx: ElementCx,
) -> Entity {
    spawn_text_input(
        commands,
        parent,
        &TextInputSpec {
            initial: "128".to_owned(),
            font_size: cx.font_size,
            ..TextInputSpec::new("text-input-unsigned", TextInputKind::NonNegativeInteger)
        },
    )
}

// ---------------------------------------------------------------------------
// The live demo panel (`F6`, or `SL_VIEWER_TEXT_INPUT_DEMO` for the screenshot
// harness) — the by-hand proof surface, in the pattern of `crate::ui_text`'s `F4`
// text panel and `crate::ui`'s `F5` scaffold panel. It is where the numeric
// rejection, the IME and the single- / multi-line behaviours are exercised by a
// human, which no headless test reaches. Not registered in `ELEMENTS`: it is a
// hand-driven demonstration, not a swept element.
// ---------------------------------------------------------------------------

/// The key that toggles the text-input demo panel (F8).
const DEMO_TOGGLE_KEY: KeyCode = KeyCode::F8;

/// The environment variable that starts the demo panel shown, for the offline
/// screenshot harness (which cannot press [`DEMO_TOGGLE_KEY`]).
const DEMO_ENV: &str = "SL_VIEWER_TEXT_INPUT_DEMO";

/// The demo panel's margin, in logical pixels, from the leading and top edges of
/// the [`UiRoot`] — clear of the top-leading pipeline overlay, like the F4 / F5
/// panels.
const DEMO_PANEL_MARGIN: f32 = 90.0;

/// The demo panel's instruction-line font size, in logical pixels.
const DEMO_TITLE_FONT_SIZE: f32 = 13.0;

/// A demo row's label font size, in logical pixels.
const DEMO_LABEL_FONT_SIZE: f32 = 14.0;

/// The demo panel's translucent backdrop, matching the F4 / F5 panels.
const DEMO_PANEL_BACKGROUND: Color = Color::srgba(0.0, 0.0, 0.0, 0.7);

/// The demo panel's instruction-line colour.
const DEMO_TITLE_COLOR: Color = Color::srgb(0.80, 0.85, 0.92);

/// A demo row's label colour.
const DEMO_LABEL_COLOR: Color = Color::srgb(0.72, 0.78, 0.88);

/// The one-line instruction shown above the demo's fields.
const DEMO_TITLE: &str = "Text-input demo (F8) - Tab between the fields and type / use your IME. \
     The numeric fields reject a bad character as you type, and revert a bad arrangement (a \
     second '.', a misplaced '-'); the single-line field scrolls, the multi-line one wraps.";

/// Whether the demo panel is currently shown. Toggled by [`DEMO_TOGGLE_KEY`];
/// hidden by default.
#[derive(Resource, Debug, Clone, Copy, Default)]
pub(crate) struct TextInputDemoVisible(pub(crate) bool);

impl TextInputDemoVisible {
    /// The initial visibility, seeded from [`DEMO_ENV`]: set to start shown, unset
    /// to start hidden (the interactive default).
    pub(crate) fn from_env() -> Self {
        Self(std::env::var_os(DEMO_ENV).is_some())
    }
}

/// A marker on the demo panel's root node, so the toggle system can show / hide
/// the whole subtree.
#[derive(Component, Debug, Clone, Copy)]
pub(crate) struct TextInputDemoRoot;

/// A live read-out beside a numeric demo field, showing what
/// [`TextInputKind::parse`] currently makes of the field's text — the committed
/// value, or `(incomplete)` for an intermediate state a number is not reached yet.
/// Present only on the demo's numeric rows; it is what exercises `parse` and
/// [`TextInputValue`] against live input.
#[derive(Component, Debug, Clone, Copy)]
pub(crate) struct DemoValueReadout {
    /// The field whose value this read-out parses.
    field: Entity,
    /// The field's kind, so the read-out parses it the same way the field
    /// validates it.
    kind: TextInputKind,
}

/// Startup system: spawn the demo panel — a title over one labelled row per field
/// kind — starting shown or hidden per [`TextInputDemoVisible`]. Parents itself to
/// the scaffold's [`UiRoot`], so it must run after
/// [`crate::ui::UiScaffoldSystems::SpawnRoot`].
pub(crate) fn setup_text_input_demo(
    mut commands: Commands,
    visible: Res<TextInputDemoVisible>,
    root: Res<UiRoot>,
) {
    let display = if visible.0 {
        Display::Flex
    } else {
        Display::None
    };
    let panel = commands
        .spawn((
            Node {
                display,
                padding: UiRect::all(Val::Px(12.0)),
                max_width: Val::Px(MULTILINE_MAX_WIDTH + 80.0),
                ..column(Val::Px(8.0))
            },
            LogicalMargin(LogicalRect {
                inline_start: Val::Px(DEMO_PANEL_MARGIN),
                block_start: Val::Px(DEMO_PANEL_MARGIN),
                ..LogicalRect::ZERO
            }),
            BackgroundColor(DEMO_PANEL_BACKGROUND),
            UiPanelShown(visible.0),
            TextInputDemoRoot,
            ChildOf(root.0),
        ))
        .with_child((
            Text::new(DEMO_TITLE),
            UiFont::Sans.at(DEMO_TITLE_FONT_SIZE),
            TextColor(DEMO_TITLE_COLOR),
        ))
        .id();

    // One labelled row per kind, tab-ordered top to bottom. The multi-line field
    // is prefilled with prose; the numeric ones with valid sample values.
    let rows = [
        ("Single line", TextInputKind::Line, String::new()),
        (
            "Multi line",
            TextInputKind::Multiline,
            SAMPLE_TEXT.to_owned(),
        ),
        ("Float (+/-)", TextInputKind::Float, "-3.5".to_owned()),
        ("Integer (+/-)", TextInputKind::Integer, "-42".to_owned()),
        (
            "Positive integer",
            TextInputKind::NonNegativeInteger,
            "128".to_owned(),
        ),
    ];
    for (index, (label, kind, initial)) in rows.into_iter().enumerate() {
        let tab_index = i32::try_from(index).unwrap_or(0);
        spawn_demo_row(&mut commands, panel, label, kind, initial, tab_index);
    }
}

/// Spawn one labelled demo row: the label beside a field of `kind`, prefilled with
/// `initial` and slotted at `tab_index` in the panel's tab order.
fn spawn_demo_row(
    commands: &mut Commands,
    panel: Entity,
    label: &str,
    kind: TextInputKind,
    initial: String,
    tab_index: i32,
) {
    let row_entity = commands
        .spawn((
            Node {
                align_items: AlignItems::Center,
                ..row(Val::Px(8.0))
            },
            Name::new(format!("text-input-demo-row:{label}")),
            ChildOf(panel),
        ))
        .with_child((
            Text::new(label.to_owned()),
            UiFont::Sans.at(DEMO_LABEL_FONT_SIZE),
            TextColor(DEMO_LABEL_COLOR),
        ))
        .id();
    let field = spawn_text_input(
        commands,
        row_entity,
        &TextInputSpec {
            initial,
            tab_index,
            ..TextInputSpec::new("text-input-demo", kind)
        },
    );
    // A numeric row carries a live read-out of its parsed value, so the by-hand
    // tester can see what `parse` makes of what they type (and that an
    // intermediate state has no value yet).
    if kind.is_numeric() {
        commands.spawn((
            Text::new("= (incomplete)"),
            UiFont::Mono.at(DEMO_LABEL_FONT_SIZE),
            TextColor(DEMO_LABEL_COLOR),
            DemoValueReadout { field, kind },
            Name::new("text-input-demo-readout"),
            ChildOf(row_entity),
        ));
    }
}

/// Keep each numeric demo row's [`DemoValueReadout`] showing the live result of
/// [`TextInputKind::parse`] on its field's text. Inert where there is no demo (the
/// gallery has no read-outs), so it is safe to run everywhere.
pub(crate) fn update_demo_value_readouts(
    fields: Query<&EditableText>,
    mut readouts: Query<(&DemoValueReadout, &mut Text)>,
) {
    for (readout, mut text) in &mut readouts {
        let Ok(editable) = fields.get(readout.field) else {
            continue;
        };
        let value = editable.value().to_string();
        let shown = match readout.kind.parse(&value) {
            Some(TextInputValue::Float(number)) => format!("= {number}"),
            Some(TextInputValue::Integer(number)) => format!("= {number}"),
            Some(TextInputValue::Unsigned(number)) => format!("= {number}"),
            None => "= (incomplete)".to_owned(),
        };
        if text.0 != shown {
            shown.clone_into(&mut text.0);
        }
    }
}

/// Toggle the demo panel when [`DEMO_TOGGLE_KEY`] is pressed.
pub(crate) fn toggle_text_input_demo(
    keyboard: Res<ButtonInput<KeyCode>>,
    mut visible: ResMut<TextInputDemoVisible>,
) {
    if keyboard.just_pressed(DEMO_TOGGLE_KEY) {
        visible.0 = !visible.0;
    }
}

/// Drive the demo panel's [`UiPanelShown`] from [`TextInputDemoVisible`] whenever
/// it changes, leaving the scaffold's `apply_panel_visibility` to do the hiding.
pub(crate) fn apply_text_input_demo_visibility(
    visible: Res<TextInputDemoVisible>,
    mut panels: Query<&mut UiPanelShown, With<TextInputDemoRoot>>,
) {
    if !visible.is_changed() {
        return;
    }
    for mut shown in &mut panels {
        if shown.0 != visible.0 {
            shown.0 = visible.0;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        NumericField, TextInputKind, TextInputSpec, TextInputValue, accepts_float_intermediate,
        accepts_integer_intermediate, accepts_unsigned_integer, reconcile_numeric_field,
    };
    use bevy::text::{EditableText, FontCx, LayoutCx};
    use pretty_assertions::assert_eq;

    /// A boxed error so a test can use `?` instead of the disallowed
    /// `unwrap` / `expect`.
    type TestError = Box<dyn core::error::Error>;

    /// The float validator accepts every intermediate state a decimal is typed
    /// through and rejects the structurally impossible ones — the arrangement
    /// checks the per-character filter cannot make.
    #[test]
    fn float_validator_accepts_intermediates_and_rejects_bad_arrangements() {
        for good in [
            "", "-", ".", "-.", "1.", "-.5", "0", "42", "-3.5", "0.25", "-0",
        ] {
            assert!(
                accepts_float_intermediate(good),
                "float should accept intermediate {good:?}"
            );
        }
        for bad in [
            "1.2.3", "1-2", "5-", "--5", "1e5", "1,5", "abc", ". .", "-1.2.3",
        ] {
            assert!(
                !accepts_float_intermediate(bad),
                "float should reject {bad:?}"
            );
        }
    }

    /// The signed-integer validator: an optional leading sign then digits, and
    /// nothing structurally impossible.
    #[test]
    fn integer_validator_accepts_intermediates_and_rejects_bad_arrangements() {
        for good in ["", "-", "0", "5", "-5", "128", "-0"] {
            assert!(
                accepts_integer_intermediate(good),
                "integer should accept {good:?}"
            );
        }
        for bad in ["1.5", "1-2", "5-", "--5", "1e5", "abc", "+5"] {
            assert!(
                !accepts_integer_intermediate(bad),
                "integer should reject {bad:?}"
            );
        }
    }

    /// The non-negative-integer validator: digits only, so a sign is rejected
    /// outright (the `-` key never applies).
    #[test]
    fn unsigned_validator_accepts_digits_only() {
        for good in ["", "0", "5", "128", "00"] {
            assert!(
                accepts_unsigned_integer(good),
                "unsigned should accept {good:?}"
            );
        }
        for bad in ["-5", "-", "1.5", "1e5", "abc", "+5"] {
            assert!(
                !accepts_unsigned_integer(bad),
                "unsigned should reject {bad:?}"
            );
        }
    }

    /// The per-character filters admit exactly their kind's character set — the
    /// cheap half of validation, checked here so a change to a set is caught.
    #[test]
    fn char_filters_admit_the_right_character_set() -> Result<(), TestError> {
        let float = TextInputKind::Float
            .char_filter()
            .ok_or("float should have a filter")?;
        assert!(float('3') && float('-') && float('.'));
        assert!(!float('e') && !float('+') && !float(' '));

        let integer = TextInputKind::Integer
            .char_filter()
            .ok_or("integer should have a filter")?;
        assert!(integer('3') && integer('-'));
        assert!(!integer('.') && !integer('e'));

        let unsigned = TextInputKind::NonNegativeInteger
            .char_filter()
            .ok_or("unsigned should have a filter")?;
        assert!(unsigned('3'));
        assert!(!unsigned('-') && !unsigned('.'));
        Ok(())
    }

    /// The free-text kinds install no filter and accept any string — they are
    /// plain text, not numbers.
    #[test]
    fn free_text_kinds_have_no_filter_and_accept_anything() {
        assert!(TextInputKind::Line.char_filter().is_none());
        assert!(TextInputKind::Multiline.char_filter().is_none());
        assert!(TextInputKind::Line.accepts("anything at all: 1.2.3 -- é 世界"));
        assert!(TextInputKind::Multiline.accepts("two\nlines"));
    }

    /// `parse` reads a committed value out, and returns `None` for the incomplete
    /// intermediate states `accepts` admits — the distinction between "can still be
    /// typed" and "is a number now".
    #[test]
    fn parse_reads_committed_values_and_rejects_incompletes() {
        assert_eq!(
            TextInputKind::Float.parse("-3.5"),
            Some(TextInputValue::Float(-3.5))
        );
        assert_eq!(
            TextInputKind::Integer.parse("-42"),
            Some(TextInputValue::Integer(-42))
        );
        assert_eq!(
            TextInputKind::NonNegativeInteger.parse("128"),
            Some(TextInputValue::Unsigned(128))
        );
        // Intermediate states are accepted while typing but are not yet values.
        assert_eq!(TextInputKind::Float.parse("-"), None);
        assert_eq!(TextInputKind::Float.parse(""), None);
        assert_eq!(TextInputKind::Integer.parse("-"), None);
        // A non-negative field never yields a negative value.
        assert_eq!(TextInputKind::NonNegativeInteger.parse("-5"), None);
        // Free text has no numeric value.
        assert_eq!(TextInputKind::Line.parse("123"), None);
    }

    /// The runtime reconcile drives the whole numeric enforcement path — the real
    /// parley editor, the `set_text` revert, the caret move — so a bad edit is
    /// undone and a good one remembered, exactly as the live system does it. This
    /// is the one path `crate::ui_test`'s headless harness does not run (it does
    /// not add [`super::TextInputPlugin`]), so it is exercised here.
    #[test]
    fn reconcile_reverts_a_bad_edit_and_remembers_a_good_one() -> Result<(), TestError> {
        let mut font_cx = FontCx::default();
        let mut layout_cx = LayoutCx::default();
        let mut editable = EditableText::new("1.5");
        let mut field = NumericField {
            kind: TextInputKind::Float,
            last_valid: "1.5".to_owned(),
        };

        // Type a second decimal point at the end → "1.5.", structurally invalid.
        {
            let mut driver = editable.editor.driver(&mut font_cx, &mut layout_cx);
            driver.refresh_layout();
            driver.move_to_text_end();
            driver.insert_or_replace_selection(".");
        }
        assert_eq!(editable.value().to_string(), "1.5.");
        reconcile_numeric_field(&mut editable, &mut field, &mut font_cx, &mut layout_cx);
        assert_eq!(
            editable.value().to_string(),
            "1.5",
            "a second decimal point should revert to the last valid value"
        );
        assert_eq!(field.last_valid, "1.5");

        // A valid edit is kept and becomes the new fallback.
        {
            let mut driver = editable.editor.driver(&mut font_cx, &mut layout_cx);
            driver.refresh_layout();
            driver.move_to_text_end();
            driver.insert_or_replace_selection("2");
        }
        assert_eq!(editable.value().to_string(), "1.52");
        reconcile_numeric_field(&mut editable, &mut field, &mut font_cx, &mut layout_cx);
        assert_eq!(editable.value().to_string(), "1.52");
        assert_eq!(
            field.last_valid, "1.52",
            "a valid edit should become the new fallback"
        );
        Ok(())
    }

    /// A numeric spec with an invalid initial value sanitises to an empty field
    /// rather than seeding a `last_valid` its own validator would reject.
    #[test]
    fn an_invalid_initial_value_sanitises_to_empty() {
        let spec = TextInputSpec {
            initial: "1.2.3".to_owned(),
            ..TextInputSpec::new("t", TextInputKind::Float)
        };
        assert_eq!(spec.sanitised_initial(), "");
        // A valid initial is kept.
        let ok = TextInputSpec {
            initial: "-3.5".to_owned(),
            ..TextInputSpec::new("t", TextInputKind::Float)
        };
        assert_eq!(ok.sanitised_initial(), "-3.5");
    }
}
