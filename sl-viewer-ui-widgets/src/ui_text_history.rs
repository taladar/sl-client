//! **Undo and redo** for a text field (`viewer-lsl-editor-widget`).
//!
//! `bevy_text`'s [`EditableText`] is `parley::PlainEditor`, which has no
//! history at all, and `bevy_ui_widgets` binds no `Ctrl+Z`. This module is the
//! missing half: an [`EditHistory`] on a field records what each frame's edits
//! did to its buffer, and `Ctrl+Z` / `Ctrl+Y` (or `Ctrl+Shift+Z`) walk it.
//!
//! # Recording by comparing, not by intercepting
//!
//! A buffer changes through many doors — a keystroke, a paste, a cut, an IME
//! commit, the overwrite mode's rewritten queue, a consumer's own replace — and
//! a history that hooked each door would miss the next one somebody adds. So
//! this records the **effect** instead: after the frame's edits have applied, it
//! compares the buffer with the one it saw last and keeps the difference as one
//! step — the byte offset, the text that went and the text that came, and the
//! selection either side of it. Whatever changed the buffer, the step undoes it.
//!
//! The comparison is a common prefix and suffix, linear in the buffer, which a
//! 64 KB script does in microseconds — two orders of magnitude under the
//! relayout the same keystroke pays for.
//!
//! # Typing is one step, not one per letter
//!
//! A run of typed characters at one place, with no pause longer than
//! [`COALESCE_WINDOW_SECS`] and no line break, folds into a single step, and so
//! does a run of backspaces or deletes — the grouping every editor has, and the
//! reference's (`LLTextEditor`'s `LLTextCmdInsert::canExtend`). A paste, a cut,
//! a replace, anything that is not one character, is a step of its own.
//!
//! # Read-only fields
//!
//! A [`ReadOnlyField`] keeps its history but refuses to walk it: an undo is an
//! edit like any other, and a field that may not be changed may not be changed
//! back either.
//!
//! Reference (Firestorm, read-only): `lltexteditor` (`undo`, `redo`, the
//! `LLTextCmd` family).

use core::ops::Range;

use bevy::input::keyboard::{Key, KeyboardInput};
use bevy::input_focus::FocusedInput;
use bevy::prelude::*;
use bevy::text::{EditableText, EditableTextSystems, FontCx, LayoutCx};
use bevy::ui::UiSystems;

use crate::ui_text_input::ReadOnlyField;

/// The longest pause, in seconds, inside a run of typing that still folds into
/// the step before it. A pause longer than this starts a new undo step, so an
/// undo takes back what was typed since the resident last stopped to think.
pub const COALESCE_WINDOW_SECS: f64 = 1.0;

/// The most steps a field remembers. The oldest are dropped past it: a long
/// session of typing must not grow without bound, and nobody undoes a thousand
/// steps back.
const MAX_STEPS: usize = 1000;

/// A field's selection as two byte offsets: where it started (`anchor`) and
/// where the caret is (`focus`). Equal for a plain caret.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct HistorySelection {
    /// Where the selection was started.
    pub anchor: usize,
    /// Where the caret is.
    pub focus: usize,
}

/// What kind of edit a step is, which decides whether the next one folds into
/// it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StepKind {
    /// One character typed after the last.
    Typing,
    /// One character deleted beside the last deleted one.
    Deleting,
    /// Anything else: a paste, a cut, a line break, a replace.
    Other,
}

/// One undoable change: at byte `at`, `removed` gave way to `inserted`.
#[derive(Debug, Clone, PartialEq, Eq)]
struct HistoryStep {
    /// Where the change starts.
    at: usize,
    /// The text that was there before.
    removed: String,
    /// The text that is there after.
    inserted: String,
    /// The selection before the change, which an undo restores.
    before: HistorySelection,
    /// The selection after the change, which a redo restores.
    after: HistorySelection,
    /// What kind of edit this is, for folding the next one in.
    kind: StepKind,
}

/// A walk through a field's history the resident asked for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HistoryRequest {
    /// Take back the last step.
    Undo,
    /// Put back the last step taken back.
    Redo,
}

/// A buffer change a history walk wants made: replace `range` with `text`, then
/// select `selection`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HistoryEdit {
    /// The byte range of the current buffer to replace.
    pub range: Range<usize>,
    /// What goes there.
    pub text: String,
    /// The selection afterwards.
    pub selection: HistorySelection,
}

