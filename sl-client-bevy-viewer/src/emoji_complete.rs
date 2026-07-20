//! The **inline `:`-emoji completer** (`viewer-emoji-colon-autocomplete`): type
//! `:smi` in a text field and a small popup offers `:smile:` / `:smiley:` / … to
//! filter as you type and drop the Unicode glyph in.
//!
//! # A field opt-in, on the same edit path
//!
//! The completer is attached to a single-line [`EditableText`] (the chat input
//! ([`crate::chat_input`]) is its first consumer, and it is reusable on any field)
//! together with an **anchor** node it hangs its popup under — so the popup needs
//! no screen-space maths: it is an absolute child of the field's own container,
//! sitting just above it. Short-codes and glyphs come from [`sl_emoji`].
//!
//! # The trailing-token model
//!
//! A chat line is typed left to right, so the completer looks only at the
//! **trailing** `:`-token of the value ([`trailing_colon_prefix`]): the run of
//! short-code characters at the end, back to a `:` that itself sits at the start
//! of the string or after whitespace. That keeps the detection a pure function of
//! the string (no caret-offset bookkeeping), and means an already-closed
//! `:smile:` does not re-trigger. Accepting a match replaces that token with the
//! glyph ([`replace_trailing_token`]) and leaves the caret at the end.
//!
//! # Keys
//!
//! While the popup is open: `Up`/`Down` move the selection, `Enter`/`Tab` accept
//! it, `Escape` closes it. The consumed keys are cleared from the frame's input
//! ([`ButtonInput::clear_just_pressed`]) so the chat input's own `Enter`-to-send
//! ([`crate::chat_input`]) — ordered after [`ColonCompleteSet`] — does not also
//! fire on the same press.
//!
//! Reference (Firestorm, read-only): `llemojihelper`, `llpanelemojicomplete`.

use bevy::input_focus::InputFocus;
use bevy::prelude::*;
use bevy::text::{EditableText, FontCx, LayoutCx};
use sl_emoji::{ShortcodeMatch, complete};

use crate::ui::column;
use crate::ui_font::UiFont;

/// The fewest short-code characters after the `:` before the popup opens — so a
/// lone `:` or `:)` does not pop a list up.
const MIN_PREFIX: usize = 2;

/// The most matches the popup shows at once.
const MAX_MATCHES: usize = 8;

/// One popup row's height, in logical pixels.
const ROW_HEIGHT: f32 = 20.0;

/// The popup / row font size, in logical pixels.
const FONT_SIZE: f32 = 14.0;

/// The popup's background — a dark panel it reads as, over the field.
const POPUP_BACKGROUND: Color = Color::srgba(0.10, 0.12, 0.16, 0.97);

/// The popup's border.
const POPUP_BORDER: Color = Color::srgb(0.34, 0.40, 0.52);

/// A row's resting background.
const ROW_BACKGROUND: Color = Color::NONE;

/// The selected row's background — the one `Enter` would accept.
const ROW_SELECTED_BACKGROUND: Color = Color::srgb(0.22, 0.40, 0.60);

/// A row's text colour.
const ROW_TEXT_COLOR: Color = Color::srgb(0.90, 0.92, 0.96);

/// The system set the completer's key handling runs in, so a consumer's own
/// `Enter` handling (the chat input's send) can order **after** it and not fire on
/// a press the completer already consumed.
#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct ColonCompleteSet;

/// The plugin that drives every attached completer: recompute-and-reflect, key
/// handling, and the row highlight. Each system is a no-op where there is no
/// completer, so adding it is always safe.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct ColonCompletePlugin;

impl Plugin for ColonCompletePlugin {
    /// Register the completer systems, with the key handling in [`ColonCompleteSet`].
    fn build(&self, app: &mut App) {
        app.add_systems(
            Update,
            (
                drive_colon_complete,
                handle_colon_complete_keys.in_set(ColonCompleteSet),
                highlight_colon_rows,
            )
                .chain(),
        );
    }
}

