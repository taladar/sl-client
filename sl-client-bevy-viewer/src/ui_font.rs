//! The viewer's UI font stack (`viewer-ui-text-font-family-selection`): the one
//! place fonts are chosen, so no widget ever hand-picks a font again.
//!
//! # Why this module exists
//!
//! Bringing up the text foundation ([`crate::ui_text`]) surfaced a trap that the
//! whole UI cluster would otherwise inherit: **a generic [`FontSource`]
//! silently destroys colour emoji.** `fontique` expands a generic
//! (`SansSerif`, `Monospace`, …) through fontconfig's alias list — ~150 families
//! on a typical Linux host — and most distributions put the system
//! `Noto Color Emoji` (a `COLRv1` font) on the `sans-serif` alias so emoji
//! render in any text. `parley` appends the `Emoji` generic *after* the primary
//! family stack (`select_font` in `parley::shape`), so that `COLRv1` face
//! matches the emoji codepoints and ends the query **before** the `Emoji`
//! generic — i.e. before the bundled colour font is ever offered. `swash` cannot
//! paint `COLRv1`, so the emoji come out blank.
//!
//! The fix is to never let a host alias list into the primary stack. Every face
//! the UI uses is **bundled** and registered under a **private family name**
//! ([`SANS_FAMILY`], [`MONO_FAMILY`], [`EMOJI_FAMILY`]) that no host font can
//! collide with, and widgets select one through [`UiFont`] — which always
//! resolves to [`FontSource::Family`], never to a generic. Bundling also makes
//! the UI look the same on every host, which is why the reference viewer ships
//! its own faces rather than trusting the system.
//!
//! # The stack
//!
//! | Role | Family | Faces |
//! | --- | --- | --- |
//! | [`UiFont::Sans`] | [`SANS_FAMILY`] | `Inter` variable, upright + italic (weight 100–900) |
//! | [`UiFont::Mono`] | [`MONO_FAMILY`] | `DejaVu Sans Mono` regular + bold |
//! | *(none — script fallback)* | [`SANS_FALLBACK_FAMILY`] | `DejaVu Sans` regular / bold / oblique / bold-oblique |
//! | *(none — `Emoji` generic)* | [`EMOJI_FAMILY`] | `Noto Color Emoji` (`CBDT`) |
//!
//! `Inter` is a UI typeface designed for on-screen text, and is where the
//! reference viewer is heading. It is bundled as its **variable** build, so one
//! file spans the whole weight axis — which is what `TextFont::weight` wants
//! (Bevy notes weight "only supports variable weight fonts"); registering both
//! faces of the family under one name is what additionally makes `style` select
//! a **real** italic rather than a synthesised slant.
//!
//! `Inter` covers Latin, Greek and Cyrillic but not Hebrew, Arabic, Armenian or
//! Georgian, so [`SCRIPT_FALLBACKS`] points those scripts at a bundled
//! `DejaVu Sans` ([`SANS_FALLBACK_FAMILY`]) — reachable *only* as a fallback,
//! never selectable as a role. That keeps [`crate::ui_text`]'s hard bidi
//! requirement (mixed RTL + Latin) rendering from bundled faces on every host.
//! Scripts we bundle nothing for (CJK, Thai, Devanagari, …) still resolve
//! through the host via `system_font_discovery`; that path is unaffected by the
//! trap above, since it is consulted *after* the primary family and the `Emoji`
//! generic, not before.
//!
//! There is deliberately **no emoji role**: emoji inside text of either role
//! already resolve to the bundled colour font through the `Emoji` generic, which
//! [`register_ui_fonts`] binds to [`EMOJI_FAMILY`]. A role would not help the
//! one case that *is* broken — a dual-presentation codepoint like `❤️` — see
//! the note below.
//!
//! # Known-broken: `❤️` renders monochrome
//!
//! A codepoint carrying the emoji-presentation selector (`U+FE0F`, VS16) still
//! renders as its monochrome text glyph. This is **not** fixable from here, and
//! the cause is worth recording because it is not the obvious one:
//! `parley`'s cluster mapper requires *every* char in a cluster to be in the
//! font's `cmap` to count the font a `Complete` match
//! (`analysis/cluster.rs`), and `U+FE0F` is `General_Category = Mn`, so it is
//! excluded neither by the control-character filter in `map_len` nor by
//! `contributes_to_shaping`. Noto's `CBDT` build has no `FE0F` in its `cmap`
//! (nor does `Inter`), so for the cluster `[U+2764, U+FE0F]` the emoji font maps
//! 1 of 2 → ratio `0.5` → **rejected**; `DejaVu Sans` maps both → `1.0` → wins.
//! The selector that *requests* emoji presentation is what disqualifies the
//! emoji font. Reordering font selection cannot help: the emoji font is rejected
//! even when it is the sole requested family.
//!
//! The fix is upstream in `parley` (exclude default-ignorables such as VS16 from
//! the mapping ratio, and use the `is_variation_selector` flag it already tracks
//! but leaves behind `#[allow(dead_code, reason = "To be used in more complete
//! emoji checking, in select_font")]`). Tracked by
//! `viewer-ui-text-emoji-presentation`, together with the related
//! `viewer-ui-text-grapheme-backdelete` — both are the same
//! `cluster.is_emoji` / VS16 handling in `parley`.
//!
//! # Three layers of defence
//!
//! [`register_ui_fonts`] installs the stack at startup, and does three things so
//! that text renders correctly no matter how a caller asks for a font:
//!
//! 1. **The private families** above — what [`UiFont`] resolves to. This is the
//!    path every viewer widget takes.
//! 2. **The generics are re-pointed.** `set_generic_family` replaces a generic's
//!    whole family list with a single family, so binding `SansSerif`,
//!    `Monospace` and `Emoji` to our private families means even a stray generic
//!    resolves to a bundled face and never reaches fontconfig's alias list. This
//!    is what defuses the trap at the root — including for text Bevy itself
//!    shapes.
//! 3. **Bevy's default font is replaced.** Text spawned with a bare
//!    `TextFont { font_size, ..default() }` uses `FontSource::Handle(default)`,
//!    which is Bevy's built-in `FiraMono-subset` — a Latin-only *monospace*
//!    subset. Overwriting that asset with the upright `Inter` face makes
//!    forgetting [`UiFont`] merely lose the italic face rather than render the
//!    UI in a mono subset. (A handle always resolves to a single face, so italic
//!    via [`UiFont`] still needs the family.)
//!
//! Layers 2 and 3 are safety nets, not licence to skip [`UiFont`]: a generic
//! that we do *not* bind (`Serif`, `SystemUi`, `Cursive`, …) still walks the
//! host alias list and still eats emoji, which is why
//! `tests::no_generic_font_source_outside_this_module` fails the build if one
//! appears.
//!
//! # Re-check: has `swash` gained `COLRv1`?
//!
//! Not as of `swash` 0.2.9 (checked 2026-07-16), so the whole hazard stands.
//! `swash`'s `scale::color` reads the `COLR` **version 0** header only —
//! `numBaseGlyphRecords` at offset 2, `baseGlyphRecordsOffset` at 4,
//! `layerRecordsOffset` at 8 — without so much as checking the table's version
//! field. In a `COLRv1`-only font those v0 record counts are zero, so the lookup
//! finds no layers and the glyph paints as nothing. Its `Source` enum likewise
//! offers only `ColorOutline` (`COLRv0`) and `ColorBitmap` (`CBDT`/`sbix`). If a
//! later `swash` adds `COLRv1`, the host's emoji font becomes usable and this
//! module could drop the bundled emoji font and the emoji generic binding —
//! re-check `swash`'s `scale::color` before assuming it still cannot.
//!
//! Reference (Firestorm, read-only): `indra/newview/fonts/fonts.xml` — the same
//! model of named font roles mapped to bundled files, never to OS aliases.