/// A text field's **undo history** — put it on any [`EditableText`] to give
/// that field `Ctrl+Z` and `Ctrl+Y`.
///
/// The history is a projection of the buffer, not a log of input: see the
/// [module documentation](self).
#[derive(Component, Debug, Default)]
pub struct EditHistory {
    /// The steps an undo walks back, newest last.
    undo: Vec<HistoryStep>,
    /// The steps a redo walks forward, newest last.
    redo: Vec<HistoryStep>,
    /// The buffer as last seen, or `None` before the first look — the first
    /// buffer a field shows is its starting point, not a change.
    seen: Option<String>,
    /// The selection as last seen.
    seen_selection: HistorySelection,
    /// When the last step was recorded or extended, in seconds.
    last_change_secs: f64,
    /// A walk asked for and not yet made.
    request: Option<HistoryRequest>,
}

impl EditHistory {
    /// Whether there is a step to undo.
    #[must_use]
    pub const fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    /// Whether there is a step to redo.
    #[must_use]
    pub const fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    /// Ask for a walk, made after this frame's edits apply.
    pub const fn request(&mut self, request: HistoryRequest) {
        self.request = Some(request);
    }

    /// Forget every step and take `text` as the new starting point — for a
    /// consumer that replaces the whole buffer with something the resident did
    /// not type (a reload from the grid), which no undo should walk into.
    pub fn reset(&mut self, text: &str) {
        self.undo.clear();
        self.redo.clear();
        self.seen = Some(text.to_owned());
    }

    /// Look at the buffer after a frame's edits: record the difference from
    /// the last look as a step (or fold it into the last step), and remember
    /// the selection. `now` is in seconds.
    pub fn observe(&mut self, text: &str, selection: HistorySelection, now: f64) {
        let Some(seen) = self.seen.as_deref() else {
            self.seen = Some(text.to_owned());
            self.seen_selection = selection;
            return;
        };
        if seen == text {
            self.seen_selection = selection;
            return;
        }
        let (at, removed, inserted) = difference(seen, text);
        let step = HistoryStep {
            at,
            kind: step_kind(&removed, &inserted),
            removed,
            inserted,
            before: self.seen_selection,
            after: selection,
        };
        let recent = now - self.last_change_secs <= COALESCE_WINDOW_SECS;
        let folded = recent
            && self
                .undo
                .last_mut()
                .is_some_and(|last| fold_into(last, &step));
        if !folded {
            self.undo.push(step);
            if self.undo.len() > MAX_STEPS {
                let _oldest = self.undo.remove(0);
            }
        }
        self.redo.clear();
        self.last_change_secs = now;
        self.seen = Some(text.to_owned());
        self.seen_selection = selection;
    }

    /// Take the pending request and turn it into the edit it makes, moving the
    /// step between the two stacks. `None` when nothing was asked for or there
    /// is nothing to walk.
    ///
    /// The caller makes the edit; the next [`observe`](Self::observe) then sees
    /// a buffer that already matches, because this records it as seen.
    pub fn take_edit(&mut self) -> Option<HistoryEdit> {
        let request = self.request.take()?;
        let (edit, step) = match request {
            HistoryRequest::Undo => {
                let step = self.undo.pop()?;
                let edit = HistoryEdit {
                    range: step.at..step.at.saturating_add(step.inserted.len()),
                    text: step.removed.clone(),
                    selection: step.before,
                };
                (edit, step)
            }
            HistoryRequest::Redo => {
                let step = self.redo.pop()?;
                let edit = HistoryEdit {
                    range: step.at..step.at.saturating_add(step.removed.len()),
                    text: step.inserted.clone(),
                    selection: step.after,
                };
                (edit, step)
            }
        };
        if let Some(seen) = self.seen.as_mut()
            && seen.is_char_boundary(edit.range.start)
            && seen.is_char_boundary(edit.range.end.min(seen.len()))
        {
            seen.replace_range(edit.range.start..edit.range.end.min(seen.len()), &edit.text);
        }
        self.seen_selection = edit.selection;
        // A walk is never folded into by the typing after it.
        self.last_change_secs = f64::NEG_INFINITY;
        match request {
            HistoryRequest::Undo => self.redo.push(step),
            HistoryRequest::Redo => self.undo.push(step),
        }
        Some(edit)
    }
}