/// The completer state carried on a field: the popup it drives, its pooled rows,
/// whether it is open, which match is selected, the current matches, and where the
/// trailing `:` sits (so accepting replaces from there).
#[derive(Component, Debug, Clone)]
pub(crate) struct ColonComplete {
    /// The popup container node (an absolute child of the field's anchor).
    popup: Entity,
    /// The pooled row entities, [`MAX_MATCHES`] of them, shown / hidden per match
    /// count.
    rows: Vec<Entity>,
    /// Whether the popup is currently open (a real match list is showing).
    open: bool,
    /// The selected match, as an index into [`matches`](Self::matches).
    selected: usize,
    /// The current matches for the trailing token, at most [`MAX_MATCHES`].
    matches: Vec<ShortcodeMatch>,
    /// The byte offset of the trailing `:` the matches complete, so accepting a
    /// match replaces the token from there.
    colon_byte: usize,
}

/// One popup row: its index within the current match list (so its click observer
/// and the reflect / highlight systems address the right match) and its label
/// node. The field it belongs to is captured by the row's click observer.
#[derive(Component, Debug, Clone, Copy)]
pub(crate) struct ColonRow {
    /// This row's index into the completer's current matches.
    index: usize,
    /// The row's glyph + short-code text node.
    label: Entity,
}

// ---------------------------------------------------------------------------
// Pure core — the trailing-token detection and replacement, unit-tested.
// ---------------------------------------------------------------------------

/// Whether `c` may appear in a gemoji short-code — ASCII alphanumerics plus the
/// `_`, `+`, `-` the codes use (`+1`, `e-mail`).
const fn is_shortcode_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, '_' | '+' | '-')
}

/// The trailing `:`-token of `value`, or `None` when there is not one at the end.
///
/// Returns the **byte offset of the `:`** and the short-code prefix after it (in
/// order). The `:` must sit at the string start or right after whitespace, and
/// everything after it to the end must be short-code characters — so `":ro"` and
/// `"hi :ro"` yield `("ro")`, while `"a:ro"` (`:` mid-word), a non-code char after
/// the `:`, and an already-closed `":rocket:"` (trailing `:`) yield `None`.
fn trailing_colon_prefix(value: &str) -> Option<(usize, String)> {
    let mut rev_prefix: Vec<char> = Vec::new();
    let mut iter = value.char_indices().rev().peekable();
    while let Some((byte, ch)) = iter.next() {
        if ch == ':' {
            // The `:` must start a token: at the string start, or after whitespace.
            let before_ok = iter.peek().is_none_or(|(_, prev)| prev.is_whitespace());
            if !before_ok {
                return None;
            }
            let prefix: String = rev_prefix.iter().rev().collect();
            return Some((byte, prefix));
        }
        if is_shortcode_char(ch) {
            rev_prefix.push(ch);
        } else {
            // A non-code, non-colon char ends the trailing run without a token.
            return None;
        }
    }
    None
}

/// `value` with the trailing token starting at `colon_byte` replaced by `glyph`
/// (everything from the `:` to the end becomes the glyph), avoiding a byte slice
/// (the workspace forbids indexing / slicing) by rebuilding the head char by char.
fn replace_trailing_token(value: &str, colon_byte: usize, glyph: &str) -> String {
    let mut result: String = value
        .char_indices()
        .take_while(|(byte, _)| *byte < colon_byte)
        .map(|(_, ch)| ch)
        .collect();
    result.push_str(glyph);
    result
}

/// The matches to offer for `value`'s trailing token, and the byte the token
/// starts at — or `None` when there is no token, it is too short, or nothing
/// matches. The pure decision the popup reflects.
fn matches_for(value: &str) -> Option<(usize, Vec<ShortcodeMatch>)> {
    let (colon_byte, prefix) = trailing_colon_prefix(value)?;
    if prefix.chars().count() < MIN_PREFIX {
        return None;
    }
    let mut matches = complete(&prefix);
    if matches.is_empty() {
        return None;
    }
    matches.truncate(MAX_MATCHES);
    Some((colon_byte, matches))
}

// ---------------------------------------------------------------------------
// Attaching
// ---------------------------------------------------------------------------