use bevy::prelude::*;
use bevy::text::FontCx;
use parley::GenericFamily;
use parley::fontique::{Blob, FallbackKey, FontInfoOverride, Script};

/// The bundled `Inter` upright variable face — the UI body font. One file
/// carries the whole 100–900 weight axis, which is what `TextFont::weight`
/// wants (Bevy notes weight "only supports variable weight fonts").
const SANS_UPRIGHT: &[u8] = include_bytes!("../assets/fonts/InterVariable.ttf");

/// The bundled `Inter` italic variable face, likewise carrying the full weight
/// axis.
const SANS_ITALIC: &[u8] = include_bytes!("../assets/fonts/InterVariable-Italic.ttf");

/// The bundled `DejaVu Sans` regular face — script fallback, not a UI face.
const FALLBACK_REGULAR: &[u8] = include_bytes!("../assets/fonts/DejaVuSans.ttf");

/// The bundled `DejaVu Sans` bold face.
const FALLBACK_BOLD: &[u8] = include_bytes!("../assets/fonts/DejaVuSans-Bold.ttf");

/// The bundled `DejaVu Sans` oblique (italic) face.
const FALLBACK_OBLIQUE: &[u8] = include_bytes!("../assets/fonts/DejaVuSans-Oblique.ttf");

/// The bundled `DejaVu Sans` bold-oblique (bold italic) face.
const FALLBACK_BOLD_OBLIQUE: &[u8] = include_bytes!("../assets/fonts/DejaVuSans-BoldOblique.ttf");

