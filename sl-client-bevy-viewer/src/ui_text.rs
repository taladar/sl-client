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
//! Two pieces make requirement 4 work. The `system_font_discovery` Bevy feature
//! (enabled in this crate's `Cargo.toml`) turns on fontique OS font enumeration
//! and script fallback, so CJK / Cyrillic / Arabic / Hebrew resolve to the
//! host's own fonts instead of rendering as tofu. Colour emoji are the
//! exception: the emoji font most Linux hosts ship today is `COLRv1`, which
//! `swash` (the rasteriser under parley) cannot paint, so a discovered system
//! emoji font would render blank. This module therefore **bundles** the
//! `CBDT`/`CBLC` build of Noto Color Emoji (`assets/fonts/`), registers it as a
//! [`Font`] asset — which Bevy's `load_font_assets_into_font_collection` folds
//! into the fontique collection under its embedded family name — and then binds
//! it as the `Emoji` generic family so parley's fallback picks it for emoji
//! codepoints ([`bind_emoji_family`]).
//!
//! The proof surface is a toggleable panel (the `F4` key, or the
//! `SL_VIEWER_TEXT_DEMO` environment variable so the screenshot harness can
//! capture it) holding one prefilled multi-line [`EditableText`]. Clicking it
//! focuses it (`bevy_input_focus`), after which typing, the host IME, caret
//! motion, and grapheme-aware backspace can all be exercised by hand. It reuses
//! the persistent-node + key-toggle pattern of the [`crate::diagnostics`]
//! overlays.
//!
//! Reference (Firestorm, read-only): `indra/llui/` text widgets and
//! `llpreeditor` (the IME model).

use bevy::prelude::*;
use bevy::text::{EditableText, FontCx, TextCursorStyle};

/// The bundled Noto Color Emoji font, embedded into the binary. This is the
/// `CBDT`/`CBLC` (colour-bitmap) build — the format `swash` can rasterise —
/// rather than the `COLRv1` build most hosts discover, which would render blank.
/// See `assets/fonts/README.md` for the provenance and licence.
const EMOJI_FONT: &[u8] = include_bytes!("../assets/fonts/NotoColorEmoji.ttf");

/// The embedded family name (`name` ID 1) of [`EMOJI_FONT`]. Bevy registers the
/// font under this name in the fontique collection, and [`bind_emoji_family`]
/// binds that name as the `Emoji` generic family.
const EMOJI_FAMILY: &str = "Noto Color Emoji";

/// The key that toggles the text-foundation demo panel on and off.
const TEXT_DEMO_TOGGLE_KEY: KeyCode = KeyCode::F4;

/// The demo editor's font size, in logical pixels (larger than the diagnostic
/// overlays so colour-emoji bitmaps and bidi caret placement are legible).
const DEMO_FONT_SIZE: f32 = 22.0;

/// The demo editor's height, in visible text lines.
const DEMO_VISIBLE_LINES: f32 = 5.0;

/// The demo editor's width, in logical pixels.
const DEMO_WIDTH: f32 = 620.0;

/// The inset, in logical pixels, of the demo panel from the top-left corner
/// (clear of the top-left pipeline overlay, which is hidden by default).
const PANEL_INSET: f32 = 90.0;

/// The one-line instruction shown above the demo editor.
const DEMO_TITLE: &str = "Text foundation demo (F4) - click to focus, then type / use your IME; \
     test caret & backspace over the bidi and emoji runs.";

/// The multilingual + colour-emoji sample prefilled into the demo editor. It is
/// written with explicit `\u{..}` escapes so this source file stays ASCII, and
/// it exercises all four hard requirements at once: a bidi line mixing Latin
/// with right-to-left Hebrew and Arabic, a CJK line (no-tofu), and an emoji line
/// carrying a zero-width-joiner family and a regional-indicator flag (both
/// single graphemes made of several codepoints).
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
    "\u{1F389}\u{2764}\u{FE0F}\u{1F525}",
);