/// Where `old` and `new` differ: the offset, what `old` had there and what
/// `new` has. Both ends are pulled back onto character boundaries.
fn difference(old: &str, new: &str) -> (usize, String, String) {
    let mut prefix = old
        .bytes()
        .zip(new.bytes())
        .take_while(|(a, b)| a == b)
        .count();
    while !(old.is_char_boundary(prefix) && new.is_char_boundary(prefix)) {
        prefix = prefix.saturating_sub(1);
    }
    let room = old.len().min(new.len()).saturating_sub(prefix);
    let mut suffix = old
        .bytes()
        .rev()
        .zip(new.bytes().rev())
        .take(room)
        .take_while(|(a, b)| a == b)
        .count();
    while !(old.is_char_boundary(old.len().saturating_sub(suffix))
        && new.is_char_boundary(new.len().saturating_sub(suffix)))
    {
        suffix = suffix.saturating_sub(1);
    }
    let removed = old
        .get(prefix..old.len().saturating_sub(suffix))
        .unwrap_or_default();
    let inserted = new
        .get(prefix..new.len().saturating_sub(suffix))
        .unwrap_or_default();
    (prefix, removed.to_owned(), inserted.to_owned())
}

/// What kind of edit replacing `removed` with `inserted` is.
fn step_kind(removed: &str, inserted: &str) -> StepKind {
    let single = |text: &str| text.chars().count() == 1;
    if removed.is_empty() && single(inserted) && inserted != "\n" {
        StepKind::Typing
    } else if inserted.is_empty() && single(removed) && removed != "\n" {
        StepKind::Deleting
    } else {
        StepKind::Other
    }
}

/// Fold `next` into `last` if it continues it — a letter typed right after the
/// last one, or a character deleted right beside the last deleted one — and
/// say whether it did.
fn fold_into(last: &mut HistoryStep, next: &HistoryStep) -> bool {
    match (last.kind, next.kind) {
        (StepKind::Typing, StepKind::Typing)
            if next.at == last.at.saturating_add(last.inserted.len()) =>
        {
            last.inserted.push_str(&next.inserted);
        }
        // A backspace: the next deletion ends where the last one started.
        (StepKind::Deleting, StepKind::Deleting)
            if next.at.saturating_add(next.removed.len()) == last.at =>
        {
            last.removed.insert_str(0, &next.removed);
            last.at = next.at;
        }
        // A forward delete: the next deletion starts where the last one did.
        (StepKind::Deleting, StepKind::Deleting) if next.at == last.at => {
            last.removed.push_str(&next.removed);
        }
        _ => return false,
    }
    last.after = next.after;
    true
}

/// A field's selection as the history keeps it.
fn selection_of(editable: &EditableText) -> HistorySelection {
    let selection = editable.editor.raw_selection();
    HistorySelection {
        anchor: selection.anchor().index(),
        focus: selection.focus().index(),
    }
}

/// Record each changed field's buffer into its history, after the frame's
/// edits have applied.
///
/// A composing IME is skipped: its preedit sits in the buffer without being
/// part of the text, and the commit that ends it is the change worth keeping.
fn record_edit_history(
    time: Res<Time>,
    mut fields: Query<(&EditableText, &mut EditHistory), Changed<EditableText>>,
) {
    let now = time.elapsed_secs_f64();
    for (editable, mut history) in &mut fields {
        if editable.is_composing() {
            continue;
        }
        history.observe(editable.editor.raw_text(), selection_of(editable), now);
    }
}

/// Make each field's pending undo or redo, after the frame's edits and before
/// they are recorded — so the walk itself is never recorded as a new step.
fn apply_history_requests(
    mut fields: Query<(&mut EditableText, &mut EditHistory, Has<ReadOnlyField>)>,
    mut font_cx: ResMut<FontCx>,
    mut layout_cx: ResMut<LayoutCx>,
) {
    for (mut editable, mut history, read_only) in &mut fields {
        if history.request.is_none() {
            continue;
        }
        if read_only || editable.is_composing() {
            history.request = None;
            continue;
        }
        let Some(edit) = history.take_edit() else {
            continue;
        };
        apply_history_edit(&mut editable, &edit, &mut font_cx, &mut layout_cx);
    }
}