/// Attach a colon-completer to `field`, hanging its popup under `anchor` (the
/// field's own container), and return the popup entity.
///
/// The popup is an **absolute** child of `anchor`, sitting just above it
/// (`bottom: 100%`) as the reference completer sits above the chat bar, so it
/// needs no screen-space placement. It starts hidden; [`drive_colon_complete`]
/// shows and fills it.
pub(crate) fn attach_colon_complete(
    commands: &mut Commands,
    field: Entity,
    anchor: Entity,
) -> Entity {
    let popup = commands
        .spawn((
            Node {
                display: Display::None,
                position_type: PositionType::Absolute,
                bottom: Val::Percent(100.0),
                left: Val::Px(0.0),
                min_width: Val::Px(140.0),
                border: UiRect::all(Val::Px(1.0)),
                padding: UiRect::all(Val::Px(2.0)),
                ..column(Val::Px(0.0))
            },
            BorderColor::all(POPUP_BORDER),
            BackgroundColor(POPUP_BACKGROUND),
            GlobalZIndex(10_000),
            Pickable {
                should_block_lower: true,
                is_hoverable: true,
            },
            Name::new("colon-complete-popup"),
            ChildOf(anchor),
        ))
        .id();

    let rows: Vec<Entity> = (0..MAX_MATCHES)
        .map(|index| spawn_colon_row(commands, popup, field, index))
        .collect();

    commands.entity(field).insert(ColonComplete {
        popup,
        rows,
        open: false,
        selected: 0,
        matches: Vec::new(),
        colon_byte: 0,
    });
    popup
}

/// Spawn one pooled popup row under `popup` for `field`'s completer at match
/// `index`: a clickable line with a glyph + short-code label. Clicking it accepts
/// that match.
fn spawn_colon_row(commands: &mut Commands, popup: Entity, field: Entity, index: usize) -> Entity {
    let label = commands
        .spawn((
            Text::new(""),
            UiFont::Sans.at(FONT_SIZE),
            TextColor(ROW_TEXT_COLOR),
            Pickable::IGNORE,
        ))
        .id();
    let row = commands
        .spawn((
            Node {
                display: Display::None,
                height: Val::Px(ROW_HEIGHT),
                align_items: AlignItems::Center,
                padding: UiRect::horizontal(Val::Px(4.0)),
                ..default()
            },
            BackgroundColor(ROW_BACKGROUND),
            Pickable::default(),
            ColonRow { index, label },
            ChildOf(popup),
        ))
        .add_child(label)
        .id();
    commands.entity(row).observe(
        move |mut press: On<Pointer<Press>>,
              mut completers: Query<&mut ColonComplete>,
              mut fields: Query<&mut EditableText>,
              mut font_cx: ResMut<FontCx>,
              mut layout_cx: ResMut<LayoutCx>| {
            press.propagate(false);
            if press.button != PointerButton::Primary {
                return;
            }
            accept_match(
                field,
                index,
                &mut completers,
                &mut fields,
                &mut font_cx,
                &mut layout_cx,
            );
        },
    );
    row
}

// ---------------------------------------------------------------------------
// Systems
// ---------------------------------------------------------------------------

/// Recompute each **focused** field's completer from its value and reflect it:
/// open the popup with the current matches, or close it. A field that is not
/// focused is closed.
fn drive_colon_complete(
    focus: Res<InputFocus>,
    mut completers: Query<(Entity, &mut ColonComplete, &EditableText)>,
    mut nodes: Query<&mut Node>,
    mut texts: Query<&mut Text>,
    rows: Query<&ColonRow>,
) {
    let focused = focus.get();
    for (field, mut completer, editable) in &mut completers {
        let value = editable.value().to_string();
        let result = if focused == Some(field) {
            matches_for(&value)
        } else {
            None
        };
        match result {
            Some((colon_byte, matches)) => {
                completer.colon_byte = colon_byte;
                completer.matches = matches;
                if completer.selected >= completer.matches.len() {
                    completer.selected = 0;
                }
                completer.open = true;
            }
            None => {
                if completer.open {
                    completer.open = false;
                    completer.matches.clear();
                    completer.selected = 0;
                }
            }
        }
        reflect_popup(&completer, &mut nodes, &mut texts, &rows);
    }
}