/// The bundled `DejaVu Sans Mono` regular face.
const MONO_REGULAR: &[u8] = include_bytes!("../assets/fonts/DejaVuSansMono.ttf");

/// The bundled `DejaVu Sans Mono` bold face.
const MONO_BOLD: &[u8] = include_bytes!("../assets/fonts/DejaVuSansMono-Bold.ttf");

/// The bundled Noto Color Emoji font. This is the `CBDT`/`CBLC` (colour-bitmap)
/// build — the format `swash` can rasterise — rather than the `COLRv1` build
/// most hosts ship, which would render blank. See `assets/fonts/README.md` for
/// the provenance and licence.
const EMOJI_FONT: &[u8] = include_bytes!("../assets/fonts/NotoColorEmoji.ttf");

/// The private family name the bundled `Inter` faces are registered under.
///
/// Deliberately **not** the faces' embedded `Inter` name: with
/// `system_font_discovery` on, a host that also has Inter installed would
/// enumerate its own copy under that same name and the two would merge into one
/// family, making which face wins depend on the host. A name no host font can
/// carry keeps the family exactly the faces bundled here.
pub(crate) const SANS_FAMILY: &str = "SL Viewer Sans";

/// The private family name the bundled `DejaVu Sans` faces are registered
/// under. This is **not** a [`UiFont`] role: nothing selects it directly. It is
/// reachable only as the script fallback wired up by [`SCRIPT_FALLBACKS`], for
/// the scripts `Inter` does not cover.
pub(crate) const SANS_FALLBACK_FAMILY: &str = "SL Viewer Sans Fallback";

/// The private family name the bundled monospace faces are registered under.
/// Private for the same reason as [`SANS_FAMILY`].
pub(crate) const MONO_FAMILY: &str = "SL Viewer Mono";

/// The private family name the bundled colour-emoji font is registered under.
///
/// Private is load-bearing here rather than merely tidy: the host's own
/// `Noto Color Emoji` is `COLRv1`, and under the shared embedded name the two
/// faces merge into one family from which the blank `COLRv1` face may be
/// selected instead of ours.
pub(crate) const EMOJI_FAMILY: &str = "SL Viewer Emoji";

/// A UI font role: which bundled family a piece of viewer text is set in.
///
/// This is the **only** way viewer UI text should pick a font. It always
/// resolves to a bundled private family, so it cannot fall into the generic /
/// colour-emoji trap described in the module docs. Weight and slant are *not*
/// roles — each family carries its real bold and italic faces, so ask for them
/// through `TextFont`'s own `weight` / `style` fields:
///
/// ```ignore
/// use bevy::text::{FontWeight, FontStyle};
///
/// // Regular body text.
/// UiFont::Sans.at(13.0)
/// // Bold body text — resolves to the real bold face, not a synthesised one.
/// UiFont::Sans.at(13.0).with_font_weight(FontWeight::BOLD)
/// // Italic monospace.
/// UiFont::Mono.at(11.0).with_font_style(FontStyle::Italic)
/// ```
///
/// Mirrors the reference viewer's named font roles (`fonts.xml`'s `SansSerif`,
/// `Monospace`, `Emoji`, …), which likewise map to bundled files.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum UiFont {
    /// The UI body face (`Inter`) — chat, labels, name tags, prose. The default
    /// for anything that is not tabular.
    Sans,
    /// The fixed-width face (`DejaVu Sans Mono`) — diagnostics, the pipeline
    /// overlay, script and console text, and anything else whose columns must
    /// line up.
    Mono,
}

