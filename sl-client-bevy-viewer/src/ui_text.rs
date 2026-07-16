//! UI text & font foundation (`viewer-ui-text-foundation`): the root bring-up of
//! the parley-backed `bevy_ui` text stack, on which the whole viewer UI cluster
//! is built.
//!
//! Bevy 0.19 replaced cosmic-text with `parley` (harfrust shaping, ICU
//! segmentation, fontique fallback) and ships an `EditableText` widget plus IME
//! plumbing in the default `ui` feature. This module stands that stack up far
//! enough to prove the four hard text requirements before the widget layer
//! (`viewer-ui-widget-scaffold`) is written on top of it:
//!
//! 1. **Bidi** — mixed Arabic / Hebrew + Latin lays out in visual order, and the
//!    caret and selection geometry split correctly across runs (parley's
//!    Unicode Bidirectional Algorithm, exercised through `EditableText`).
//! 2. **Grapheme editing** — backspace over an emoji ZWJ family or a
//!    regional-indicator flag deletes exactly one grapheme cluster (parley's
//!    `backdelete`).
//! 3. **IME** — a live CJK IME shows preedit and places its candidate window
//!    (`EditableTextInputPlugin` transports `Ime::Preedit` / `Ime::Commit`, and
//!    drives `Window::ime_enabled` / `Window::ime_position`).
//! 4. **No tofu** — a CJK + colour-emoji line renders fully, in colour.
//!
//! Two pieces make requirement 4 work, and both now live in [`crate::ui_font`],
//! which owns the viewer's font stack. The `system_font_discovery` Bevy feature
//! (enabled in this crate's `Cargo.toml`) turns on fontique OS font enumeration
//! and script fallback, so CJK / Cyrillic / Arabic / Hebrew resolve to the
//! host's own fonts instead of rendering as tofu. Colour emoji are the
//! exception: the emoji font most Linux hosts ship today is `COLRv1`, which
//! `swash` (the rasteriser under parley) cannot paint, so a discovered system
//! emoji font would render blank — hence the bundled `CBDT` build that
//! [`crate::ui_font`] registers and binds as the `Emoji` generic.
//!
//! The proof surface is a toggleable panel (the `F4` key, or the
//! `SL_VIEWER_TEXT_DEMO` environment variable so the screenshot harness can
//! capture it) holding one prefilled multi-line [`EditableText`]. Clicking it —
//! or tabbing to it — focuses it (`bevy_input_focus`), after which typing, the
//! host IME, caret motion, and grapheme-aware backspace can all be exercised by
//! hand. It reuses the persistent-node + key-toggle pattern of the
//! [`crate::diagnostics`] overlays.
//!
//! The panel is built on [`crate::ui`]'s scaffold, which post-dates it: it hangs
//! off the [`UiRoot`], is laid out by [`column()`] and a [`LogicalMargin`] rather
//! than an absolute pixel inset (so it mirrors under an RTL locale), and is
//! shown / hidden through [`UiPanelShown`].
//!
//! Reference (Firestorm, read-only): `indra/llui/` text widgets and
//! `llpreeditor` (the IME model).

use bevy::input_focus::tab_navigation::TabIndex;
use bevy::prelude::*;
use bevy::text::{EditableText, TextCursorStyle};

use crate::ui::{LogicalMargin, LogicalRect, UiPanelShown, UiRoot, column};
use crate::ui_font::UiFont;

/// The key that toggles the text-foundation demo panel on and off.
const TEXT_DEMO_TOGGLE_KEY: KeyCode = KeyCode::F4;

/// The demo editor's font size, in logical pixels (larger than the diagnostic
/// overlays so colour-emoji bitmaps and bidi caret placement are legible).
const DEMO_FONT_SIZE: f32 = 22.0;

/// The demo panel's instruction-line font size, in logical pixels.
const TITLE_FONT_SIZE: f32 = 13.0;

/// The demo editor's height, in visible text lines.
const DEMO_VISIBLE_LINES: f32 = 5.0;