/// The bundled colour-emoji [`Font`] asset: a strong handle kept alive for the
/// app's lifetime (dropping it would remove the asset and clear its collection
/// registration), plus whether the `Emoji` generic family has been bound yet.
#[derive(Resource)]
pub(crate) struct EmojiFont {
    /// Strong handle to the registered emoji [`Font`] asset.
    handle: Handle<Font>,
    /// Whether [`bind_emoji_family`] has already bound the `Emoji` generic
    /// family, so it only binds once.
    bound: bool,
}

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

/// Startup system: register the bundled colour-emoji font as a [`Font`] asset
/// (Bevy folds it into the fontique collection next frame) and spawn the demo
/// panel — a title plus one prefilled multi-line [`EditableText`] — starting
/// shown or hidden per [`TextDemoVisible`].
pub(crate) fn setup_text_demo(
    mut commands: Commands,
    mut fonts: ResMut<Assets<Font>>,
    visible: Res<TextDemoVisible>,
) {
    let handle = fonts.add(Font::from_bytes(EMOJI_FONT.to_vec()));
    commands.insert_resource(EmojiFont {
        handle,
        bound: false,
    });

    let start_visibility = if visible.0 {
        Visibility::Visible
    } else {
        Visibility::Hidden
    };
    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                top: Val::Px(PANEL_INSET),
                left: Val::Px(PANEL_INSET),
                flex_direction: FlexDirection::Column,
                row_gap: Val::Px(8.0),
                padding: UiRect::all(Val::Px(12.0)),
                max_width: Val::Px(DEMO_WIDTH),
                ..default()
            },
            BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.7)),
            start_visibility,
            TextDemoRoot,
        ))
        .with_children(|panel| {
            panel.spawn((
                Text::new(DEMO_TITLE),
                TextFont {
                    font_size: FontSize::Px(13.0),
                    ..default()
                },
                TextColor(Color::srgb(0.80, 0.85, 0.92)),
            ));
            let mut editor = EditableText::new(DEMO_TEXT);
            editor.allow_newlines = true;
            editor.visible_lines = Some(DEMO_VISIBLE_LINES);
            panel.spawn((
                editor,
                // Deliberately keep the *default* single-font primary rather than
                // `FontSource::SansSerif`. parley appends the `Emoji` generic
                // *after* the primary family stack, and on Linux fontconfig's
                // `sans-serif` alias expands to a long list that (on most distros)
                // already contains the host's COLRv1 Noto Color Emoji. That face
                // covers the emoji codepoints, so it wins the query before the
                // `Emoji` generic is ever reached — and `swash` paints COLRv1 as
                // nothing. A single unpolluted primary lets the emoji fall through
                // to the generic bound in [`bind_emoji_family`]. Every other script
                // (CJK / Arabic / Hebrew / Cyrillic) still resolves through
                // parley's per-script system fallback, which is unaffected.
                TextFont {
                    font_size: FontSize::Px(DEMO_FONT_SIZE),
                    ..default()
                },
                TextColor(Color::WHITE),
                // Draw the caret and selection rectangles.
                TextCursorStyle::default(),
                Node {
                    width: Val::Px(DEMO_WIDTH),
                    border: UiRect::all(Val::Px(2.0)),
                    padding: UiRect::all(Val::Px(6.0)),
                    ..default()
                },
                BorderColor::all(Color::srgb(0.40, 0.50, 0.62)),
                BackgroundColor(Color::srgb(0.10, 0.12, 0.16)),
            ));
        });
}