impl UiFont {
    /// The private family name this role resolves to.
    pub(crate) const fn family(self) -> &'static str {
        match self {
            Self::Sans => SANS_FAMILY,
            Self::Mono => MONO_FAMILY,
        }
    }

    /// Every role, for tests that must cover the whole vocabulary.
    #[cfg(test)]
    const ALL: [Self; 2] = [Self::Sans, Self::Mono];

    /// A [`TextFont`] in this role at `size` logical pixels, in the regular
    /// weight and upright style. Chain `TextFont`'s own `with_font_weight` /
    /// `with_font_style` for bold or italic.
    pub(crate) fn at(self, size: f32) -> TextFont {
        TextFont {
            font_size: FontSize::Px(size),
            ..default()
        }
        .with_family(self.family())
    }
}

/// Every bundled face, paired with the private family it is registered under.
/// Faces sharing a family name merge into one family, keeping each face's own
/// weight and style as read from the file — which is what lets `TextFont`'s
/// `weight` / `style` select a real face.
const BUNDLED_FACES: &[(&str, &[u8])] = &[
    (SANS_FAMILY, SANS_UPRIGHT),
    (SANS_FAMILY, SANS_ITALIC),
    (SANS_FALLBACK_FAMILY, FALLBACK_REGULAR),
    (SANS_FALLBACK_FAMILY, FALLBACK_BOLD),
    (SANS_FALLBACK_FAMILY, FALLBACK_OBLIQUE),
    (SANS_FALLBACK_FAMILY, FALLBACK_BOLD_OBLIQUE),
    (MONO_FAMILY, MONO_REGULAR),
    (MONO_FAMILY, MONO_BOLD),
    (EMOJI_FAMILY, EMOJI_FONT),
];

/// The scripts `Inter` does not cover but the bundled `DejaVu Sans` does, as
/// ISO-15924 tags. Each is pointed at [`SANS_FALLBACK_FAMILY`] so the text
/// renders from a bundled face rather than whatever the host happens to have —
/// the same determinism argument that motivates bundling at all, and it is what
/// keeps [`crate::ui_text`]'s hard bidi requirement (mixed Hebrew / Arabic +
/// Latin) rendering identically everywhere.
///
/// Verified against `InterVariable.ttf`'s `cmap`: it has Latin, Greek and
/// Cyrillic, and lacks every script below.
///
/// This deliberately **replaces** the host's fallback list for these scripts
/// rather than appending to it. The trade: a host with a *better* face for a
/// script (a proper Naskh Arabic, say) no longer gets to use it. Scripts we do
/// not bundle at all (CJK, Thai, Devanagari, …) are untouched and still resolve
/// through the host, as does anything DejaVu itself lacks.
const SCRIPT_FALLBACKS: &[[u8; 4]] = &[
    *b"Hebr", // Hebrew
    *b"Arab", // Arabic
    *b"Armn", // Armenian
    *b"Geor", // Georgian
];

/// The generics [`register_ui_fonts`] re-points at a bundled family, so that
/// even text that asks for one by generic — ours or Bevy's own — resolves to a
/// bundled face instead of walking the host's fontconfig alias list. Only the
/// generics we actually bundle a font for are listed: pointing, say, `Cursive`
/// at DejaVu Sans would be a lie, and the guard test bans reaching for the
/// unbound ones anyway.
const GENERIC_BINDINGS: &[(GenericFamily, &str)] = &[
    (GenericFamily::SansSerif, SANS_FAMILY),
    (GenericFamily::UiSansSerif, SANS_FAMILY),
    (GenericFamily::SystemUi, SANS_FAMILY),
    (GenericFamily::Monospace, MONO_FAMILY),
    (GenericFamily::UiMonospace, MONO_FAMILY),
    (GenericFamily::Emoji, EMOJI_FAMILY),
];

/// Register `face` into `font_cx`'s fontique collection under `family`,
/// overriding only the family name so the face keeps its own weight, width and
/// style. Returns whether any face in the blob was registered.
fn register_face(font_cx: &mut FontCx, family: &str, face: &[u8]) -> bool {
    let registered = font_cx.collection.register_fonts(
        Blob::from(face.to_vec()),
        Some(FontInfoOverride {
            family_name: Some(family),
            ..Default::default()
        }),
    );
    !registered.is_empty()
}

