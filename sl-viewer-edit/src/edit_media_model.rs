//! The **pure half** of the per-face media editor (`viewer-media-prim-browser`,
//! the media-settings addendum): what the selected faces' media looks like as
//! one form, and what a form's edit does to each face — no ECS, so every rule
//! is a unit test.
//!
//! The reference spreads this over `LLPanelFace::refreshMedia` /
//! `updateMediaSettings` (the gather), the three `LLPanelMediaSettings*::
//! getValues` (which fields an Apply carries) and `LLSelectMgr::
//! selectionSetMedia` + `LLVOVolume::syncMediaData` (how an edit lands on a
//! face). The rules, as they are here:
//!
//! - **A face with no media reads as the default entry.** The gather is the
//!   reference's `getSelectedTEValue` with a default functor, so a selection of
//!   one media face and one bare face shows its fields *mixed*, not the media
//!   face's values.
//! - **A mixed field is left alone unless it was changed.** The reference marks
//!   such a widget *tentative* and leaves it out of the Apply until the user
//!   touches it; here a field goes into the edit when the selection agreed on it
//!   **or** the form's value differs from what was shown.
//! - **A face without media only gains it from a home URL.** An edit that does
//!   not set the home URL passes a bare face by (`selectionSetMedia`'s "skip
//!   adding/updating media"); one that does gives it the default entry merged
//!   with the edit.
//! - **Permissions merge per bit.** The reference writes the whole bitfield as
//!   soon as one of its three checkboxes is not tentative, which (its own
//!   comment says) is "not quite the user expectation": a mixed bit is then
//!   written as whatever the box happened to show. Here each bit is its own
//!   field, so a mixed bit the user did not touch keeps each face's own value.

use sl_client_bevy::{MEDIA_PERM_ANYONE, MEDIA_PERM_GROUP, MEDIA_PERM_OWNER, MediaEntry};

/// The media-permission bits in the order the Customize tab lists them: the
/// owner, the object's group, anyone.
pub(crate) const MEDIA_PERM_BITS: [u8; 3] = [MEDIA_PERM_OWNER, MEDIA_PERM_GROUP, MEDIA_PERM_ANYONE];

/// The largest media surface size the General tab accepts, in pixels (the
/// reference's `width_pixels` / `height_pixels` spinner `max_val`).
pub(crate) const MAX_MEDIA_PIXELS: i32 = 2048;

/// One field as the selection shows it: the value, and whether the selected
/// faces disagree about it (the reference's *tentative*).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) struct Shown<T> {
    /// The value shown — the first face's when the faces disagree.
    pub(crate) value: T,
    /// Whether the selected faces disagree.
    pub(crate) mixed: bool,
}

impl<T: PartialEq + Clone> Shown<T> {
    /// Fold the faces' values into one shown value: the first face's, marked
    /// mixed when any other face differs. `None` for an empty selection.
    fn gather<'item>(mut values: impl Iterator<Item = &'item T>) -> Option<Self>
    where
        T: 'item,
    {
        let first = values.next()?.clone();
        let mixed = values.any(|value| *value != first);
        Some(Self {
            value: first,
            mixed,
        })
    }

    /// The value an edit should write, when it should write one: the form's
    /// value if the faces agreed (so writing it is a no-op or the intended
    /// change) or the user changed it; nothing for a mixed field left as shown.
    fn edited(&self, form: &T) -> Option<T> {
        (!self.mixed || *form != self.value).then(|| form.clone())
    }
}

/// What the Texture tab's media line says about the selection — the reference's
/// `media_title` / `media_info`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) enum MediaSummary {
    /// No selected face carries media.
    #[default]
    None,
    /// One media configuration, named by its home URL (which may be empty).
    Single(String),
    /// The selected faces carry different media (`Multiple Media`).
    Multiple,
}

