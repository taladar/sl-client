//! How long `parley::PlainEditor` takes to lay out a **full-size script** — the
//! measurement `viewer-lsl-editor-widget` asked for before building an editor on
//! it.
//!
//! Stock `PlainEditor` relays out its *whole buffer* on every edit, and the
//! script limit is 65,536 characters, so the question was whether one
//! keystroke in a 64 KB script costs a frame — it did (22 ms), which is why the
//! workspace's parley fork now lays the buffer out a paragraph at a time; this
//! is what keeps that honest. These are timings, not assertions — a debug
//! build of the commit hook would make any bound either flaky or meaningless —
//! so they are `#[ignore]`d and run by hand, in release:
//!
//! ```text
//! cargo test --release -p sl-viewer-ui-widgets --test editor_relayout -- --ignored --nocapture
//! ```

#[cfg(test)]
mod timing {
    use std::borrow::Cow;
    use std::ops::Range;
    use std::sync::Arc;
    use std::time::{Duration, Instant};

    use parley::fontique::Blob;
    use parley::{FontContext, FontFamily, LayoutContext, PlainEditor, StyleProperty};

    /// The monospace face the script editor draws in, as the viewer bundles it.
    const MONO: &[u8] = include_bytes!("../../sl-viewer-ui-core/assets/fonts/DejaVuSansMono.ttf");

    /// The editor's font size, in logical pixels.
    const FONT_SIZE: f32 = 14.0;

    /// The script limit Second Life compiles, in characters.
    const SCRIPT_LIMIT: usize = 0x0001_0000;

    /// One stretch of ordinary LSL, repeated to fill the buffer.
    const CHUNK: &str = "// A comment line explaining the next function in some detail.\n\
    integer gChannel = -12345;\n\
    list gItems = [\"alpha\", \"beta\", 3.25, <1.0, 2.0, 3.0>];\n\
    \n\
    say_items(key target)\n\
    {\n\
        integer i;\n\
        for (i = 0; i < llGetListLength(gItems); ++i)\n\
        {\n\
            llRegionSayTo(target, gChannel, llList2String(gItems, i) + \" \" + (string)i);\n\
        }\n\
    }\n\
    \n";

    /// How many timed relayouts each case takes the median of.
    const SAMPLES: usize = 21;

    /// A script of just under `size` bytes, made of whole [`CHUNK`]s.
    fn script(size: usize) -> String {
        let mut text = String::with_capacity(size);
        while text.len().saturating_add(CHUNK.len()) <= size {
            text.push_str(CHUNK);
        }
        text
    }

    /// One range per word-ish token, in one of eight brushes chosen by the
    /// token's text — about the density of a highlighted script, where nearly
    /// every token is coloured, and like a highlighter in that a token's colour
    /// depends on what it is, not on where it is.
    fn token_styles(text: &str) -> Vec<(Range<usize>, StyleProperty<'static, u32>)> {
        let brush = |word: &str| {
            word.bytes()
                .fold(0_u32, |hash, byte| {
                    hash.wrapping_mul(31).wrapping_add(u32::from(byte))
                })
                .wrapping_rem(8)
                .wrapping_add(1)
        };
        let mut styles = Vec::new();
        let mut start = None;
        for (index, character) in text.char_indices() {
            let word = character.is_alphanumeric() || character == '_';
            match (start, word) {
                (None, true) => start = Some(index),
                (Some(from), false) => {
                    let token = text.get(from..index).unwrap_or_default();
                    styles.push((from..index, StyleProperty::Brush(brush(token))));
                    start = None;
                }
                _ => {}
            }
        }
        styles
    }

    /// A font context holding only the bundled monospace face, and an editor over
    /// `text` drawing in it, unwrapped as code is.
    fn editor(text: &str) -> (FontContext, LayoutContext<u32>, PlainEditor<u32>) {
        let mut font_cx = FontContext::new();
        let _families = font_cx
            .collection
            .register_fonts(Blob::new(Arc::new(MONO)), None);
        let mut editor = PlainEditor::new(FONT_SIZE);
        editor
            .edit_styles()
            .insert(StyleProperty::FontFamily(FontFamily::Source(
                Cow::Borrowed("DejaVu Sans Mono"),
            )));
        editor.set_width(None);
        editor.set_text(text);
        (font_cx, LayoutContext::new(), editor)
    }

    /// The median of `samples`.
    fn median(mut samples: Vec<Duration>) -> Duration {
        samples.sort_unstable();
        samples
            .get(samples.len() / 2)
            .copied()
            .unwrap_or(Duration::ZERO)
    }

    /// Time a one-character edit in the middle of the buffer and the relayout it
    /// forces, `SAMPLES` times, and print the median.
    #[expect(
        clippy::print_stdout,
        reason = "a timing, printed for the person who ran it by hand"
    )]
    fn time_keystrokes(label: &str, size: usize, styled: bool) {
        let text = script(size);
        let (mut font_cx, mut layout_cx, mut editor) = editor(&text);
        if styled {
            editor.set_range_styles(token_styles(&text));
        }
        let first = Instant::now();
        editor.refresh_layout(&mut font_cx, &mut layout_cx);
        let initial = first.elapsed();
        let lines = editor.try_layout().map_or(0, parley::Layout::len);

        let middle = text.len() / 2;
        let mut samples = Vec::with_capacity(SAMPLES);
        for _ in 0..SAMPLES {
            editor
                .driver(&mut font_cx, &mut layout_cx)
                .move_to_byte(middle);
            // A keystroke: the `String` splice, the highlighter's ranges
            // recomputed over the new text (every one after the edit has moved),
            // and the relayout that follows — which is what the frame pays.
            let started = Instant::now();
            editor
                .driver(&mut font_cx, &mut layout_cx)
                .insert_or_replace_selection("x");
            if styled {
                let restyled = token_styles(editor.raw_text());
                editor.set_range_styles(restyled);
            }
            editor.refresh_layout(&mut font_cx, &mut layout_cx);
            samples.push(started.elapsed());
            editor.driver(&mut font_cx, &mut layout_cx).backdelete();
            if styled {
                let restyled = token_styles(editor.raw_text());
                editor.set_range_styles(restyled);
            }
            editor.refresh_layout(&mut font_cx, &mut layout_cx);
        }
        println!(
            "{label}: {} bytes, {lines} lines, first layout {initial:?}, keystroke relayout median {:?}",
            text.len(),
            median(samples)
        );
    }

    /// Scripts of a quarter, half and all of the limit, with no styling: the
    /// floor, and how it grows with the buffer.
    #[test]
    #[ignore = "a timing, run by hand in release"]
    fn a_script_relays_out_per_keystroke() {
        for size in [SCRIPT_LIMIT / 4, SCRIPT_LIMIT / 2, SCRIPT_LIMIT] {
            time_keystrokes("plain", size, false);
        }
    }

    /// The same scripts with a style range on nearly every token, which is what
    /// syntax highlighting hands the editor.
    #[test]
    #[ignore = "a timing, run by hand in release"]
    fn a_highlighted_script_relays_out_per_keystroke() {
        for size in [SCRIPT_LIMIT / 4, SCRIPT_LIMIT / 2, SCRIPT_LIMIT] {
            time_keystrokes("highlighted", size, true);
        }
    }
}