/// Startup system: install the UI font stack — register every bundled face
/// under its private family, re-point the generics we bundle a font for, and
/// replace Bevy's `FiraMono-subset` default font with DejaVu Sans.
///
/// This runs in `Startup`, which is early enough without any explicit ordering:
/// text is shaped in `PostUpdate`, so every family is in the collection before
/// the first layout regardless of when the text entities themselves spawn. That
/// is a real simplification over binding the emoji generic from a font *asset*,
/// which could not resolve until Bevy's loader had run a frame later and so had
/// to force a re-shape of everything already laid out.
pub(crate) fn register_ui_fonts(mut font_cx: ResMut<FontCx>, mut fonts: ResMut<Assets<Font>>) {
    for (family, face) in BUNDLED_FACES {
        if !register_face(&mut font_cx, family, face) {
            error!("bundled UI font for family `{family}` registered no faces");
        }
    }

    // Layer 2: re-point the generics we have a bundled font for, so even a stray
    // generic resolves to a bundled face instead of the host's alias list.
    for (generic, family) in GENERIC_BINDINGS {
        if let Err(error) = font_cx.set_generic_family(*generic, family) {
            warn!("failed to bind the `{generic:?}` generic family to `{family}`: {error}");
        }
    }

    // Point the scripts Inter lacks at the bundled DejaVu, so RTL and the other
    // extra scripts come from a bundled face instead of the host's.
    if let Some(fallback_id) = font_cx.collection.family_id(SANS_FALLBACK_FAMILY) {
        for tag in SCRIPT_FALLBACKS {
            let key = FallbackKey::new(Script::from_bytes(*tag), None);
            if !font_cx
                .collection
                .set_fallbacks(key, core::iter::once(fallback_id))
            {
                let script = String::from_utf8_lossy(tag);
                warn!("failed to point the `{script}` script fallback at `{SANS_FALLBACK_FAMILY}`");
            }
        }
    } else {
        error!("`{SANS_FALLBACK_FAMILY}` is not registered; script fallbacks not installed");
    }

    // Layer 3: replace Bevy's built-in default font (a Latin-only *monospace*
    // subset) so text that forgets `UiFont` still lands in the UI body face.
    // Bevy's own font loader picks this up next frame and registers it; nothing
    // ever registers `FiraMono-subset`, because the asset is replaced before
    // that loader first runs.
    if let Err(error) = fonts.insert(
        AssetId::<Font>::default(),
        Font::from_bytes(SANS_UPRIGHT.to_vec()),
    ) {
        warn!("failed to replace the default font with `{SANS_FAMILY}`: {error}");
    }

    info!(
        "registered the UI font stack (`{SANS_FAMILY}`, `{MONO_FAMILY}`, `{EMOJI_FAMILY}`, \
         `{SANS_FALLBACK_FAMILY}` for {} scripts)",
        SCRIPT_FALLBACKS.len()
    );
}

#[cfg(test)]
mod tests {
    use super::{
        BUNDLED_FACES, EMOJI_FAMILY, EMOJI_FONT, FALLBACK_REGULAR, GENERIC_BINDINGS, MONO_FAMILY,
        SANS_FALLBACK_FAMILY, SANS_FAMILY, SANS_UPRIGHT, SCRIPT_FALLBACKS, UiFont, register_face,
    };
    use bevy::text::{FontCx, FontSource};
    use parley::fontique::{FallbackKey, Script};
    use parley::{FontFamily, LayoutContext, StyleProperty};
    use pretty_assertions::assert_eq;

    /// A boxed error so tests can use `?` instead of disallowed `unwrap`/`expect`.
    type TestError = Box<dyn core::error::Error>;

    /// A [`FontCx`] with the bundled stack registered and the generics bound,
    /// mirroring what [`super::register_ui_fonts`] installs at startup (minus
    /// the default-font asset swap, which needs a running `App`).
    ///
    /// This deliberately starts from a real [`FontCx::default`], which — with
    /// `system_font_discovery` on — enumerates the host's fonts too. That is
    /// what gives [`emoji_in_ui_text_resolves_to_the_bundled_colour_font`] its
    /// teeth: the host's `COLRv1` emoji font is present and free to win, exactly
    /// as it did before this module existed.
    fn ui_font_cx() -> FontCx {
        let mut font_cx = FontCx::default();
        for (family, face) in BUNDLED_FACES {
            assert!(
                register_face(&mut font_cx, family, face),
                "bundled face for `{family}` registered nothing"
            );
        }
        for (generic, family) in GENERIC_BINDINGS {
            assert!(
                font_cx.set_generic_family(*generic, family).is_ok(),
                "binding the `{generic:?}` generic to `{family}` failed"
            );
        }
        let Some(fallback_id) = font_cx.collection.family_id(SANS_FALLBACK_FAMILY) else {
            unreachable!("`{SANS_FALLBACK_FAMILY}` was just registered above")
        };
        for tag in SCRIPT_FALLBACKS {
            assert!(
                font_cx.collection.set_fallbacks(
                    FallbackKey::new(Script::from_bytes(*tag), None),
                    core::iter::once(fallback_id)
                ),
                "pointing the `{}` script fallback failed",
                String::from_utf8_lossy(tag)
            );
        }
        font_cx
    }