/// The selected faces' media, gathered into the form the Media Settings window
/// shows: every field with whether the faces agree on it, and the summary the
/// Texture tab's media line reads.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) struct MediaSelectionView {
    /// The home page.
    pub(crate) home_url: Shown<String>,
    /// The page currently shown (read-only; changed only by navigation or the
    /// Reset button).
    pub(crate) current_url: Shown<String>,
    /// Loop playback.
    pub(crate) auto_loop: Shown<bool>,
    /// The first click interacts instead of focusing.
    pub(crate) first_click_interact: Shown<bool>,
    /// Zoom the camera to the face on click.
    pub(crate) auto_zoom: Shown<bool>,
    /// Play without a click.
    pub(crate) auto_play: Shown<bool>,
    /// Scale the media to the face.
    pub(crate) auto_scale: Shown<bool>,
    /// The surface width in pixels.
    pub(crate) width_pixels: Shown<i32>,
    /// The surface height in pixels.
    pub(crate) height_pixels: Shown<i32>,
    /// The control style (`0` standard, `1` mini).
    pub(crate) controls: Shown<i32>,
    /// The *interact* permission per bit, in [`MEDIA_PERM_BITS`] order.
    pub(crate) perms_interact: [Shown<bool>; 3],
    /// The *show controls* permission per bit, in [`MEDIA_PERM_BITS`] order.
    pub(crate) perms_control: [Shown<bool>; 3],
    /// Enforce the white-list.
    pub(crate) whitelist_enable: Shown<bool>,
    /// The white-list patterns.
    pub(crate) whitelist: Shown<Vec<String>>,
    /// Whether any selected face carries media.
    pub(crate) any_media: bool,
    /// The Texture tab's one-line summary.
    pub(crate) summary: MediaSummary,
}

/// One selected face as the gather sees it: whether its texture entry says it
/// carries media, and the entry the `ObjectMedia` capability last reported for
/// it.
#[derive(Debug, Clone, Copy)]
pub(crate) struct FaceMedia<'entry> {
    /// The texture entry's media flag (`MF_HAS_MEDIA`).
    pub(crate) flagged: bool,
    /// The capability's entry for the face, if any.
    pub(crate) entry: Option<&'entry MediaEntry>,
}

impl FaceMedia<'_> {
    /// The entry this face *has* — only when the texture entry agrees it carries
    /// media, since an entry left over from before a removal is not media the
    /// face shows (OpenSim's `ObjectMedia` GET drops such entries the same way).
    fn media(&self) -> Option<&MediaEntry> {
        self.entry.filter(|_entry| self.flagged)
    }
}

/// The text a media URL field shows for `url` (empty for none).
fn url_text(url: Option<&url::Url>) -> String {
    url.map_or_else(String::new, |url| url.as_str().to_owned())
}

/// Whether `bit` is set in the permission field.
const fn has_bit(field: u8, bit: u8) -> bool {
    field & bit != 0
}

/// Gather one field of every entry into a [`Shown`] (the default for no
/// entries).
fn field<T: PartialEq + Clone + Default>(
    entries: &[&MediaEntry],
    read: impl Fn(&MediaEntry) -> T,
) -> Shown<T> {
    let values: Vec<T> = entries.iter().map(|entry| read(entry)).collect();
    Shown::gather(values.iter()).unwrap_or_default()
}