/// Show / hide and fill a completer's popup from its current state: the popup node
/// shown only while open, each row shown for a match (glyph + `:code:`) and hidden
/// otherwise.
fn reflect_popup(
    completer: &ColonComplete,
    nodes: &mut Query<&mut Node>,
    texts: &mut Query<&mut Text>,
    rows: &Query<&ColonRow>,
) {
    set_display(nodes, completer.popup, completer.open);
    for &row_entity in &completer.rows {
        let Ok(row) = rows.get(row_entity) else {
            continue;
        };
        let shown = completer.open && row.index < completer.matches.len();
        set_display(nodes, row_entity, shown);
        if let Some(hit) = completer.matches.get(row.index)
            && let Ok(mut text) = texts.get_mut(row.label)
        {
            let wanted = format!("{}  :{}:", hit.emoji.glyph(), hit.shortcode);
            if text.0 != wanted {
                text.0 = wanted;
            }
        }
    }
}

/// Set a node's display between shown (`Flex`) and hidden (`None`), writing only on
/// a real change.
fn set_display(nodes: &mut Query<&mut Node>, entity: Entity, shown: bool) {
    let wanted = if shown { Display::Flex } else { Display::None };
    if let Ok(mut node) = nodes.get_mut(entity)
        && node.display != wanted
    {
        node.display = wanted;
    }
}

/// Handle the completer's keys while its field is focused and the popup is open:
/// `Up`/`Down` move the selection, `Enter`/`Tab` accept it, `Escape` closes it.
/// Each consumed key is cleared from the frame so a consumer's own `Enter` handler
/// (ordered after [`ColonCompleteSet`]) does not also fire.
fn handle_colon_complete_keys(
    focus: Res<InputFocus>,
    mut keyboard: ResMut<ButtonInput<KeyCode>>,
    mut completers: Query<&mut ColonComplete>,
    mut fields: Query<&mut EditableText>,
    mut font_cx: ResMut<FontCx>,
    mut layout_cx: ResMut<LayoutCx>,
) {
    let Some(field) = focus.get() else {
        return;
    };
    // Snapshot the state, so the arrow / accept branches can re-borrow the query
    // without holding a borrow across `accept_match`.
    let Ok((open, count, selected)) = completers
        .get(field)
        .map(|completer| (completer.open, completer.matches.len(), completer.selected))
    else {
        return;
    };
    if !open || count == 0 {
        return;
    }
    if keyboard.just_pressed(KeyCode::ArrowDown) {
        if let Ok(mut completer) = completers.get_mut(field) {
            let next = completer.selected.saturating_add(1);
            completer.selected = if next >= count { 0 } else { next };
        }
        keyboard.clear_just_pressed(KeyCode::ArrowDown);
    } else if keyboard.just_pressed(KeyCode::ArrowUp) {
        if let Ok(mut completer) = completers.get_mut(field) {
            completer.selected = completer
                .selected
                .checked_sub(1)
                .unwrap_or_else(|| count.saturating_sub(1));
        }
        keyboard.clear_just_pressed(KeyCode::ArrowUp);
    } else if keyboard.just_pressed(KeyCode::Enter) || keyboard.just_pressed(KeyCode::Tab) {
        accept_match(
            field,
            selected,
            &mut completers,
            &mut fields,
            &mut font_cx,
            &mut layout_cx,
        );
        keyboard.clear_just_pressed(KeyCode::Enter);
        keyboard.clear_just_pressed(KeyCode::Tab);
    } else if keyboard.just_pressed(KeyCode::Escape) {
        if let Ok(mut completer) = completers.get_mut(field) {
            completer.open = false;
            completer.matches.clear();
            completer.selected = 0;
        }
        keyboard.clear_just_pressed(KeyCode::Escape);
    }
}