    /// Each role resolves to a family that actually exists in the collection
    /// once the stack is registered — a typo in a private family name would
    /// otherwise only show up as invisible text at runtime.
    #[test]
    fn every_role_resolves_to_a_registered_family() {
        let mut font_cx = ui_font_cx();
        for role in UiFont::ALL {
            assert!(
                font_cx.collection.family_id(role.family()).is_some(),
                "`{role:?}` resolves to unregistered family `{}`",
                role.family()
            );
        }
    }

    /// [`UiFont::at`] must always produce a [`FontSource::Family`] — never a
    /// generic (the trap) and never a bare handle (which would resolve to a
    /// single face and lose bold/italic).
    #[test]
    fn ui_font_always_selects_a_named_family() {
        for role in UiFont::ALL {
            let font = UiFont::at(role, 13.0).font;
            assert!(
                matches!(&font, FontSource::Family(family) if family.as_str() == role.family()),
                "`{role:?}` selected {font:?} rather than the named family `{}`",
                role.family()
            );
        }
    }

    /// Every family must carry **real** faces for the slants and weights the UI
    /// asks for, so `TextFont`'s `weight` / `style` select a designed face
    /// rather than a synthesised slant or fake-bold. Registering faces under a
    /// shared family name is what merges them; if that broke, each face would
    /// land in its own family and bold would silently become fake.
    ///
    /// The counts differ by design: the sans family is two **variable** Inter
    /// faces (upright + italic) that each span the whole weight axis, whereas
    /// the DejaVu families are static faces and so need one file per weight.
    #[test]
    fn every_family_carries_the_faces_the_ui_asks_for() -> Result<(), TestError> {
        let mut font_cx = ui_font_cx();
        for (family, want) in [
            (SANS_FAMILY, 2),
            (SANS_FALLBACK_FAMILY, 4),
            (MONO_FAMILY, 2),
            (EMOJI_FAMILY, 1),
        ] {
            let info = font_cx
                .collection
                .family_by_name(family)
                .ok_or("family is not registered")?;
            assert_eq!(info.fonts().len(), want, "`{family}` face count");
        }
        Ok(())
    }

    /// The sans faces must be **variable** across the weight axis, which is what
    /// lets one file serve every weight the UI asks for (Bevy notes that
    /// `TextFont::weight` "only supports variable weight fonts"). A static Inter
    /// would silently collapse every weight request onto one face.
    #[test]
    fn sans_spans_the_weight_axis() -> Result<(), TestError> {
        let mut font_cx = ui_font_cx();
        let info = font_cx
            .collection
            .family_by_name(SANS_FAMILY)
            .ok_or("the sans family is not registered")?;
        for font in info.fonts() {
            // Compared as bytes because the axis `Tag` type comes from
            // `read_fonts`, which `fontique` does not re-export — so it cannot be
            // named here without taking on another direct dependency.
            let weight_axis = font
                .axes()
                .iter()
                .find(|axis| axis.tag.to_be_bytes() == *b"wght")
                .ok_or("a sans face has no `wght` axis")?;
            assert!(
                weight_axis.min <= 400.0 && weight_axis.max >= 700.0,
                "the sans weight axis must at least span regular..bold; got {}..{}",
                weight_axis.min,
                weight_axis.max
            );
        }
        Ok(())
    }

    /// Hebrew and Arabic must render from the **bundled** DejaVu, not from
    /// whatever the host happens to have: Inter covers neither, and
    /// [`crate::ui_text`]'s hard bidi requirement (mixed RTL + Latin) should
    /// look the same on every machine. Latin in the same string must still come
    /// from Inter, or the fallback would have swallowed the whole line.
    #[test]
    fn rtl_scripts_fall_back_to_the_bundled_dejavu() {
        let mut font_cx = ui_font_cx();
        // "abc" + Hebrew "shalom" + Arabic "marhaba".
        let text = "abc\u{5E9}\u{5DC}\u{5D5}\u{5DD}\u{645}\u{631}\u{62D}\u{628}\u{627}";
        let lengths = resolved_run_lengths(&mut font_cx, SANS_FAMILY, text);
        assert!(
            lengths.contains(&FALLBACK_REGULAR.len()),
            "RTL text must resolve to the bundled {}-byte DejaVu face; got runs of {lengths:?}",
            FALLBACK_REGULAR.len()
        );
        assert!(
            lengths.contains(&SANS_UPRIGHT.len()),
            "Latin must still resolve to the bundled {}-byte Inter face; got runs of {lengths:?}",
            SANS_UPRIGHT.len()
        );
    }