/// Gather the selected faces into the form the Media Settings window shows.
///
/// A face without media reads as [`MediaEntry::default`] (see the module
/// documentation), and an empty selection is the default form.
pub(crate) fn gather_media(faces: &[FaceMedia<'_>]) -> MediaSelectionView {
    let default = MediaEntry::default();
    let entries: Vec<&MediaEntry> = faces
        .iter()
        .map(|face| face.media().unwrap_or(&default))
        .collect();
    let bits = |read: fn(&MediaEntry) -> u8| {
        MEDIA_PERM_BITS.map(|bit| field(&entries, |entry| has_bit(read(entry), bit)))
    };
    MediaSelectionView {
        home_url: field(&entries, |entry| url_text(entry.home_url.as_ref())),
        current_url: field(&entries, |entry| url_text(entry.current_url.as_ref())),
        auto_loop: field(&entries, |entry| entry.auto_loop),
        first_click_interact: field(&entries, |entry| entry.first_click_interact),
        auto_zoom: field(&entries, |entry| entry.auto_zoom),
        auto_play: field(&entries, |entry| entry.auto_play),
        auto_scale: field(&entries, |entry| entry.auto_scale),
        width_pixels: field(&entries, |entry| entry.width_pixels),
        height_pixels: field(&entries, |entry| entry.height_pixels),
        controls: field(&entries, |entry| entry.controls),
        perms_interact: bits(|entry| entry.perms_interact),
        perms_control: bits(|entry| entry.perms_control),
        whitelist_enable: field(&entries, |entry| entry.whitelist_enable),
        whitelist: field(&entries, |entry| entry.whitelist.clone()),
        any_media: faces.iter().any(|face| face.media().is_some()),
        summary: summarise(faces),
    }
}

/// The Texture tab's media line for the selected faces — the reference's
/// `refreshMedia` title: nothing when no face carries media, the home URL of
/// the one configuration they carry, or *Multiple Media* when they carry more
/// than one. Faces without media do not count as a second configuration.
fn summarise(faces: &[FaceMedia<'_>]) -> MediaSummary {
    let mut distinct: Vec<&MediaEntry> = Vec::new();
    for entry in faces.iter().filter_map(FaceMedia::media) {
        if !distinct.contains(&entry) {
            distinct.push(entry);
        }
    }
    match distinct.as_slice() {
        [] => MediaSummary::None,
        [entry] => MediaSummary::Single(url_text(entry.home_url.as_ref())),
        _several => MediaSummary::Multiple,
    }
}

/// A per-bit edit of a media-permission field: the bits to write, and their
/// values.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) struct PermEdit {
    /// The bits the edit writes.
    pub(crate) mask: u8,
    /// Their new values (only the bits in `mask` count).
    pub(crate) bits: u8,
}

impl PermEdit {
    /// Apply the edit to one face's field, keeping the bits it does not write.
    const fn apply(self, field: u8) -> u8 {
        (field & !self.mask) | (self.bits & self.mask)
    }
}

/// What an edit does to one URL field of a face.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) enum UrlEdit {
    /// Leave the face's URL as it is.
    #[default]
    Keep,
    /// Write this URL (`None` clears it).
    Write(Option<url::Url>),
}

impl UrlEdit {
    /// Whether the edit writes the URL.
    const fn writes(&self) -> bool {
        matches!(self, Self::Write(_))
    }
}

/// What an Apply (or a Reset) writes to each selected face: every field is
/// either left alone (`None`) or set.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) struct MediaEdit {
    /// The home page. Writing it is also what lets a face without media gain
    /// some.
    pub(crate) home_url: UrlEdit,
    /// The current page; only the Reset button writes it (clearing it).
    pub(crate) current_url: UrlEdit,
    /// Loop playback.
    pub(crate) auto_loop: Option<bool>,
    /// The first click interacts.
    pub(crate) first_click_interact: Option<bool>,
    /// Zoom on click.
    pub(crate) auto_zoom: Option<bool>,
    /// Play without a click.
    pub(crate) auto_play: Option<bool>,
    /// Scale to the face.
    pub(crate) auto_scale: Option<bool>,
    /// The surface width.
    pub(crate) width_pixels: Option<i32>,
    /// The surface height.
    pub(crate) height_pixels: Option<i32>,
    /// The control style.
    pub(crate) controls: Option<i32>,
    /// The interact permission bits.
    pub(crate) perms_interact: PermEdit,
    /// The show-controls permission bits.
    pub(crate) perms_control: PermEdit,
    /// Enforce the white-list.
    pub(crate) whitelist_enable: Option<bool>,
    /// The white-list.
    pub(crate) whitelist: Option<Vec<String>>,
}