/// The widest the demo editor is allowed to get, in logical pixels, before its
/// text wraps. A *bound*, not a size (`viewer-ui-widget-scaffold`'s
/// content-driven layout convention): the editor is as wide as its longest line
/// needs, up to this.
const DEMO_MAX_WIDTH: f32 = 620.0;

/// The demo panel's margin, in logical pixels, from the leading inline edge and
/// the top of the [`UiRoot`] — enough to clear the top-leading pipeline overlay,
/// which is hidden by default.
const PANEL_MARGIN: f32 = 90.0;

/// The one-line instruction shown above the demo editor.
const DEMO_TITLE: &str = "Text foundation demo (F4) - click to focus, then type / use your IME. \
     Each emoji/grapheme cluster should take ONE backspace; the VS16 heart should be colour, \
     the VS15 one monochrome.";

/// The multilingual + colour-emoji sample prefilled into the demo editor. It is
/// written with explicit `\u{..}` escapes so this source file stays ASCII, and
/// it exercises all four hard requirements at once: a bidi line mixing Latin
/// with right-to-left Hebrew and Arabic, a CJK line (no-tofu), an emoji line
/// carrying a zero-width-joiner family and a regional-indicator flag (both
/// single graphemes made of several codepoints), and a grapheme line whose
/// clusters are *not* emoji.
///
/// Every cluster on the last two lines should take exactly **one** backspace,
/// and `\u{2764}\u{FE0F}` should render in **colour** — both behaviours come
/// from the patched `parley` (see the workspace `Cargo.toml`), so this panel is
/// the by-hand counterpart to
/// `ui_font`'s `emoji_presentation_selector_beats_the_text_font` and this
/// module's `backspace_deletes_exactly_one_grapheme` (not linked: `rustdoc` does
/// not see `#[cfg(test)]` items).
const DEMO_TEXT: &str = concat!(
    // Bidi: Latin + Hebrew "shalom" + Arabic "marhaba" + Latin.
    "Bidi: Hello ",
    "\u{5E9}\u{5DC}\u{5D5}\u{5DD}",
    " ",
    "\u{645}\u{631}\u{62D}\u{628}\u{627}",
    " world 123\n",
    // No-tofu: Chinese, Japanese, Korean.
    "CJK: ",
    "\u{4F60}\u{597D}\u{4E16}\u{754C} ",
    "\u{3053}\u{3093}\u{306B}\u{3061}\u{306F} ",
    "\u{C548}\u{B155}\u{D558}\u{C138}\u{C694}\n",
    // Colour emoji: a ZWJ family, the Japan flag (two regional indicators), then
    // a few standalone emoji.
    "Emoji: ",
    "\u{1F468}\u{200D}\u{1F469}\u{200D}\u{1F467}\u{200D}\u{1F466} ",
    "\u{1F1EF}\u{1F1F5} ",
    "\u{1F389}\u{2764}\u{FE0F}\u{1F525} ",
    // A waving hand plus a skin-tone modifier: one grapheme, two codepoints.
    "\u{1F44B}\u{1F3FD}\n",
    // Graphemes that are *not* emoji, and were equally broken before the parley
    // fix: `e` + a combining acute, a Hangul syllable built from three jamo, and
    // a Devanagari consonant + vowel sign.
    "Graphemes: ",
    "e\u{301} ",
    "\u{1100}\u{1161}\u{11A8} ",
    "\u{915}\u{93F}\n",
    // A heart with the *text* presentation selector (VS15), next to the emoji
    // one: these two request opposite presentations and must not look alike.
    "VS15/VS16: \u{2764}\u{FE0E} vs \u{2764}\u{FE0F}",
);

/// Whether the text-foundation demo panel is currently shown. Toggled by
/// [`TEXT_DEMO_TOGGLE_KEY`]; hidden by default so it stays out of the way until
/// the text stack is being inspected.
#[derive(Resource, Debug, Clone, Copy, Default)]
pub(crate) struct TextDemoVisible(pub(crate) bool);