/// Replace `edit.range` with `edit.text` in `editable` and select
/// `edit.selection` — the buffer operation an undo, a redo and a find/replace
/// share. Out-of-range or mid-character offsets are refused rather than
/// panicking parley.
pub fn apply_history_edit(
    editable: &mut EditableText,
    edit: &HistoryEdit,
    font_cx: &mut FontCx,
    layout_cx: &mut LayoutCx,
) {
    let text = editable.editor.raw_text();
    let valid = |index: usize| index <= text.len() && text.is_char_boundary(index);
    if !(valid(edit.range.start) && valid(edit.range.end) && edit.range.start <= edit.range.end) {
        warn!(range = ?edit.range, "an edit no longer fits its field's buffer; dropped");
        return;
    }
    let mut driver = editable.editor.driver(font_cx, layout_cx);
    driver.select_byte_range(edit.range.start, edit.range.end);
    if edit.text.is_empty() {
        driver.delete_selection();
    } else {
        driver.insert_or_replace_selection(&edit.text);
    }
    driver.select_byte_range(edit.selection.anchor, edit.selection.focus);
}

/// Turn `Ctrl+Z` into an undo and `Ctrl+Y` / `Ctrl+Shift+Z` into a redo on the
/// focused field, when it keeps a history.
///
/// The key is read as the **logical** character, as `bevy_ui_widgets` reads
/// `Ctrl+A`: on a QWERTZ keyboard the key labelled Z is the one that undoes.
fn on_history_key(
    event: On<FocusedInput<KeyboardInput>>,
    keys: Res<ButtonInput<KeyCode>>,
    mut histories: Query<&mut EditHistory>,
) {
    if !event.input.state.is_pressed() {
        return;
    }
    let Ok(mut history) = histories.get_mut(event.focused_entity) else {
        return;
    };
    let held = ChordModifiers::read(&keys);
    if !held.ctrl || held.alt {
        return;
    }
    let Key::Character(character) = &event.input.logical_key else {
        return;
    };
    let shift = held.shift;
    if character.eq_ignore_ascii_case("z") {
        history.request(if shift {
            HistoryRequest::Redo
        } else {
            HistoryRequest::Undo
        });
    } else if character.eq_ignore_ascii_case("y") && !shift {
        history.request(HistoryRequest::Redo);
    }
}

/// The modifiers held with a chord, read from the **physical** keys as the
/// crate's other widgets read them — either side's key counts, and `Super`
/// stands in for `Ctrl` as `Cmd` does on a Mac keyboard.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ChordModifiers {
    /// Either `Ctrl` or either `Super`.
    pub(crate) ctrl: bool,
    /// Either `Shift`.
    pub(crate) shift: bool,
    /// Either `Alt`.
    pub(crate) alt: bool,
}

impl ChordModifiers {
    /// The modifiers `keys` holds now.
    pub(crate) fn read(keys: &ButtonInput<KeyCode>) -> Self {
        Self {
            ctrl: keys.any_pressed([
                KeyCode::ControlLeft,
                KeyCode::ControlRight,
                KeyCode::SuperLeft,
                KeyCode::SuperRight,
            ]),
            shift: keys.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]),
            alt: keys.any_pressed([KeyCode::AltLeft, KeyCode::AltRight]),
        }
    }
}

/// The plugin for field histories: the recorder, the walker and the chord.
#[derive(Debug, Clone, Copy, Default)]
pub struct TextHistoryPlugin;

impl Plugin for TextHistoryPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            PostUpdate,
            (apply_history_requests, record_edit_history)
                .chain()
                // After the frame's edits, so what is recorded is what they
                // did; before the layout, so an undo is drawn this frame.
                .after(EditableTextSystems)
                .before(UiSystems::PostLayout),
        )
        .add_observer(on_history_key);
    }
}

#[cfg(test)]
mod tests {
    use super::{EditHistory, HistoryEdit, HistoryRequest, HistorySelection, difference};
    use pretty_assertions::assert_eq;

    /// A caret at `at`.
    const fn caret(at: usize) -> HistorySelection {
        HistorySelection {
            anchor: at,
            focus: at,
        }
    }