impl MediaEdit {
    /// The face's media after this edit, or `None` when the face has none and
    /// the edit does not give it any (no home URL — see the module
    /// documentation).
    pub(crate) fn apply(&self, current: Option<&MediaEntry>) -> Option<MediaEntry> {
        if current.is_none() && !self.home_url.writes() {
            return None;
        }
        let mut entry = current.cloned().unwrap_or_default();
        if let UrlEdit::Write(url) = &self.home_url {
            entry.home_url.clone_from(url);
        }
        if let UrlEdit::Write(url) = &self.current_url {
            entry.current_url.clone_from(url);
        }
        set(&mut entry.auto_loop, self.auto_loop);
        set(&mut entry.first_click_interact, self.first_click_interact);
        set(&mut entry.auto_zoom, self.auto_zoom);
        set(&mut entry.auto_play, self.auto_play);
        set(&mut entry.auto_scale, self.auto_scale);
        set(&mut entry.width_pixels, self.width_pixels);
        set(&mut entry.height_pixels, self.height_pixels);
        set(&mut entry.controls, self.controls);
        set(&mut entry.whitelist_enable, self.whitelist_enable);
        if let Some(whitelist) = &self.whitelist {
            entry.whitelist.clone_from(whitelist);
        }
        entry.perms_interact = self.perms_interact.apply(entry.perms_interact);
        entry.perms_control = self.perms_control.apply(entry.perms_control);
        Some(entry)
    }
}

/// Overwrite `slot` with `value` when there is one.
fn set<T>(slot: &mut T, value: Option<T>) {
    if let Some(value) = value {
        *slot = value;
    }
}

/// The values a Media Settings form holds, read off its widgets — the input to
/// [`form_edit`].
#[derive(Debug, Clone, PartialEq, Eq, Default)]
#[expect(
    clippy::struct_excessive_bools,
    reason = "one field per checkbox of the reference's form, each an independent switch"
)]
pub(crate) struct MediaForm {
    /// The home-page field's text.
    pub(crate) home_url: String,
    /// Loop playback.
    pub(crate) auto_loop: bool,
    /// The first click interacts.
    pub(crate) first_click_interact: bool,
    /// Zoom on click.
    pub(crate) auto_zoom: bool,
    /// Play without a click.
    pub(crate) auto_play: bool,
    /// Scale to the face.
    pub(crate) auto_scale: bool,
    /// The width field's text.
    pub(crate) width_pixels: String,
    /// The height field's text.
    pub(crate) height_pixels: String,
    /// The control style.
    pub(crate) controls: i32,
    /// The interact boxes, in [`MEDIA_PERM_BITS`] order.
    pub(crate) perms_interact: [bool; 3],
    /// The show-controls boxes, in [`MEDIA_PERM_BITS`] order.
    pub(crate) perms_control: [bool; 3],
    /// Enforce the white-list.
    pub(crate) whitelist_enable: bool,
    /// The white-list.
    pub(crate) whitelist: Vec<String>,
}

impl MediaForm {
    /// The form a selection opens as: every field showing the selection's value,
    /// and a mixed home page as `multiple` (the reference's *Multiple Media*
    /// text, which an Apply leaves alone while it still reads that way).
    pub(crate) fn from_view(view: &MediaSelectionView, multiple: &str) -> Self {
        Self {
            home_url: if view.home_url.mixed {
                multiple.to_owned()
            } else {
                view.home_url.value.clone()
            },
            auto_loop: view.auto_loop.value,
            first_click_interact: view.first_click_interact.value,
            auto_zoom: view.auto_zoom.value,
            auto_play: view.auto_play.value,
            auto_scale: view.auto_scale.value,
            width_pixels: view.width_pixels.value.to_string(),
            height_pixels: view.height_pixels.value.to_string(),
            controls: view.controls.value,
            perms_interact: view.perms_interact.clone().map(|bit| bit.value),
            perms_control: view.perms_control.clone().map(|bit| bit.value),
            whitelist_enable: view.whitelist_enable.value,
            whitelist: view.whitelist.value.clone(),
        }
    }
}

/// Why a form cannot be applied.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum FormError {
    /// The home page is not a URL, even with a scheme supplied.
    HomeUrl,
    /// A size field is not a whole number from 0 to [`MAX_MEDIA_PIXELS`].
    Size,
}

/// The URL a home-page (or white-list test) text names — the reference's
/// `makeValidUrl`, which supplies `https://` to a bare `example.com` — or
/// `None` for an empty field.
///
/// # Errors
/// [`FormError::HomeUrl`] when the text is not a URL even then.
pub(crate) fn parse_media_url(text: &str) -> Result<Option<url::Url>, FormError> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }
    let candidate = if trimmed.contains("://") {
        trimmed.to_owned()
    } else {
        format!("https://{trimmed}")
    };
    url::Url::parse(&candidate)
        .map(Some)
        .map_err(|_error| FormError::HomeUrl)
}

