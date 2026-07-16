# Bundled fonts

## `NotoColorEmoji.ttf`

The **Noto Color Emoji** font, in its **CBDT/CBLC** (embedded colour-bitmap)
build. Bundled and embedded into the viewer binary (via `include_bytes!` in
`src/ui_text.rs`) so colour emoji render without any host font configuration.

Why this specific build: the viewer's text stack rasterises glyphs with
`swash`, which renders the `COLRv0`, `CBDT`, and `sbix` colour-glyph formats but
**not** `COLRv1`. The colour-emoji font shipped by most Linux distributions
today (`Noto-COLRv1.ttf`) is `COLRv1`-only, so relying on system font discovery
for emoji would render blank glyphs. The CBDT/CBLC build below is what `swash`
can actually paint, so it is bundled rather than discovered.

- Family name (name ID 1): `Noto Color Emoji`
- Colour tables: `CBDT` + `CBLC` (no `COLR`)
- Source: <https://github.com/googlefonts/noto-emoji>,
  `fonts/NotoColorEmoji.ttf`
- Licence: SIL Open Font License 1.1 — see [`OFL.txt`](OFL.txt)

All other text (Latin, Cyrillic, CJK, Arabic, Hebrew, …) is served by the host's
own fonts through `parley`/`fontique` system font discovery, which is enabled by
the `system_font_discovery` Bevy feature; only the colour-emoji fallback is
bundled here.
