//! A single emoji and the skin tones it can take.

use crate::group::Group;

/// One emoji, with its glyph, CLDR name, group and gemoji short-codes.
///
/// This is a thin, `Copy` handle over a `'static` entry in the bundled dataset
/// — constructing one is only ever done by the crate's lookups
/// ([`by_glyph`](crate::by_glyph), [`by_shortcode`](crate::by_shortcode),
/// [`all`](crate::all), [`Group::emojis`]). Two handles are equal when they
/// refer to the same glyph.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct Emoji {
    /// The backing `'static` entry in the bundled `emojis` dataset.
    inner: &'static emojis::Emoji,
}

impl core::fmt::Debug for Emoji {
    /// Show the glyph and CLDR name rather than the opaque backing pointer.
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Emoji")
            .field("glyph", &self.glyph())
            .field("name", &self.name())
            .finish()
    }
}

impl Emoji {
    /// Wrap a backing dataset entry. Internal: the only way a caller obtains an
    /// [`Emoji`] is through the crate's lookups.
    pub(crate) const fn new(inner: &'static emojis::Emoji) -> Self {
        Self { inner }
    }

    /// The emoji glyph itself, e.g. `"🚀"`.
    #[must_use]
    pub const fn glyph(&self) -> &'static str {
        self.inner.as_str()
    }

    /// The Unicode CLDR name, e.g. `"rocket"` for `"🚀"`.
    #[must_use]
    pub const fn name(&self) -> &'static str {
        self.inner.name()
    }

    /// The group this emoji belongs to (the picker's top-level tabs).
    #[must_use]
    pub const fn group(&self) -> Group {
        Group::from_backing(self.inner.group())
    }

    /// The primary gemoji short-code (without the surrounding colons), e.g.
    /// `Some("rocket")`. A handful of emoji have none.
    #[must_use]
    pub fn shortcode(&self) -> Option<&'static str> {
        self.inner.shortcode()
    }

    /// Every gemoji short-code for this emoji (without colons). Most emoji have
    /// zero or one; a few — e.g. `"laughing"` / `"satisfied"` for `"😆"` —
    /// have several. The iterator is empty when the emoji has no short-code.
    pub fn shortcodes(&self) -> impl Iterator<Item = &'static str> + Clone {
        self.inner.shortcodes()
    }

    /// The Unicode emoji version this glyph was introduced in, as
    /// `(major, minor)`. Useful for hiding glyphs a target platform's fonts
    /// will not yet have.
    #[must_use]
    pub const fn unicode_version(&self) -> (u32, u32) {
        let version = self.inner.unicode_version();
        (version.major(), version.minor())
    }

    /// Whether this emoji has skin-tone variants (a person, body part, etc.).
    #[must_use]
    pub fn has_skin_tones(&self) -> bool {
        self.inner.skin_tones().is_some()
    }

    /// This emoji's own skin tone, if it is a single-tone member of a
    /// skin-tone family. Emoji outside any family, and the multi-person glyphs
    /// that carry two independent tones, return [`None`] (this crate models
    /// only the six single tones a picker offers).
    #[must_use]
    pub fn skin_tone(&self) -> Option<SkinTone> {
        self.inner.skin_tone().and_then(SkinTone::from_backing)
    }

    /// This emoji re-cast to `tone`, if it supports skin tones. Applying a tone
    /// to a toneless emoji, or reading a tone off one, returns [`None`].
    #[must_use]
    pub fn with_skin_tone(&self, tone: SkinTone) -> Option<Self> {
        self.inner.with_skin_tone(tone.to_backing()).map(Self::new)
    }
}

impl core::fmt::Display for Emoji {
    /// Display writes the glyph, so `format!("{emoji}")` yields the emoji.
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(self.glyph())
    }
}

/// One of the six single skin tones a viewer's emoji picker offers.
///
/// The Fitzpatrick modifiers. The reference dataset also encodes the paired
/// tones the two-person glyphs (couples, handshakes) can take; those are out of
/// scope here — a picker applies one tone at a time.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum SkinTone {
    /// The default, tone-neutral (yellow) rendering.
    Default,
    /// Fitzpatrick type 1–2.
    Light,
    /// Fitzpatrick type 3.
    MediumLight,
    /// Fitzpatrick type 4.
    Medium,
    /// Fitzpatrick type 5.
    MediumDark,
    /// Fitzpatrick type 6.
    Dark,
}

impl SkinTone {
    /// The six single tones, in picker order (default first, then light → dark).
    pub const ALL: [Self; 6] = [
        Self::Default,
        Self::Light,
        Self::MediumLight,
        Self::Medium,
        Self::MediumDark,
        Self::Dark,
    ];

    /// Map a backing single tone into ours, or [`None`] for a paired tone this
    /// crate does not model.
    const fn from_backing(tone: emojis::SkinTone) -> Option<Self> {
        match tone {
            emojis::SkinTone::Default => Some(Self::Default),
            emojis::SkinTone::Light => Some(Self::Light),
            emojis::SkinTone::MediumLight => Some(Self::MediumLight),
            emojis::SkinTone::Medium => Some(Self::Medium),
            emojis::SkinTone::MediumDark => Some(Self::MediumDark),
            emojis::SkinTone::Dark => Some(Self::Dark),
            _ => None,
        }
    }

    /// Map ours to the backing single tone.
    const fn to_backing(self) -> emojis::SkinTone {
        match self {
            Self::Default => emojis::SkinTone::Default,
            Self::Light => emojis::SkinTone::Light,
            Self::MediumLight => emojis::SkinTone::MediumLight,
            Self::Medium => emojis::SkinTone::Medium,
            Self::MediumDark => emojis::SkinTone::MediumDark,
            Self::Dark => emojis::SkinTone::Dark,
        }
    }
}