/// A size field's value.
///
/// # Errors
/// [`FormError::Size`] when it is not a whole number in range.
fn parse_size(text: &str) -> Result<i32, FormError> {
    text.trim()
        .parse::<i32>()
        .ok()
        .filter(|value| (0..=MAX_MEDIA_PIXELS).contains(value))
        .ok_or(FormError::Size)
}

/// The per-bit permission edit a row of boxes makes against what was shown.
fn perm_edit(shown: &[Shown<bool>; 3], form: [bool; 3]) -> PermEdit {
    let mut edit = PermEdit::default();
    for ((bit, shown), value) in MEDIA_PERM_BITS.iter().zip(shown).zip(form) {
        if let Some(value) = shown.edited(&value) {
            edit.mask |= bit;
            if value {
                edit.bits |= bit;
            }
        }
    }
    edit
}

/// The edit an Apply of `form` makes to a selection shown as `view` (see the
/// module documentation for which fields it carries). `multiple` is the text a
/// mixed home page shows, which is left alone while the field still reads that
/// way.
///
/// # Errors
/// A [`FormError`] naming the field that does not parse.
pub(crate) fn form_edit(
    view: &MediaSelectionView,
    form: &MediaForm,
    multiple: &str,
) -> Result<MediaEdit, FormError> {
    let home_untouched = view.home_url.mixed && form.home_url == multiple;
    let home_url = if home_untouched {
        UrlEdit::Keep
    } else {
        UrlEdit::Write(parse_media_url(&form.home_url)?)
    };
    let width = parse_size(&form.width_pixels)?;
    let height = parse_size(&form.height_pixels)?;
    Ok(MediaEdit {
        home_url,
        current_url: UrlEdit::Keep,
        auto_loop: view.auto_loop.edited(&form.auto_loop),
        first_click_interact: view.first_click_interact.edited(&form.first_click_interact),
        auto_zoom: view.auto_zoom.edited(&form.auto_zoom),
        auto_play: view.auto_play.edited(&form.auto_play),
        auto_scale: view.auto_scale.edited(&form.auto_scale),
        width_pixels: view.width_pixels.edited(&width),
        height_pixels: view.height_pixels.edited(&height),
        controls: view.controls.edited(&form.controls),
        perms_interact: perm_edit(&view.perms_interact, form.perms_interact),
        perms_control: perm_edit(&view.perms_control, form.perms_control),
        whitelist_enable: view.whitelist_enable.edited(&form.whitelist_enable),
        whitelist: view.whitelist.edited(&form.whitelist),
    })
}

/// Whether the home page passes a white-list — the reference's
/// `urlPassesWhiteList`, which also passes an empty home page and an empty list.
/// A home page that does not parse passes too: the General tab reports that
/// failure itself.
pub(crate) fn home_passes_whitelist(home_url: &str, whitelist: &[String]) -> bool {
    match parse_media_url(home_url) {
        Ok(Some(url)) => MediaEntry::url_passes_whitelist(&url, whitelist),
        Ok(None) | Err(_) => true,
    }
}

/// One object's media after an edit of some of its faces: every face's entry
/// (the object's others unchanged — the capability's update names every face),
/// and the faces that now carry media, whose texture-entry flag the edit must
/// set.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) struct ObjectMediaEdit {
    /// One slot per face, in order.
    pub(crate) faces: Vec<Option<MediaEntry>>,
    /// The selected faces the edit gave (or kept) media on.
    pub(crate) flagged: Vec<usize>,
}

/// Apply `edit` to the `selected` faces of an object whose faces currently
/// carry `existing` — the reference's `selectionSetMedia(MF_HAS_MEDIA, …)` for
/// one object.
pub(crate) fn edit_object_media(
    existing: &[Option<MediaEntry>],
    selected: &[usize],
    edit: &MediaEdit,
) -> ObjectMediaEdit {
    let mut faces = existing.to_vec();
    let mut flagged = Vec::new();
    for &index in selected {
        let Some(slot) = faces.get_mut(index) else {
            continue;
        };
        if let Some(entry) = edit.apply(slot.as_ref()) {
            *slot = Some(entry);
            flagged.push(index);
        }
    }
    ObjectMediaEdit { faces, flagged }
}