    /// Replay `edit` on `text`, as the field would.
    fn replay(text: &str, edit: &HistoryEdit) -> String {
        let mut out = text.to_owned();
        out.replace_range(edit.range.clone(), &edit.text);
        out
    }

    /// The difference is the narrowest change, on character boundaries.
    #[test]
    fn the_difference_is_the_narrowest_change() {
        assert_eq!(
            difference("hello", "help"),
            (3, "lo".to_owned(), "p".to_owned())
        );
        assert_eq!(difference("aa", "aaa"), (2, String::new(), "a".to_owned()));
        // `é` and `è` share their first byte: the change is the whole character.
        assert_eq!(difference("é", "è"), (0, "é".to_owned(), "è".to_owned()));
    }

    /// A run of typing is one step; a pause or a line break starts another.
    #[test]
    fn typing_folds_until_a_pause_or_a_line_break() -> Result<(), Box<dyn core::error::Error>> {
        let mut history = EditHistory::default();
        history.observe("", caret(0), 0.0);
        history.observe("a", caret(1), 0.1);
        history.observe("ab", caret(2), 0.2);
        history.observe("ab\n", caret(3), 0.3);
        history.observe("ab\nc", caret(4), 0.4);
        history.observe("ab\ncd", caret(5), 2.0);

        let mut text = "ab\ncd".to_owned();
        let mut undone = Vec::new();
        while history.can_undo() {
            history.request(HistoryRequest::Undo);
            let edit = history
                .take_edit()
                .ok_or("an undo with steps left made no edit")?;
            text = replay(&text, &edit);
            undone.push(text.clone());
        }
        assert_eq!(undone, vec!["ab\nc", "ab\n", "ab", ""]);
        Ok(())
    }

    /// Undo puts the selection back where it was before the step, redo where
    /// it was after, and a new edit after an undo drops the redo.
    #[test]
    fn undo_and_redo_restore_text_and_selection() -> Result<(), Box<dyn core::error::Error>> {
        let mut history = EditHistory::default();
        let selected = HistorySelection {
            anchor: 0,
            focus: 5,
        };
        history.observe("hello world", selected, 0.0);
        // The selection is replaced by a paste.
        history.observe("bye world", caret(3), 5.0);

        history.request(HistoryRequest::Undo);
        let undo = history.take_edit().ok_or("no undo")?;
        assert_eq!(replay("bye world", &undo), "hello world");
        assert_eq!(undo.selection, selected);

        history.request(HistoryRequest::Redo);
        let redo = history.take_edit().ok_or("no redo")?;
        assert_eq!(replay("hello world", &redo), "bye world");
        assert_eq!(redo.selection, caret(3));

        history.request(HistoryRequest::Undo);
        let _again = history.take_edit().ok_or("no second undo")?;
        history.observe("hello world!", caret(12), 9.0);
        assert!(!history.can_redo(), "a new edit must drop the redo stack");
        Ok(())
    }

    /// Backspacing and forward-deleting fold, each in its own direction.
    #[test]
    fn deletions_fold_in_their_direction() -> Result<(), Box<dyn core::error::Error>> {
        let mut history = EditHistory::default();
        history.observe("abcdef", caret(4), 0.0);
        history.observe("abcef", caret(3), 0.1);
        history.observe("abef", caret(2), 0.2);
        history.observe("abf", caret(2), 0.3);
        history.request(HistoryRequest::Undo);
        let edit = history.take_edit().ok_or("no undo")?;
        assert_eq!(replay("abf", &edit), "abcdef");
        assert!(
            !history.can_undo(),
            "three deletions around one caret are one step"
        );
        Ok(())
    }

    /// The walk is recorded as seen, so the next look finds nothing new.
    #[test]
    fn a_walk_is_not_recorded_again() -> Result<(), Box<dyn core::error::Error>> {
        let mut history = EditHistory::default();
        history.observe("", caret(0), 0.0);
        history.observe("x", caret(1), 0.1);
        history.request(HistoryRequest::Undo);
        let edit = history.take_edit().ok_or("no undo")?;
        let text = replay("x", &edit);
        history.observe(&text, caret(0), 0.2);
        assert!(!history.can_undo());
        assert!(
            history.can_redo(),
            "observing the undone buffer dropped the redo"
        );
        Ok(())
    }
}