/// Accept match `index` for `field`: replace the trailing token with the glyph,
/// put the caret at the end, and close the popup. Shared by the row click and the
/// `Enter`/`Tab` key.
fn accept_match(
    field: Entity,
    index: usize,
    completers: &mut Query<&mut ColonComplete>,
    fields: &mut Query<&mut EditableText>,
    font_cx: &mut FontCx,
    layout_cx: &mut LayoutCx,
) {
    let Ok(mut completer) = completers.get_mut(field) else {
        return;
    };
    let Some(hit) = completer.matches.get(index).copied() else {
        return;
    };
    if let Ok(mut editable) = fields.get_mut(field) {
        let value = editable.value().to_string();
        let replaced = replace_trailing_token(&value, completer.colon_byte, hit.emoji.glyph());
        editable.editor.set_text(&replaced);
        let mut driver = editable.editor.driver(font_cx, layout_cx);
        driver.refresh_layout();
        driver.move_to_text_end();
    }
    completer.open = false;
    completer.matches.clear();
    completer.selected = 0;
}

/// Highlight the selected row of each open completer, resting the rest, so the row
/// `Enter` would accept is obvious.
fn highlight_colon_rows(
    completers: Query<&ColonComplete>,
    rows: Query<&ColonRow>,
    mut backgrounds: Query<&mut BackgroundColor>,
) {
    for completer in &completers {
        for &row_entity in &completer.rows {
            let Ok(row) = rows.get(row_entity) else {
                continue;
            };
            let selected = completer.open && row.index == completer.selected;
            let wanted = if selected {
                ROW_SELECTED_BACKGROUND
            } else {
                ROW_BACKGROUND
            };
            if let Ok(mut background) = backgrounds.get_mut(row_entity)
                && background.0 != wanted
            {
                background.0 = wanted;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        MAX_MATCHES, MIN_PREFIX, matches_for, replace_trailing_token, trailing_colon_prefix,
    };
    use pretty_assertions::assert_eq;

    /// A boxed error so a test can use `?` instead of the disallowed `expect`.
    type TestError = Box<dyn core::error::Error>;

    /// A trailing token is detected only when the `:` starts a token and the run
    /// to the end is all short-code characters.
    #[test]
    fn trailing_token_detection() {
        assert_eq!(trailing_colon_prefix(":ro"), Some((0, "ro".to_owned())));
        assert_eq!(trailing_colon_prefix("hi :ro"), Some((3, "ro".to_owned())));
        assert_eq!(
            trailing_colon_prefix("hi there :smi"),
            Some((9, "smi".to_owned()))
        );
        // Empty prefix (a lone colon) is a token with an empty prefix.
        assert_eq!(trailing_colon_prefix("a :"), Some((2, String::new())));
        // A colon mid-word is not a token.
        assert_eq!(trailing_colon_prefix("a:ro"), None);
        // An already-closed code (trailing colon) does not re-trigger.
        assert_eq!(trailing_colon_prefix(":rocket:"), None);
        // A non-code char ends the trailing run without a token.
        assert_eq!(trailing_colon_prefix("hi!"), None);
        assert_eq!(trailing_colon_prefix(""), None);
    }

    /// Replacing the trailing token swaps everything from the `:` for the glyph and
    /// keeps the head, including multi-byte content before it.
    #[test]
    fn replace_trailing_token_keeps_the_head() {
        assert_eq!(replace_trailing_token(":ro", 0, "🚀"), "🚀");
        assert_eq!(replace_trailing_token("hi :ro", 3, "🚀"), "hi 🚀");
        // A multi-byte head (an earlier emoji) is preserved by the char-wise copy.
        assert_eq!(replace_trailing_token("😀 :ro", 5, "🚀"), "😀 🚀");
    }

    /// `matches_for` opens only past the minimum prefix length, ranks the exact
    /// short-code first, and caps the list.
    #[test]
    fn matches_gate_on_length_and_cap() -> Result<(), TestError> {
        // Below the minimum: no popup.
        assert!(matches_for(":r").is_none() || MIN_PREFIX <= 1);
        // A real prefix returns matches, capped, with the token offset.
        let (byte, matches) = matches_for("say :rocket").ok_or("rocket should match")?;
        assert_eq!(byte, 4);
        assert!(matches.len() <= MAX_MATCHES);
        assert!(matches.iter().any(|hit| hit.shortcode == "rocket"));
        // No such short-code: no popup.
        assert!(matches_for(":zzzzzznotacode").is_none());
        Ok(())
    }
}
