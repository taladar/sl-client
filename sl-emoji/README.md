# sl-emoji

An emoji dataset and lookup layer for a Second Life / OpenSim viewer. It backs
the two emoji features a viewer needs — an inline `:`-completer that turns a
typed short-code (`:rocket:`) into a glyph, and a picker floater showing a
grouped, searchable grid — from one dataset, behind a small, UI-free API.

It is the Rust counterpart of the reference viewer's `llemojidictionary` (the
lookup), which feeds `llpanelemojicomplete` (the inline completer) and
`llfloateremojipicker` (the picker).

## Data source

The colon **short-codes** (`:smile:`) are the de-facto [gemoji] convention, not
part of the Unicode standard; the canonical **names / groups** are Unicode CLDR
annotations. Rather than hand-author either, this crate wraps the [`emojis`]
crate, which bundles both — gemoji short-codes over Unicode-CLDR-ordered emoji,
with names and groups — as a self-contained, no-network dependency. The wrapper
insulates the viewer from that crate's exact API and narrows it to the two
lookups the consumers need.

The backing crate exposes CLDR *names* and gemoji *short-codes* but **no**
separate CLDR *keyword* list, so `search` matches names and short-codes only. If
richer keyword search is wanted later, CLDR annotations (via `icu4x`) can be
pulled behind this same API without changing consumers.

## API

- **Short-code ↔ glyph** — `by_shortcode` / `by_glyph` resolve exact codes and
  glyphs (colons and case ignored); `complete` prefix-completes a short-code for
  the inline completer, yielding one entry per matching short-code sorted
  alphabetically.
- **Grouped, searchable list** — `Group` partitions the dataset into the
  picker's nine tabs (`Group::emojis`); `search` ranks a free-text query across
  names and short-codes (exact short-code → prefix → substring, CLDR order
  breaking ties). A blank query returns nothing so the picker shows its grouped
  list instead.
- **Skin tones** — the six single `SkinTone`s a picker offers, via
  `Emoji::with_skin_tone`. The paired tones the multi-person glyphs can take are
  out of scope — a picker applies one tone at a time.

```rust
use sl_emoji::{by_shortcode, complete, search, Group, SkinTone};

fn main() {
    let rocket = by_shortcode(":rocket:").unwrap();
    assert_eq!(rocket.glyph(), "🚀");

    assert!(complete("rock").iter().any(|m| m.shortcode == "rocket"));
    assert_eq!(search("rocket").first().map(|e| e.glyph()), Some("🚀"));

    let wave = by_shortcode("wave").unwrap();
    let _dark = wave.with_skin_tone(SkinTone::Dark).unwrap();
    let _ = Group::ALL;
}
```

[gemoji]: https://github.com/github/gemoji
[`emojis`]: https://crates.io/crates/emojis