/// Remove the media from the `selected` faces of an object whose faces carry
/// `existing` — the reference's `selectionSetMedia(0, …)` for one object.
/// Returns every face's entry after the removal, or `None` when no face keeps
/// media: then there is nothing left to send over the capability, because the
/// texture-entry update that clears the flags already removes it all.
pub(crate) fn remove_object_media(
    existing: &[Option<MediaEntry>],
    selected: &[usize],
) -> Option<Vec<Option<MediaEntry>>> {
    let mut faces = existing.to_vec();
    for &index in selected {
        if let Some(slot) = faces.get_mut(index) {
            *slot = None;
        }
    }
    faces.iter().any(Option::is_some).then_some(faces)
}

/// The texture scale and offset that fit a media surface of `media` pixels
/// drawn into a texture of `texture` pixels onto the face — the reference's
/// Align (`LLPanelFaceSetMediaFunctor`): the repeats are the used fraction of
/// the texture on each axis, and the offset centres that fraction. `None` for
/// a surface with no size yet ("must load first").
pub(crate) fn media_alignment(media: (u32, u32), texture: (u32, u32)) -> Option<[f32; 4]> {
    let fraction = |used: u32, total: u32| {
        let used = u16::try_from(used).ok().filter(|used| *used > 0)?;
        let total = u16::try_from(total).ok().filter(|total| *total > 0)?;
        Some(f32::from(used) / f32::from(total))
    };
    let scale_s = fraction(media.0, texture.0)?;
    let scale_t = fraction(media.1, texture.1)?;
    // `(scale - 1) / 2`, the reference's `-(1 - scale) / 2` written so a
    // surface that fills its texture gets an offset of `0.0`, not `-0.0`.
    Some([
        scale_s,
        scale_t,
        (scale_s - 1.0) / 2.0,
        (scale_t - 1.0) / 2.0,
    ])
}

#[cfg(test)]
mod tests {
    use super::{
        FaceMedia, FormError, MediaEdit, MediaForm, MediaSummary, PermEdit, UrlEdit,
        edit_object_media, form_edit, gather_media, home_passes_whitelist, media_alignment,
        parse_media_url, remove_object_media,
    };
    use pretty_assertions::assert_eq;
    use sl_client_bevy::{
        MEDIA_PERM_ALL, MEDIA_PERM_ANYONE, MEDIA_PERM_GROUP, MEDIA_PERM_OWNER, MediaEntry,
    };

    /// The text a mixed home page shows in these tests.
    const MULTIPLE: &str = "Multiple Media";

    /// A media entry with `home` as its home page.
    fn entry(home: &str) -> MediaEntry {
        MediaEntry {
            home_url: url::Url::parse(home).ok(),
            ..MediaEntry::default()
        }
    }