impl TextDemoVisible {
    /// The initial visibility, seeded from the `SL_VIEWER_TEXT_DEMO` environment
    /// variable so the offline screenshot harness (which cannot press
    /// [`TEXT_DEMO_TOGGLE_KEY`]) can capture the panel: set to start shown, unset
    /// to start hidden (the interactive default). The key still toggles it either
    /// way.
    pub(crate) fn from_env() -> Self {
        Self(std::env::var_os("SL_VIEWER_TEXT_DEMO").is_some())
    }
}

/// A marker component tagging the demo panel's root node, so the toggle system
/// can find and show / hide the whole subtree.
#[derive(Component, Debug, Clone, Copy)]
pub(crate) struct TextDemoRoot;

/// Startup system: spawn the demo panel — a title plus one prefilled multi-line
/// [`EditableText`] — starting shown or hidden per [`TextDemoVisible`]. The
/// fonts it renders with are installed by [`crate::ui_font::register_ui_fonts`],
/// and both are wired by [`crate::ui::ViewerUiPlugin`], whose [`UiRoot`] this
/// panel hangs off — so this system must run after
/// [`crate::ui::UiScaffoldSystems::SpawnRoot`].
pub(crate) fn setup_text_demo(
    mut commands: Commands,
    visible: Res<TextDemoVisible>,
    root: Res<UiRoot>,
) {
    let display = if visible.0 {
        Display::Flex
    } else {
        Display::None
    };
    commands
        .spawn((
            Node {
                display,
                padding: UiRect::all(Val::Px(12.0)),
                // A bound rather than a width: the panel is as wide as its text
                // needs, and wraps instead of overflowing when that is a lot.
                max_width: Val::Px(DEMO_MAX_WIDTH),
                ..column(Val::Px(8.0))
            },
            // The panel sits inside the corner where the UI's text starts, not
            // "the top left": under `SL_VIEWER_UI_DIRECTION=rtl` this margin
            // resolves onto the right edge instead, and the panel mirrors across
            // the window with no code here knowing about it. This is the
            // scaffold's direction-neutrality convention doing its one job.
            LogicalMargin(LogicalRect {
                inline_start: Val::Px(PANEL_MARGIN),
                block_start: Val::Px(PANEL_MARGIN),
                ..LogicalRect::ZERO
            }),
            BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.7)),
            UiPanelShown(visible.0),
            TextDemoRoot,
            ChildOf(root.0),
        ))
        .with_children(|panel| {
            panel.spawn((
                Text::new(DEMO_TITLE),
                UiFont::Sans.at(TITLE_FONT_SIZE),
                TextColor(Color::srgb(0.80, 0.85, 0.92)),
            ));
            let mut editor = EditableText::new(DEMO_TEXT);
            editor.allow_newlines = true;
            editor.visible_lines = Some(DEMO_VISIBLE_LINES);
            panel.spawn((
                editor,
                // The bundled sans family, via the one font-selection helper
                // (`viewer-ui-text-font-family-selection`). It carries no emoji
                // glyphs of its own, so emoji fall through to the `Emoji` generic
                // and paint in colour; every other script (CJK / Arabic / Hebrew
                // / Cyrillic) resolves through parley's per-script system
                // fallback. Note what this must *not* be: a generic like
                // `SansSerif` would expand through fontconfig's alias list, which
                // carries the host's blank-rendering COLRv1 emoji font — see
                // [`crate::ui_font`].
                UiFont::Sans.at(DEMO_FONT_SIZE),
                TextColor(Color::WHITE),
                // Draw the caret and selection rectangles.
                TextCursorStyle::default(),
                // Reachable by `Tab` as well as by clicking, via the scaffold's
                // tab navigation — ahead of its demo buttons, which take 1 and 2.
                // `EditableText` lets `Tab` bubble up to the navigation observer
                // rather than inserting a tab, except while an IME is composing
                // (when the keystroke belongs to the IME).
                TabIndex(0),
                Node {
                    // A bound, not a width: the editor is as wide as its widest
                    // line, and wraps beyond this. A fixed width here used to
                    // overflow the panel's own `max_width` by exactly its
                    // padding, which is the fixed-rect failure mode in
                    // miniature.
                    max_width: Val::Px(DEMO_MAX_WIDTH),
                    border: UiRect::all(Val::Px(2.0)),
                    padding: UiRect::all(Val::Px(6.0)),
                    ..default()
                },
                BorderColor::all(Color::srgb(0.40, 0.50, 0.62)),
                BackgroundColor(Color::srgb(0.10, 0.12, 0.16)),
            ));
        });
}

