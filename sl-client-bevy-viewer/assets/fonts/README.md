# Bundled fonts

The viewer's whole UI font stack is bundled here and embedded into the binary
(via `include_bytes!` in `src/ui_font.rs`), rather than discovered from the
host. Two reasons:

- **Colour emoji survive.** Resolving UI text through a host font alias pulls in
  the system emoji font, which on most Linux distributions today is `COLRv1` — a
  format `swash` (the rasteriser under `parley`) cannot paint, so those emoji
  render blank. See `src/ui_font.rs` for the full mechanism.
- **The UI looks the same everywhere**, which is also why the reference viewer
  ships its own faces (`indra/newview/fonts/`).

`src/ui_font.rs` registers each file below under a **private family name**
(`SL Viewer Sans`, `SL Viewer Sans Fallback`, `SL Viewer Mono`,
`SL Viewer Emoji`) so that a host copy of the same font cannot merge into the
family and change which face is picked. That is a runtime lookup name only — the
files here are redistributed **verbatim**, and none of them is modified.

Scripts nothing here covers (CJK, Thai, Devanagari, …) still fall back to the
host's fonts per script, via the `system_font_discovery` Bevy feature.

## `InterVariable.ttf`, `InterVariable-Italic.ttf`

**Inter** — the UI body face, used for chat, labels, name tags and prose. A
typeface designed for on-screen UI text, and where the reference viewer is
heading.

Bundled as the **variable** build: one file spans the whole 100–900 weight axis,
which is what Bevy's `TextFont::weight` wants (it notes weight "only supports
variable weight fonts"). The upright and italic files register into one family,
so an italic request resolves to a *real* designed face rather than a
synthesised slant.

Coverage is Latin, Greek and Cyrillic — see `DejaVuSans*.ttf` below for the
scripts it lacks.

- Family name (name ID 1): `Inter` (overridden at registration — see above)
- Version: 4.1
- Source: <https://github.com/rsms/inter>
- Licence: SIL Open Font License 1.1 — see
  [`Inter-LICENSE.txt`](Inter-LICENSE.txt)

## `DejaVuSans*.ttf` (script fallback), `DejaVuSansMono*.ttf`

**DejaVu Sans** in four faces (regular, bold, oblique, bold-oblique) is *not* a
UI face and is never selected directly: it is wired up only as the
**script fallback** for Hebrew, Arabic, Armenian and Georgian — the scripts
Inter does not cover. Bundling it keeps the text foundation's hard bidi
requirement (mixed RTL + Latin) rendering identically on every host rather than
depending on what the host has installed.

**DejaVu Sans Mono** (regular, bold) is the fixed-width face for diagnostics and
other tabular text, mirroring the reference viewer's `fonts.xml`, which maps
`Monospace` to `DejaVuSansMono.ttf`.

Both have plain outlines that `swash` rasterises.

- Family names (name ID 1): `DejaVu Sans`, `DejaVu Sans Mono` (overridden at
  registration — see above)
- Version: 2.37
- Source: <https://dejavu-fonts.github.io/>
- Licence: a permissive Bitstream Vera / Arev licence (DejaVu's own changes are
  public domain) — see [`DejaVu-LICENSE.txt`](DejaVu-LICENSE.txt)

## `NotoColorEmoji.ttf`

The **Noto Color Emoji** font, in its **CBDT/CBLC** (embedded colour-bitmap)
build — the colour-glyph format `swash` can actually paint. `swash` renders
`COLRv0`, `CBDT` and `sbix` but **not** `COLRv1`, and the emoji font shipped by
most Linux distributions today (`Noto-COLRv1.ttf`) is `COLRv1`-only, so relying
on system font discovery for emoji would render blank glyphs.

- Family name (name ID 1): `Noto Color Emoji`
- Colour tables: `CBDT` + `CBLC` (no `COLR`)
- Source: <https://github.com/googlefonts/noto-emoji>,
  `fonts/NotoColorEmoji.ttf`
- Licence: SIL Open Font License 1.1 — see [`OFL.txt`](OFL.txt)