    /// A face that carries `entry`.
    const fn flagged(entry: &MediaEntry) -> FaceMedia<'_> {
        FaceMedia {
            flagged: true,
            entry: Some(entry),
        }
    }

    /// A face without media.
    const BARE: FaceMedia<'static> = FaceMedia {
        flagged: false,
        entry: None,
    };

    /// One media configuration across the selection is named by its home page;
    /// two are *Multiple Media*; none is nothing — and a face without media
    /// never counts as a second configuration.
    #[test]
    fn the_summary_names_one_configuration_or_says_there_are_several() {
        let a = entry("https://a.example/");
        let b = entry("https://b.example/");
        assert_eq!(gather_media(&[BARE, BARE]).summary, MediaSummary::None);
        assert_eq!(
            gather_media(&[flagged(&a), BARE]).summary,
            MediaSummary::Single("https://a.example/".to_owned())
        );
        assert_eq!(
            gather_media(&[flagged(&a), flagged(&a)]).summary,
            MediaSummary::Single("https://a.example/".to_owned())
        );
        assert_eq!(
            gather_media(&[flagged(&a), flagged(&b)]).summary,
            MediaSummary::Multiple
        );
    }

    /// An entry the capability still reports for a face whose texture entry no
    /// longer carries the media flag is not media the face has.
    #[test]
    fn an_entry_without_the_flag_is_not_media() {
        let stale = entry("https://stale.example/");
        let face = FaceMedia {
            flagged: false,
            entry: Some(&stale),
        };
        let view = gather_media(&[face]);
        assert!(!view.any_media);
        assert_eq!(view.summary, MediaSummary::None);
        assert_eq!(view.home_url.value, String::new());
    }

    /// A media face and a bare face disagree on every field the media face set:
    /// the bare face reads as the default entry, not as "no opinion".
    #[test]
    fn a_bare_face_reads_as_the_default_entry() {
        let loud = MediaEntry {
            auto_play: true,
            ..entry("https://a.example/")
        };
        let view = gather_media(&[flagged(&loud), BARE]);
        assert!(view.auto_play.mixed);
        assert!(view.home_url.mixed);
        assert!(!view.auto_loop.mixed, "both faces leave auto-loop off");
    }

    /// An Apply of an untouched form over a mixed selection writes only what
    /// the faces agreed on: the mixed fields — the home page showing
    /// *Multiple Media* included — are left to each face.
    #[test]
    fn an_untouched_mixed_field_is_left_alone() -> Result<(), FormError> {
        let a = MediaEntry {
            auto_play: true,
            ..entry("https://a.example/")
        };
        let b = entry("https://b.example/");
        let view = gather_media(&[flagged(&a), flagged(&b)]);
        let form = MediaForm::from_view(&view, MULTIPLE);
        let edit = form_edit(&view, &form, MULTIPLE)?;
        assert_eq!(edit.home_url, UrlEdit::Keep);
        assert_eq!(edit.auto_play, None);
        assert_eq!(edit.auto_loop, Some(false));
        Ok(())
    }

    /// Changing a mixed field writes it to every face.
    #[test]
    fn a_changed_mixed_field_is_written() -> Result<(), FormError> {
        let a = MediaEntry {
            auto_play: true,
            ..entry("https://a.example/")
        };
        let b = entry("https://b.example/");
        let view = gather_media(&[flagged(&a), flagged(&b)]);
        let form = MediaForm {
            auto_play: false,
            home_url: "c.example".to_owned(),
            ..MediaForm::from_view(&view, MULTIPLE)
        };
        let edit = form_edit(&view, &form, MULTIPLE)?;
        assert_eq!(
            edit.home_url,
            UrlEdit::Write(url::Url::parse("https://c.example/").ok())
        );
        // `false` is what the first face did not show, so it counts as changed.
        assert_eq!(edit.auto_play, Some(false));
        Ok(())
    }

    /// A face without media gains it only from an edit that sets the home page;
    /// then it starts from the default entry, merged with the edit.
    #[test]
    fn a_bare_face_gains_media_only_from_a_home_page() {
        let no_home = MediaEdit {
            auto_play: Some(true),
            ..MediaEdit::default()
        };
        assert_eq!(no_home.apply(None), None);

        let with_home = MediaEdit {
            home_url: UrlEdit::Write(url::Url::parse("https://a.example/").ok()),
            auto_play: Some(true),
            ..MediaEdit::default()
        };
        assert_eq!(
            with_home.apply(None),
            Some(MediaEntry {
                auto_play: true,
                ..entry("https://a.example/")
            })
        );
    }

    /// Permissions merge per bit: an untouched mixed bit keeps each face's own
    /// value while a changed bit is written everywhere.
    #[test]
    fn permissions_merge_per_bit() -> Result<(), FormError> {
        let owner_only = MediaEntry {
            perms_interact: MEDIA_PERM_OWNER,
            ..entry("https://a.example/")
        };
        let everyone = MediaEntry {
            perms_interact: MEDIA_PERM_ALL,
            ..entry("https://a.example/")
        };
        let view = gather_media(&[flagged(&owner_only), flagged(&everyone)]);
        assert!(!view.perms_interact[0].mixed, "both let the owner interact");
        assert!(view.perms_interact[1].mixed);
        // The user takes the owner's right away and leaves group / anyone mixed.
        let mut form = MediaForm::from_view(&view, MULTIPLE);
        form.perms_interact[0] = false;
        let edit = form_edit(&view, &form, MULTIPLE)?;
        assert_eq!(
            edit.perms_interact,
            PermEdit {
                mask: MEDIA_PERM_OWNER,
                bits: 0,
            }
        );
        assert_eq!(
            edit.apply(Some(&owner_only))
                .map(|face| face.perms_interact),
            Some(0)
        );
        assert_eq!(
            edit.apply(Some(&everyone)).map(|face| face.perms_interact),
            Some(MEDIA_PERM_GROUP | MEDIA_PERM_ANYONE)
        );
        Ok(())
    }

    /// A bare `example.com` home page is given the reference's default scheme;
    /// an empty field clears the home page; junk is refused.
    #[test]
    fn a_home_page_is_given_a_scheme() {
        assert_eq!(
            parse_media_url("example.com/page"),
            Ok(url::Url::parse("https://example.com/page").ok())
        );
        assert_eq!(parse_media_url("  "), Ok(None));
        assert_eq!(parse_media_url("http://[::1"), Err(FormError::HomeUrl));
    }

    /// A size outside 0–2048, or not a number, is refused rather than clamped.
    #[test]
    fn a_bad_size_is_refused() {
        let view = gather_media(&[]);
        let form = MediaForm {
            width_pixels: "4096".to_owned(),
            ..MediaForm::from_view(&view, MULTIPLE)
        };
        assert_eq!(form_edit(&view, &form, MULTIPLE), Err(FormError::Size));
        let form = MediaForm {
            height_pixels: "tall".to_owned(),
            ..MediaForm::from_view(&view, MULTIPLE)
        };
        assert_eq!(form_edit(&view, &form, MULTIPLE), Err(FormError::Size));
    }

    /// An edit of one face re-sends the object's other faces untouched, and
    /// names the faces that now carry media.
    #[test]
    fn an_edit_keeps_the_other_faces() {
        let other = entry("https://other.example/");
        let existing = vec![None, Some(other.clone()), None];
        let edit = MediaEdit {
            home_url: UrlEdit::Write(url::Url::parse("https://new.example/").ok()),
            ..MediaEdit::default()
        };
        let result = edit_object_media(&existing, &[0, 7], &edit);
        assert_eq!(result.flagged, vec![0]);
        assert_eq!(
            result.faces,
            vec![Some(entry("https://new.example/")), Some(other), None]
        );
    }

    /// Removing media from some faces keeps the rest; removing the last face's
    /// leaves nothing to send.
    #[test]
    fn removal_keeps_the_rest_or_sends_nothing() {
        let a = entry("https://a.example/");
        let existing = vec![Some(a.clone()), Some(a.clone())];
        assert_eq!(
            remove_object_media(&existing, &[0]),
            Some(vec![None, Some(a)])
        );
        assert_eq!(remove_object_media(&existing, &[0, 1]), None);
    }

    /// The white-list check passes an empty home page and an empty list, and
    /// otherwise follows the entry's own matcher.
    #[test]
    fn the_home_page_is_checked_against_the_whitelist() {
        let list = vec!["*.example.com".to_owned()];
        assert!(home_passes_whitelist("", &list));
        assert!(home_passes_whitelist("https://nowhere.test/", &[]));
        assert!(home_passes_whitelist("www.example.com", &list));
        assert!(!home_passes_whitelist("https://nowhere.test/", &list));
    }

    /// A surface drawn into a texture exactly its size fills the face; one
    /// padded into a larger texture is scaled to the used part and centred.
    #[test]
    fn alignment_fits_the_used_part_of_the_texture() {
        assert_eq!(
            media_alignment((512, 512), (512, 512)).map(|values| values.map(f32::to_bits)),
            Some([1.0_f32, 1.0, 0.0, 0.0].map(f32::to_bits))
        );
        assert_eq!(
            media_alignment((1024, 768), (1024, 1024)).map(|values| values.map(f32::to_bits)),
            Some([1.0_f32, 0.75, 0.0, -0.125].map(f32::to_bits))
        );
        assert_eq!(media_alignment((0, 0), (512, 512)), None);
    }
}