    /// Shape `text` through `font_cx` in `family`, returning the bundled-font
    /// byte length each resulting run resolved to. Drives the real `parley`
    /// layout the same way `bevy_text`'s pipeline does, so what it reports is
    /// the font that would actually be rasterised on screen.
    fn resolved_run_lengths(font_cx: &mut FontCx, family: &str, text: &str) -> Vec<usize> {
        let mut layout_cx: LayoutContext<[u8; 0]> = LayoutContext::new();
        let mut builder = layout_cx.ranged_builder(&mut font_cx.context, text, 1.0, true);
        builder.push_default(StyleProperty::FontFamily(FontFamily::named(family)));
        builder.push_default(StyleProperty::FontSize(16.0));
        let mut layout = builder.build(text);
        layout.break_all_lines(None);
        layout
            .lines()
            .flat_map(|line| line.runs())
            .map(|run| run.font().data.as_ref().len())
            .collect()
    }

    /// **The behaviour the whole module exists to protect**, checked end-to-end
    /// through a real `parley` layout rather than by inspecting configuration.
    ///
    /// An emoji set in the UI's sans family must resolve to the *bundled* `CBDT`
    /// font. This is the exact measurement that surfaced the trap: with a
    /// generic primary the emoji run resolved to the host's system `COLRv1`
    /// font, which `swash` paints as nothing; with the bundled private family it
    /// resolves to the `CBDT` font and paints in colour.
    ///
    /// Note this test only has teeth on a host that *has* a system emoji font to
    /// lose to — which is precisely the configuration that broke, and the one
    /// this runs on. Latin in the same string must still come from the sans
    /// face, or the emoji font would have swallowed the whole line.
    #[test]
    fn emoji_in_ui_text_resolves_to_the_bundled_colour_font() {
        let mut font_cx = ui_font_cx();
        // A party popper: an emoji with no glyph in any text font, so font
        // selection alone decides which face paints it.
        let lengths = resolved_run_lengths(&mut font_cx, SANS_FAMILY, "hi \u{1F389}");
        assert!(
            lengths.contains(&EMOJI_FONT.len()),
            "the emoji run must resolve to the bundled {}-byte CBDT font; got runs of {lengths:?} \
             bytes (the host's COLRv1 emoji font would render blank)",
            EMOJI_FONT.len()
        );
        assert!(
            lengths.contains(&SANS_UPRIGHT.len()),
            "the Latin run must resolve to the bundled {}-byte sans face; got runs of {lengths:?}",
            SANS_UPRIGHT.len()
        );
    }

    /// **A guard against building on an unpatched `parley`.** A codepoint carrying
    /// the emoji-presentation selector (`U+FE0F`, VS16) must render in colour,
    /// per UTS #51 — even though `Inter` covers `U+2764` with a monochrome
    /// dingbat and is the primary family.
    ///
    /// Two `parley` defects had to be fixed for this to hold, both absent from
    /// the `0.9.0` that `bevy_text` 0.19 pins, so this fails loudly if the
    /// `[patch.crates-io]` in the workspace `Cargo.toml` is dropped before
    /// `bevy_text` moves to a parley that has them:
    ///
    /// 1. a variation selector was required to be in the font's `cmap` for the
    ///    font to count as a complete match — no emoji font maps `U+FE0F`, so the
    ///    emoji font was rejected outright (fixed upstream in 0.10.0);
    /// 2. the emoji family was appended *after* the requested families, so a text
    ///    font covering `U+2764` won regardless (submitted upstream —
    ///    `roadmap/deferred/viewer-ui-text-parley-pr-vs16.md`).
    ///
    /// The `❤` and `5` cases are the other half of the contract: without a
    /// selector nothing is requested, so the text font must still win. They would
    /// catch a "fix" that simply put the emoji family first — which would render
    /// digits as emoji, since `is_emoji` is the raw `Emoji` property.
    #[test]
    fn emoji_presentation_selector_beats_the_text_font() {
        let mut font_cx = ui_font_cx();
        for (name, text, want_emoji) in [
            ("heart + VS16 must be colour", "\u{2764}\u{FE0F}", true),
            (
                "victory hand + VS16 must be colour",
                "\u{270C}\u{FE0F}",
                true,
            ),
            ("bare heart stays text", "\u{2764}", false),
            ("bare digit stays text", "5", false),
        ] {
            let lengths = resolved_run_lengths(&mut font_cx, SANS_FAMILY, text);
            let got_emoji = lengths.contains(&EMOJI_FONT.len());
            assert_eq!(
                got_emoji,
                want_emoji,
                "{name}: resolved to runs of {lengths:?} bytes (bundled emoji font is {} B, \
                 Inter is {} B). If this fails, check the parley `[patch.crates-io]`.",
                EMOJI_FONT.len(),
                SANS_UPRIGHT.len()
            );
        }
    }