/// Toggle the demo panel when [`TEXT_DEMO_TOGGLE_KEY`] is pressed.
pub(crate) fn toggle_text_demo(
    keyboard: Res<ButtonInput<KeyCode>>,
    mut visible: ResMut<TextDemoVisible>,
) {
    if keyboard.just_pressed(TEXT_DEMO_TOGGLE_KEY) {
        visible.0 = !visible.0;
    }
}

/// Drive the demo panel's [`UiPanelShown`] from [`TextDemoVisible`] whenever it
/// changes, leaving the scaffold's `apply_panel_visibility` to do the hiding —
/// which is more than it sounds: a closed panel must also leave no gap in the
/// root's flow, stop being reachable by `Tab`, and give up the keyboard.
pub(crate) fn apply_text_demo_visibility(
    visible: Res<TextDemoVisible>,
    mut panels: Query<&mut UiPanelShown, With<TextDemoRoot>>,
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
    use super::DEMO_TEXT;
    use bevy::text::{EditableText, FontCx, LayoutCx};
    use pretty_assertions::assert_eq;

    /// A boxed error so tests can use `?` instead of disallowed `unwrap`/`expect`.
    type TestError = Box<dyn core::error::Error>;

    /// The prefilled sample must contain a character exercising each of the four
    /// hard requirements, so a live tester always has all of them on screen.
    #[test]
    fn demo_text_exercises_the_four_requirements() {
        // Bidi: a right-to-left script character (Hebrew lamed).
        assert!(DEMO_TEXT.contains('\u{5DC}'), "missing an RTL character");
        // No-tofu: a CJK ideograph.
        assert!(DEMO_TEXT.contains('\u{4F60}'), "missing a CJK character");
        // Grapheme editing: the zero-width joiner fusing the emoji family, and a
        // regional-indicator symbol (the flag's building block).
        assert!(DEMO_TEXT.contains('\u{200D}'), "missing a ZWJ");
        assert!(
            DEMO_TEXT.contains('\u{1F1EF}'),
            "missing a regional-indicator symbol"
        );
        // Colour emoji: the party popper.
        assert!(DEMO_TEXT.contains('\u{1F389}'), "missing a colour emoji");
    }

    /// A sanity check that the demo text is the expected labelled lines.
    #[test]
    fn demo_text_has_five_lines() {
        assert_eq!(DEMO_TEXT.lines().count(), 5);
    }

    /// How many `backdelete()` presses it takes to clear `text`, driving the real
    /// `parley` editor that [`EditableText`] wraps.
    ///
    /// Deliberately a bare [`FontCx`]: the counts are the editor's own logic and
    /// do not depend on the font stack — verified by measuring identical counts
    /// with the viewer's full bundled stack registered.
    fn backspaces_to_clear(text: &str) -> usize {
        let mut font_cx = FontCx::default();
        let mut layout_cx = LayoutCx::default();
        let mut editable = EditableText::new(text);
        {
            let mut driver = editable.editor.driver(&mut font_cx, &mut layout_cx);
            driver.refresh_layout();
            driver.move_to_text_end();
        }
        let mut presses = 0_usize;
        // Bounded so a non-advancing editor cannot spin forever.
        while !editable.editor.text().to_string().is_empty() && presses < 16 {
            let before = editable.editor.text().to_string();
            {
                let mut driver = editable.editor.driver(&mut font_cx, &mut layout_cx);
                driver.refresh_layout();
                driver.backdelete();
            }
            presses = presses.saturating_add(1);
            if editable.editor.text().to_string() == before {
                break;
            }
        }
        presses
    }

    /// Requirement 2 (grapheme editing): one backspace deletes exactly one
    /// **grapheme cluster** — the unit a reader perceives as a character.
    ///
    /// This was the text foundation's one failing hard requirement. `parley` 0.9
    /// deleted one *codepoint* except for a hard line break or a single emoji
    /// cluster, so a ZWJ family took seven presses and even `e` + a combining
    /// acute took two. Fixed in `parley` itself rather than worked around here
    /// (`viewer-ui-text-grapheme-backdelete`); we build against a patched fork
    /// until the fix lands upstream and `bevy_text` moves to it — see the
    /// `[patch.crates-io]` block in the workspace `Cargo.toml`.
    ///
    /// The last three cases are deliberately not emoji: they were equally broken,
    /// which is why the fix is grapheme segmentation rather than a better emoji
    /// check. This test therefore also guards against the patch silently
    /// vanishing.
    #[test]
    fn backspace_deletes_exactly_one_grapheme() {
        for (name, text) in [
            (
                "ZWJ family",
                "\u{1F468}\u{200D}\u{1F469}\u{200D}\u{1F467}\u{200D}\u{1F466}",
            ),
            ("regional-indicator flag", "\u{1F1EF}\u{1F1F5}"),
            ("heart + VS16", "\u{2764}\u{FE0F}"),
            ("standalone emoji", "\u{1F389}"),
            ("waving hand + skin tone", "\u{1F44B}\u{1F3FD}"),
            ("e + combining acute", "e\u{301}"),
            ("Hangul jamo syllable", "\u{1100}\u{1161}\u{11A8}"),
        ] {
            assert_eq!(
                backspaces_to_clear(text),
                1,
                "{name}: backspace deleted less than one grapheme. If this fails, check the \
                 parley `[patch.crates-io]` in the workspace `Cargo.toml` — unpatched parley \
                 0.9 deletes one codepoint, so this case takes several presses."
            );
        }
        // Separate graphemes must still take one press each, or the fix would be
        // over-deleting rather than correct.
        assert_eq!(
            backspaces_to_clear("ab"),
            2,
            "two ASCII characters: backspace over-deleted, taking more than one grapheme"
        );
    }

    /// Requirement 1 (bidi), headless: in a Latin+Hebrew line the caret must move
    /// in **visual** order, so the byte offsets a rightward caret visits are not
    /// monotonically increasing once it enters the right-to-left run. That
    /// non-monotonicity *is* the bidi behaviour (a naive logical-order editor
    /// would only ever step forward).
    #[test]
    fn caret_moves_in_visual_order_across_a_bidi_boundary() -> Result<(), TestError> {
        // "abc" + Hebrew "שלום" + "def": one LTR run, one RTL run, one LTR run.
        let text = "abc\u{5E9}\u{5DC}\u{5D5}\u{5DD}def";
        let mut font_cx = FontCx::default();
        let mut layout_cx = LayoutCx::default();
        let mut editable = EditableText::new(text);
        let mut offsets = Vec::new();
        {
            let mut driver = editable.editor.driver(&mut font_cx, &mut layout_cx);
            driver.move_to_text_start();
            for _step in 0..text.chars().count() {
                driver.move_right();
                offsets.push(driver.editor.raw_selection().focus().index());
            }
        }
        // The caret visits every position exactly once and ends at the text end.
        assert_eq!(offsets.last().copied(), Some(text.len()));
        // Visual-order movement means at least one rightward step moves *backwards*
        // in byte order (stepping through the reversed RTL run).
        let went_backwards = offsets
            .windows(2)
            .any(|pair| matches!(pair, [before, after] if after < before));
        assert!(
            went_backwards,
            "a rightward caret across an RTL run must visit byte offsets \
             non-monotonically; got {offsets:?}"
        );
        Ok(())
    }
}