/// Bind the bundled colour-emoji font as parley's `Emoji` generic family, once
/// Bevy's font-loading system has registered it into the fontique collection
/// (which happens the frame after [`setup_text_demo`] adds the asset). parley
/// routes emoji clusters to the `Emoji` generic during shaping, and
/// `matches_with` tries the generic's family before the OS script fallback, so
/// binding the generic makes our font win over the host's emoji font.
///
/// Crucially we bind the font's **per-asset alias family**, not its embedded
/// `"Noto Color Emoji"` name ([`EMOJI_FAMILY`]). With `system_font_discovery`
/// on, the host's own Noto Color Emoji — which is `COLRv1`, a format `swash`
/// cannot paint — is enumerated under that *same* family name, so both faces
/// merge into one family and the blank `COLRv1` face can be selected instead of
/// ours. Bevy also registers each font asset under a unique
/// `asset_id:…` alias naming only that one face, so binding the alias resolves
/// the emoji generic unambiguously to our `CBDT` face.
///
/// Remapping a generic family does **not** invalidate text that was already laid
/// out (the demo editor is shaped the frame it spawns, before this bind runs),
/// so on the frame we bind we also mark every [`TextFont`] changed to force a
/// re-shape — the same mechanism Bevy's own font-asset loader uses on a
/// collection change.
pub(crate) fn bind_emoji_family(
    mut font_cx: ResMut<FontCx>,
    fonts: Res<Assets<Font>>,
    emoji: Option<ResMut<EmojiFont>>,
    mut text_fonts: Query<&mut TextFont>,
) {
    let Some(mut emoji) = emoji else {
        return;
    };
    if emoji.bound {
        return;
    }
    // Wait until Bevy has loaded the asset and stamped its per-asset alias.
    let Some(font) = fonts.get(emoji.handle.id()) else {
        return;
    };
    let alias = font.alias.clone();
    if alias.is_empty() || font_cx.collection.family_id(&alias).is_none() {
        return;
    }
    match font_cx.set_emoji_family(&alias) {
        Ok(()) => {
            emoji.bound = true;
            // Re-shape all existing text now that emoji can resolve to the
            // bundled colour font.
            for mut text_font in &mut text_fonts {
                text_font.set_changed();
            }
            info!("bound colour-emoji fallback to `{EMOJI_FAMILY}` (alias `{alias}`)");
        }
        Err(error) => {
            warn!("failed to bind colour-emoji family: {error}");
        }
    }
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

/// Drive the demo panel's root visibility from [`TextDemoVisible`] whenever it
/// changes, showing or hiding the whole subtree (child visibility inherits).
pub(crate) fn apply_text_demo_visibility(
    visible: Res<TextDemoVisible>,
    mut roots: Query<&mut Visibility, With<TextDemoRoot>>,
) {
    if !visible.is_changed() {
        return;
    }
    let target = if visible.0 {
        Visibility::Visible
    } else {
        Visibility::Hidden
    };
    for mut visibility in &mut roots {
        if *visibility != target {
            *visibility = target;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{DEMO_TEXT, EMOJI_FAMILY, EMOJI_FONT};
    use bevy::text::{EditableText, FontCx, LayoutCx};
    use pretty_assertions::assert_eq;

    /// A boxed error so tests can use `?` instead of disallowed `unwrap`/`expect`.
    type TestError = Box<dyn core::error::Error>;

    /// Read a big-endian `u16` at byte offset `at` in `bytes` (0 if out of
    /// range), assembled with explicit shifts to satisfy the endian-byte lint.
    fn be_u16_at(bytes: &[u8], at: usize) -> u16 {
        let hi = bytes.get(at).copied().map_or(0_u16, u16::from);
        let lo = at
            .checked_add(1)
            .and_then(|next| bytes.get(next))
            .copied()
            .map_or(0_u16, u16::from);
        (hi << 8_u16) | lo
    }

    /// The four-byte tags of an sfnt font's table directory (`numTables`
    /// records of 16 bytes each, starting at offset 12).
    fn sfnt_table_tags(font: &[u8]) -> Vec<[u8; 4]> {
        let count = usize::from(be_u16_at(font, 4));
        font.get(12..)
            .unwrap_or(&[])
            .chunks_exact(16)
            .take(count)
            .filter_map(|record| record.get(0..4))
            .filter_map(|tag| <[u8; 4]>::try_from(tag).ok())
            .collect()
    }

    /// The bundled emoji font must be the `CBDT`/`CBLC` colour-bitmap build that
    /// `swash` can rasterise, and must carry no `COLR` table (the `COLRv1`
    /// format `swash` cannot paint) — the whole reason it is bundled instead of
    /// discovered.
    #[test]
    fn emoji_font_is_a_swash_renderable_colour_bitmap() {
        let tags = sfnt_table_tags(EMOJI_FONT);
        let has = |wanted: &[u8; 4]| tags.iter().any(|tag| tag == wanted);
        assert!(has(b"CBDT"), "bundled emoji font must have a CBDT table");
        assert!(has(b"CBLC"), "bundled emoji font must have a CBLC table");
        assert!(
            !has(b"COLR"),
            "bundled emoji font must not be COLR (swash cannot rasterise COLRv1)"
        );
    }

    /// The bundled font must embed the exact family name [`EMOJI_FAMILY`] that
    /// [`super::bind_emoji_family`] binds as the `Emoji` generic — otherwise the
    /// bind silently fails and emoji render blank. The `name` table stores it as
    /// UTF-16BE, so search for that byte pattern.
    #[test]
    fn emoji_font_embeds_the_expected_family_name() {
        let mut needle = Vec::new();
        for byte in EMOJI_FAMILY.bytes() {
            needle.push(0_u8);
            needle.push(byte);
        }
        assert!(
            EMOJI_FONT
                .windows(needle.len())
                .any(|window| window == needle.as_slice()),
            "the bundled font must embed the `{EMOJI_FAMILY}` family name",
        );
    }

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

    /// The family name is a non-empty ASCII string, so the UTF-16BE search above
    /// is well-formed.
    #[test]
    fn emoji_family_name_is_non_empty_ascii() {
        assert!(!EMOJI_FAMILY.is_empty());
        assert!(EMOJI_FAMILY.is_ascii());
    }

    /// A sanity check that the demo text is the expected three labelled lines.
    #[test]
    fn demo_text_has_three_lines() {
        assert_eq!(DEMO_TEXT.lines().count(), 3);
    }

    /// How many `backdelete()` presses it takes to clear `text`, driving the real
    /// `parley` editor that [`EditableText`] wraps.
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

    /// Requirement 2 (grapheme editing) — a **tripwire recording a known gap**,
    /// not the behaviour we want.
    ///
    /// The requirement is that one backspace deletes exactly one *grapheme
    /// cluster*, so every case below should take **1** press. `parley` 0.9's
    /// `backdelete` instead deletes the whole cluster only for a hard line break
    /// or a single emoji cluster, and otherwise deletes one **codepoint** — so a
    /// ZWJ family peels apart one member at a time, and even `e` + a combining
    /// acute takes two presses. Measured with the viewer's own font setup; it is
    /// not a font/ligature artifact.
    ///
    /// This test asserts what parley *currently does*, so it fails loudly if
    /// parley becomes grapheme-correct — at which point delete it and close the
    /// follow-up (`roadmap/ready/viewer-ui-text-grapheme-backdelete.md`).
    #[test]
    fn backdelete_is_not_grapheme_correct_yet() {
        let family = "\u{1F468}\u{200D}\u{1F469}\u{200D}\u{1F467}\u{200D}\u{1F466}";
        // Wanted: 1 (one grapheme). Actual: one press per codepoint.
        assert_eq!(backspaces_to_clear(family), 7, "ZWJ family");
        // Wanted: 1. Actual: one press per regional indicator.
        assert_eq!(backspaces_to_clear("\u{1F1EF}\u{1F1F5}"), 2, "flag");
        // Wanted: 1. Actual: the VS16 selector deletes separately.
        assert_eq!(backspaces_to_clear("\u{2764}\u{FE0F}"), 2, "heart + VS16");
        // Wanted: 1. Actual: the combining mark deletes separately.
        assert_eq!(backspaces_to_clear("e\u{301}"), 2, "e + combining acute");
        // These two are already correct.
        assert_eq!(backspaces_to_clear("\u{1F389}"), 1, "standalone emoji");
        assert_eq!(backspaces_to_clear("ab"), 2, "two ASCII characters");
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