    /// The private family names must be names no real font carries, since the
    /// whole point is that a host font cannot merge into them.
    #[test]
    fn private_family_names_are_ours() {
        for family in [SANS_FAMILY, MONO_FAMILY, EMOJI_FAMILY] {
            assert!(
                family.starts_with("SL Viewer "),
                "`{family}` should be namespaced so no host font collides with it"
            );
        }
    }

    /// **The guard the whole module exists for.** No viewer source outside this
    /// module may name a generic [`FontSource`]: a generic expands through the
    /// host's fontconfig alias list, which on a typical Linux box contains the
    /// system `COLRv1` emoji font, which shadows the bundled colour font and
    /// renders emoji blank (`swash` cannot paint `COLRv1` — see the module
    /// docs). Use [`UiFont`] instead.
    ///
    /// [`super::register_ui_fonts`] re-points the handful of generics we bundle
    /// a font for, but the rest (`Serif`, `Cursive`, `Fantasy`, …) still walk
    /// the alias list, so the ban stays blanket rather than trying to track
    /// which generics happen to be safe today.
    #[test]
    fn no_generic_font_source_outside_this_module() -> Result<(), TestError> {
        /// The `FontSource` variants that are safe anywhere: an explicit family
        /// (what `UiFont` produces) and a handle to a specific face.
        const ALLOWED: [&str; 2] = ["FontSource::Family", "FontSource::Handle"];
        let src = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let mut offenders = Vec::new();
        for entry in fs_err::read_dir(&src)? {
            let path = entry?.path();
            if path.extension().is_none_or(|ext| ext != "rs")
                || path.file_name().is_some_and(|name| name == "ui_font.rs")
            {
                continue;
            }
            let text = fs_err::read_to_string(&path)?;
            for (number, line) in text.lines().enumerate() {
                // Skip doc/comment lines: the trap is worth *describing* outside
                // this module, just not invoking.
                if line.trim_start().starts_with("//") {
                    continue;
                }
                if line.contains("FontSource::")
                    && !ALLOWED.iter().any(|allowed| line.contains(allowed))
                {
                    let name = path.file_name().unwrap_or_default();
                    offenders.push(format!(
                        "{name:?}:{}: {}",
                        number.saturating_add(1),
                        line.trim()
                    ));
                }
            }
        }
        assert!(
            offenders.is_empty(),
            "a generic `FontSource` shadows the bundled colour-emoji font and renders emoji \
             blank; select a font through `UiFont` instead. Offending lines:\n{}",
            offenders.join("\n")
        );
        Ok(())
    }

    /// The bundled sans face must be a plain-outline font `swash` can rasterise:
    /// no colour tables at all, and in particular no `COLR`. A sans face that
    /// carried its own emoji glyphs would shadow the emoji family from *inside*
    /// the primary family — the very failure this module prevents from outside.
    #[test]
    fn bundled_sans_has_no_colour_tables() {
        let tags = sfnt_table_tags(SANS_UPRIGHT);
        for tag in [b"COLR", b"CBDT", b"sbix"] {
            assert!(
                !tags.iter().any(|found| found == tag),
                "the sans face must not carry the `{}` colour table",
                String::from_utf8_lossy(tag)
            );
        }
    }

    /// The bundled emoji font must be the `CBDT`/`CBLC` colour-bitmap build that
    /// `swash` can rasterise, and must carry no `COLR` table (the `COLRv1`
    /// format `swash` cannot paint) — the whole reason it is bundled rather than
    /// discovered from the host.
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
}
