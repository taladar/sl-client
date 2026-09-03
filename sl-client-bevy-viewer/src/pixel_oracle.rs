//! The **pixel oracles** of the render harness: pure functions over a captured
//! frame that decide things a machine can decide about it — where a known colour
//! landed, how much of a silhouette an object covers, whether two frames differ —
//! shared by every tier that reads pixels back (`crate::render_readback` today,
//! the fixture-world and full-stack rigs as they land).
//!
//! # Not golden images
//!
//! Nothing here compares a frame against a reference frame. Pixel-exact
//! comparison across drivers turns a suite into a driver-version detector, and a
//! suite that fails on a Mesa upgrade is one that gets disabled. What is decided
//! instead is *classifiable*: a pixel's dominant channel, a patch's majority, the
//! centroid of one colour inside one object's disc. Every scene that wants to be
//! measured this way paints its subject and its actors in near-primary
//! [`Marker`] colours, so the question "is the yellow one where the yellow one
//! should be" has an answer no driver difference changes.
//!
//! # Teeth
//!
//! Every oracle has a test below that draws a synthetic frame by hand and shows
//! the oracle firing on the bad case and staying silent on the good one. An
//! oracle that cannot be shown to bite is decoration.

use bevy::prelude::*;

/// How saturated a pixel must be to count as *one of the marker colours* rather
/// than the grey backdrop, the sea, the sky, or a specular highlight.
///
/// Markers are deliberately near-primary (0.9 in their channel(s), 0.1 in the
/// others), so a real one is unambiguous. This only has to exclude grey — it is
/// nowhere near having to *discriminate* between the four, which
/// [`dominant`]'s ordering does.
pub(crate) const SATURATION: f32 = 0.06;

/// How bright a channel must be for a patch to count as carrying it.
///
/// The subject and the actors are emissive and near-primary, so this only has to
/// clear whatever is neither — the sea and the sky, which are dark and neutral.
pub(crate) const CHANNEL_PRESENT: f32 = 0.20;

/// The half-width of a sampled [`Patch`], in pixels: a 3×3 patch around the
/// projected point, of which [`PATCH_MAJORITY`] must agree.
pub(crate) const PATCH_RADIUS: i32 = 1;

/// How many of a patch's pixels must carry a channel for the patch to count as
/// carrying it. A majority, so one stray pixel on a band's edge — a projected
/// point can land a pixel off at a grazing angle — decides nothing.
pub(crate) const PATCH_MAJORITY: u32 = 5;

/// The fewest pixels of a colour inside a silhouette before [`centroid`] reports
/// one: below this a centroid is rounding, not a position.
const CENTROID_MIN_PIXELS: u32 = 4;

/// How far, per channel in `0..=1`, two pixels must differ before
/// [`differing_pixels`] counts them as different. Eight steps of an 8-bit target:
/// quantisation cannot flicker that far without a real change behind it.
const DIFFER_STEP: f32 = 8.0 / 255.0;

/// A frame read back from the GPU.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Frame {
    /// Row-major `Rgba8` pixels, `width * height * 4` bytes, as stored in the
    /// render target — an sRGB-encoded 8-bit surface, so these are display
    /// values. Every oracle here is monotone per channel and does not care.
    pixels: Vec<u8>,
    /// Width in pixels.
    width: u32,
    /// Height in pixels.
    height: u32,
}

impl Frame {
    /// Wrap a read-back buffer. Returns `None` if the byte count does not match
    /// `width * height * 4`, which means the readback and the target disagree
    /// about the frame and nothing sampled from it would mean anything.
    pub(crate) fn from_rgba8(pixels: Vec<u8>, width: u32, height: u32) -> Option<Self> {
        let expected = usize::try_from(width)
            .ok()?
            .checked_mul(usize::try_from(height).ok()?)?
            .checked_mul(4)?;
        (pixels.len() == expected).then_some(Self {
            pixels,
            width,
            height,
        })
    }

    /// The frame's size, in pixels.
    pub(crate) fn size(&self) -> UVec2 {
        UVec2::new(self.width, self.height)
    }

    /// The raw bytes, for a whole-frame comparison.
    pub(crate) fn bytes(&self) -> &[u8] {
        &self.pixels
    }

    /// The pixel at `(x, y)` as `(r, g, b, a)` in `0..=1`, or `None` if the
    /// coordinate is outside the frame.
    pub(crate) fn pixel(&self, x: u32, y: u32) -> Option<Vec4> {
        if x >= self.width || y >= self.height {
            return None;
        }
        let index = usize::try_from(y)
            .ok()?
            .checked_mul(usize::try_from(self.width).ok()?)?
            .checked_add(usize::try_from(x).ok()?)?
            .checked_mul(4)?;
        let texel = self.pixels.get(index..index.checked_add(4)?)?;
        match texel {
            [r, g, b, a] => Some(Vec4::new(
                f32::from(*r) / 255.0,
                f32::from(*g) / 255.0,
                f32::from(*b) / 255.0,
                f32::from(*a) / 255.0,
            )),
            _other => None,
        }
    }

    /// Every pixel coordinate of the frame, row-major.
    fn coordinates(&self) -> impl Iterator<Item = (u32, u32)> + '_ {
        (0..self.height).flat_map(move |y| (0..self.width).map(move |x| (x, y)))
    }
}

/// The near-primary colours a scene paints its subject and actors in, so an
/// oracle can tell them apart by channel alone.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum Marker {
    /// `(0.9, 0.1, 0.1)`.
    Red,
    /// `(0.1, 0.9, 0.1)`.
    Green,
    /// `(0.1, 0.1, 0.9)`.
    Blue,
    /// `(0.9, 0.9, 0.1)` — red *and* green, so it is tested before either.
    Yellow,
}

impl Marker {
    /// The marker's name, for a failure message that says "the red one" rather
    /// than quoting a float triple nobody can picture.
    pub(crate) const fn name(self) -> &'static str {
        match self {
            Self::Red => "red",
            Self::Green => "green",
            Self::Blue => "blue",
            Self::Yellow => "yellow",
        }
    }

    /// Whether `pixel` carries this marker's channel(s) above
    /// [`CHANNEL_PRESENT`] — a looser test than [`dominant`], for a translucent
    /// face through which *two* markers show at once.
    pub(crate) fn present_in(self, pixel: Vec4) -> bool {
        match self {
            Self::Red => pixel.x > CHANNEL_PRESENT,
            Self::Green => pixel.y > CHANNEL_PRESENT,
            Self::Blue => pixel.z > CHANNEL_PRESENT,
            Self::Yellow => pixel.x > CHANNEL_PRESENT && pixel.y > CHANNEL_PRESENT,
        }
    }
}

/// Which marker dominates a pixel, if any does by [`SATURATION`].
pub(crate) fn dominant(pixel: Vec4) -> Option<Marker> {
    let (r, g, b) = (pixel.x, pixel.y, pixel.z);
    // Yellow is red+green, so it must be tested before either of them.
    if r > b + SATURATION && g > b + SATURATION && (r - g).abs() < SATURATION {
        return Some(Marker::Yellow);
    }
    if r > g + SATURATION && r > b + SATURATION {
        return Some(Marker::Red);
    }
    if g > r + SATURATION && g > b + SATURATION {
        return Some(Marker::Green);
    }
    if b > r + SATURATION && b > g + SATURATION {
        return Some(Marker::Blue);
    }
    None
}

/// A projected screen coordinate as a pixel index. Saturating, so a point that
/// projected off the frame clamps rather than wrapping into it.
#[expect(
    clippy::as_conversions,
    clippy::cast_possible_truncation,
    reason = "a projected screen coordinate, saturating at the i32 bounds"
)]
fn i32_from_f32(value: f32) -> i32 {
    value as i32
}

/// A pixel index as a screen coordinate. Exact for any frame a test would render.
#[expect(
    clippy::as_conversions,
    clippy::cast_precision_loss,
    reason = "a pixel index of a test frame (at most a few thousand) converts to f32 exactly"
)]
pub(crate) const fn f32_from_u32(value: u32) -> f32 {
    value as f32
}

/// A small square of pixels around a projected point.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Patch {
    /// The projected point, in pixels.
    pub(crate) at: Vec2,
}

impl Patch {
    /// How many of the patch's pixels carry each of `subject` and `other`.
    ///
    /// Returns `None` if the patch's origin cannot be a pixel index (a point that
    /// projected far off the frame).
    fn count(self, frame: &Frame, subject: Marker, other: Marker) -> Option<(u32, u32)> {
        let (mut subject_count, mut other_count) = (0_u32, 0_u32);
        for dy in -PATCH_RADIUS..=PATCH_RADIUS {
            for dx in -PATCH_RADIUS..=PATCH_RADIUS {
                let x = u32::try_from(i32_from_f32(self.at.x).saturating_add(dx)).ok()?;
                let y = u32::try_from(i32_from_f32(self.at.y).saturating_add(dy)).ok()?;
                let Some(pixel) = frame.pixel(x, y) else {
                    continue;
                };
                if subject.present_in(pixel) {
                    subject_count = subject_count.saturating_add(1);
                }
                if other.present_in(pixel) {
                    other_count = other_count.saturating_add(1);
                }
            }
        }
        Some((subject_count, other_count))
    }
}

/// What one sampled cell came back as, given a subject painted in one marker
/// and whatever is behind it painted in another.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CellVerdict {
    /// The subject's colour is there and the other's is too: drawn, and
    /// see-through.
    Translucent,
    /// The subject's colour is there and nothing behind it is: drawn, but
    /// nothing shows through.
    Solid,
    /// Only what is behind the subject: the subject is not on screen here.
    Missing,
    /// Neither colour — the sea, the sky, or a point that missed the subject.
    Background,
}

/// Read one cell: sample a patch around `at` and say what is there, with the
/// subject painted `subject` and what is behind it painted `other`.
///
/// `None` if `at` is `None` (the point did not project) or cannot index the
/// frame.
pub(crate) fn read_cell(
    frame: &Frame,
    at: Option<Vec2>,
    subject: Marker,
    other: Marker,
) -> Option<CellVerdict> {
    let (subject_count, other_count) = Patch { at: at? }.count(frame, subject, other)?;
    Some(
        match (
            subject_count >= PATCH_MAJORITY,
            other_count >= PATCH_MAJORITY,
        ) {
            (true, true) => CellVerdict::Translucent,
            (true, false) => CellVerdict::Solid,
            (false, true) => CellVerdict::Missing,
            (false, false) => CellVerdict::Background,
        },
    )
}

/// An object's projected outline, as the disc it fits in: the region an oracle
/// restricts itself to so it measures *that object's* pixels and not the whole
/// frame's.
///
/// Restricting is the whole check. A coloured actor is *directly visible* in the
/// frame as well as reflected in a mirror, and a centroid over the whole frame is
/// dominated by the actor itself — which does not move when the mirror is wrong.
/// The first mirror check did exactly that and passed with the bug present: it was
/// measuring the cubes, not the mirror.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Silhouette {
    /// The disc's centre, in pixels.
    pub(crate) centre: Vec2,
    /// The disc's radius, in pixels.
    pub(crate) radius: f32,
}

impl Silhouette {
    /// Whether the pixel at `(x, y)` lies inside the disc.
    fn contains(self, x: u32, y: u32) -> bool {
        let point = Vec2::new(f32_from_u32(x), f32_from_u32(y));
        point.distance(self.centre) <= self.radius
    }

    /// Every pixel of `frame` inside the disc, with its coordinate.
    fn pixels<'a>(self, frame: &'a Frame) -> impl Iterator<Item = (Vec2, Vec4)> + 'a {
        frame.coordinates().filter_map(move |(x, y)| {
            if !self.contains(x, y) {
                return None;
            }
            frame
                .pixel(x, y)
                .map(|pixel| (Vec2::new(f32_from_u32(x), f32_from_u32(y)), pixel))
        })
    }
}

/// The centroid, in pixels, of the pixels inside `silhouette` whose dominant
/// marker is `marker` — or `None` if fewer than [`CENTROID_MIN_PIXELS`] are.
pub(crate) fn centroid(frame: &Frame, silhouette: Silhouette, marker: Marker) -> Option<Vec2> {
    let (mut sum, mut count) = (Vec2::ZERO, 0_u32);
    for (point, pixel) in silhouette.pixels(frame) {
        if dominant(pixel) == Some(marker) {
            sum = Vec2::new(sum.x + point.x, sum.y + point.y);
            count = count.saturating_add(1);
        }
    }
    (count >= CENTROID_MIN_PIXELS).then(|| {
        let n = f32_from_u32(count);
        Vec2::new(sum.x / n, sum.y / n)
    })
}

/// The fraction of the pixels inside `silhouette` whose dominant marker is
/// `marker`: `0.0` for an object that drew nothing, near `1.0` for one that
/// fills its own outline. Zero for an empty disc.
pub(crate) fn coverage(frame: &Frame, silhouette: Silhouette, marker: Marker) -> f32 {
    let (mut hits, mut total) = (0_u32, 0_u32);
    for (_point, pixel) in silhouette.pixels(frame) {
        total = total.saturating_add(1);
        if dominant(pixel) == Some(marker) {
            hits = hits.saturating_add(1);
        }
    }
    if total == 0 {
        return 0.0;
    }
    f32_from_u32(hits) / f32_from_u32(total)
}

/// How many pixels differ between two frames of the same size by more than
/// [`DIFFER_STEP`] in any channel — inside `within` if given, else over the
/// whole frame. Zero for frames of different sizes, which cannot be compared
/// pixel for pixel.
pub(crate) fn differing_pixels(a: &Frame, b: &Frame, within: Option<Silhouette>) -> u32 {
    if a.size() != b.size() {
        return 0;
    }
    let mut count = 0_u32;
    for (x, y) in a.coordinates() {
        if within.is_some_and(|silhouette| !silhouette.contains(x, y)) {
            continue;
        }
        let (Some(before), Some(after)) = (a.pixel(x, y), b.pixel(x, y)) else {
            continue;
        };
        let delta = [
            before.x - after.x,
            before.y - after.y,
            before.z - after.z,
            before.w - after.w,
        ];
        if delta.iter().any(|channel| channel.abs() > DIFFER_STEP) {
            count = count.saturating_add(1);
        }
    }
    count
}

/// **Where** two frames differ: the centroid, in pixels, of everything
/// [`differing_pixels`] counts — inside `within` if given, else over the whole
/// frame. `None` when fewer than [`CENTROID_MIN_PIXELS`] changed, which is the
/// same floor [`centroid`] uses and for the same reason.
///
/// The companion an A/B toggle needs. "Turning the feature off changed the
/// picture" is half a claim: a lens flare would satisfy it too. Asking *where*
/// it changed pins the subject down without the test having to know how big the
/// thing is or exactly how its renderer places it — which for a camera-facing
/// billboard whose on-screen size is constant and whose anchor is pulled toward
/// the camera is not something a metre figure can say.
pub(crate) fn changed_centroid(a: &Frame, b: &Frame, within: Option<Silhouette>) -> Option<Vec2> {
    if a.size() != b.size() {
        return None;
    }
    let (mut sum, mut count) = (Vec2::ZERO, 0_u32);
    for (x, y) in a.coordinates() {
        if within.is_some_and(|silhouette| !silhouette.contains(x, y)) {
            continue;
        }
        let (Some(before), Some(after)) = (a.pixel(x, y), b.pixel(x, y)) else {
            continue;
        };
        if pixels_differ(before, after) {
            sum = Vec2::new(sum.x + f32_from_u32(x), sum.y + f32_from_u32(y));
            count = count.saturating_add(1);
        }
    }
    (count >= CENTROID_MIN_PIXELS).then(|| {
        let n = f32_from_u32(count);
        Vec2::new(sum.x / n, sum.y / n)
    })
}

/// Whether two pixels differ by more than [`DIFFER_STEP`] in any channel — the
/// same threshold [`differing_pixels`] counts by, so "these two are different"
/// means the same thing whether it is asked of two pixels or two frames.
pub(crate) fn pixels_differ(a: Vec4, b: Vec4) -> bool {
    [a.x - b.x, a.y - b.y, a.z - b.z, a.w - b.w]
        .iter()
        .any(|channel| channel.abs() > DIFFER_STEP)
}

/// The mean pixel of the frame's rows `from..to` (`to` exclusive), or `None` if
/// the range holds no row of the frame.
///
/// A **horizontal band** is the right shape for a scene laid out by height:
/// looking level across a region, the sky is above the horizon, the sea is
/// between the horizon and the far shore, and the ground is below it. Each of
/// those is a band spanning the whole frame, and reading one as a mean says what
/// the band *is* without asking what colour it should have been.
pub(crate) fn band_mean(frame: &Frame, from: u32, to: u32) -> Option<Vec4> {
    let height = frame.size().y;
    let (from, to) = (from.min(height), to.min(height));
    // Summed component by component rather than as a `Vec4`: the workspace's
    // `arithmetic_side_effects` lint bans the overloaded operator, and the
    // component form is what `corner_background` already uses.
    let mut sum = [0.0_f32; 4];
    let mut count = 0.0_f32;
    for y in from..to {
        for x in 0..frame.size().x {
            if let Some(pixel) = frame.pixel(x, y) {
                sum = [
                    sum[0] + pixel.x,
                    sum[1] + pixel.y,
                    sum[2] + pixel.z,
                    sum[3] + pixel.w,
                ];
                count += 1.0;
            }
        }
    }
    (count > 0.0).then(|| {
        Vec4::new(
            sum[0] / count,
            sum[1] / count,
            sum[2] / count,
            sum[3] / count,
        )
    })
}

/// The **relative luminance** of a pixel — the Rec. 709 weighting of its three
/// colour channels, alpha ignored.
///
/// The one thing a capture can honestly say about a *sky*: it has no marker
/// colour to classify and no silhouette to cover, so what changes when the
/// environment changes is how bright it came out. Weighted rather than a plain
/// mean because the eye's is: a sky that loses its sun loses most of its green,
/// and an unweighted average understates that by a third.
///
/// Alpha is dropped because in the full-stack tier's frames the alpha channel is
/// the glow mask rather than opacity (`crate::full_stack_test`), so a bright
/// glow would otherwise read as a bright sky.
pub(crate) fn luminance(pixel: Vec4) -> f32 {
    0.2126_f32.mul_add(pixel.x, 0.7152_f32.mul_add(pixel.y, 0.0722 * pixel.z))
}

/// The mean of the frame's four corner pixels: what the *background* looks
/// like, sampled rather than assumed, for [`coverage_not_background`]. The
/// corners belong to the subject only if it fills the frame, which the caller's
/// framing already forbids.
pub(crate) fn corner_background(frame: &Frame) -> Vec4 {
    let UVec2 {
        x: width,
        y: height,
    } = frame.size();
    let corners = [
        (0, 0),
        (width.saturating_sub(1), 0),
        (0, height.saturating_sub(1)),
        (width.saturating_sub(1), height.saturating_sub(1)),
    ];
    let mut sum = Vec4::ZERO;
    let mut count = 0.0_f32;
    for (x, y) in corners {
        if let Some(pixel) = frame.pixel(x, y) {
            sum = Vec4::new(
                sum.x + pixel.x,
                sum.y + pixel.y,
                sum.z + pixel.z,
                sum.w + pixel.w,
            );
            count += 1.0;
        }
    }
    if count > 0.0 {
        Vec4::new(sum.x / count, sum.y / count, sum.z / count, sum.w / count)
    } else {
        Vec4::ZERO
    }
}

/// The fraction of the pixels inside `silhouette` that differ from `background`
/// by more than [`DIFFER_STEP`] in some channel — how much of its own outline an
/// object painted, without asking what colour it is. Zero for an empty disc.
pub(crate) fn coverage_not_background(
    frame: &Frame,
    silhouette: Silhouette,
    background: Vec4,
) -> f32 {
    let (mut hits, mut total) = (0_u32, 0_u32);
    for (_point, pixel) in silhouette.pixels(frame) {
        total = total.saturating_add(1);
        let delta = [
            pixel.x - background.x,
            pixel.y - background.y,
            pixel.z - background.z,
        ];
        if delta.iter().any(|channel| channel.abs() > DIFFER_STEP) {
            hits = hits.saturating_add(1);
        }
    }
    if total == 0 {
        return 0.0;
    }
    f32_from_u32(hits) / f32_from_u32(total)
}

/// The two failures that actually happen to a whole frame, decided without a
/// reference image.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct FrameHealth {
    /// Every pixel is black: the render drew nothing, or drew it before a
    /// pipeline compiled.
    pub(crate) all_black: bool,
    /// Every pixel is fully transparent: the target was never written.
    pub(crate) all_transparent: bool,
}

/// Decide [`FrameHealth`] for `frame`.
pub(crate) fn health(frame: &Frame) -> FrameHealth {
    let mut all_black = true;
    let mut all_transparent = true;
    for (x, y) in frame.coordinates() {
        let Some(pixel) = frame.pixel(x, y) else {
            continue;
        };
        if pixel.xyz().max_element() > 0.0 {
            all_black = false;
        }
        if pixel.w > 0.0 {
            all_transparent = false;
        }
        if !all_black && !all_transparent {
            break;
        }
    }
    FrameHealth {
        all_black,
        all_transparent,
    }
}

#[cfg(test)]
mod tests {
    use super::{
        CellVerdict, Frame, FrameHealth, Marker, Silhouette, centroid, coverage, differing_pixels,
        dominant, health, read_cell,
    };

    use crate::render_test::TestError;
    use bevy::prelude::*;
    use pretty_assertions::assert_eq;

    /// A blank buffer of `size`×`size`, fully opaque black, painted by the tests.
    fn blank(size: u32) -> Vec<u8> {
        let mut pixels = Vec::new();
        for _pixel in 0..size.saturating_mul(size) {
            pixels.extend_from_slice(&[0, 0, 0, 255]);
        }
        pixels
    }

    /// Paint the pixel at `(x, y)` of a `size`-wide buffer.
    fn paint(pixels: &mut [u8], size: u32, x: u32, y: u32, rgba: [u8; 4]) {
        let index = usize::try_from(y.saturating_mul(size).saturating_add(x))
            .unwrap_or(0)
            .saturating_mul(4);
        if let Some(texel) = pixels.get_mut(index..index.saturating_add(4)) {
            texel.copy_from_slice(&rgba);
        }
    }

    /// Wrap a hand-painted square buffer, or say that a fixture was wrongly sized.
    fn frame(pixels: Vec<u8>, size: u32) -> Result<Frame, TestError> {
        Frame::from_rgba8(pixels, size, size).ok_or_else(|| "a well-sized fixture".into())
    }

    /// A marker's pixel as the oracle sees it.
    fn as_vec(rgba: [u8; 4]) -> Vec4 {
        let [r, g, b, a] = rgba;
        Vec4::new(
            f32::from(r) / 255.0,
            f32::from(g) / 255.0,
            f32::from(b) / 255.0,
            f32::from(a) / 255.0,
        )
    }

    const RED: [u8; 4] = [230, 25, 25, 255];
    const GREEN: [u8; 4] = [25, 230, 25, 255];
    const BLUE: [u8; 4] = [25, 25, 230, 255];
    const YELLOW: [u8; 4] = [230, 230, 25, 255];
    /// Red *and* green above the presence threshold — what a green face over a
    /// red wall blends to. To [`dominant`] that mix reads as *yellow*, which is
    /// why a cell verdict is decided by channel presence and not by dominance.
    const RED_THROUGH_GREEN: [u8; 4] = [135, 135, 25, 255];
    /// The backdrop: dark and neutral, below [`super::CHANNEL_PRESENT`] in every
    /// channel — as the sea and the sky are, which is what that threshold is for.
    const GREY: [u8; 4] = [40, 40, 40, 255];

    /// The fixture frame's side, in pixels.
    const SIDE: u32 = 32;
    /// The green disc: centred in the frame, radius 8.
    const DISC: Silhouette = Silhouette {
        centre: Vec2::new(16.0, 16.0),
        radius: 8.0,
    };

    /// A 32×32 frame: a green disc of radius 8 centred at (16, 16), and a red
    /// strip across rows 9–11 that crosses the disc's top. The shared fixture for
    /// the cell and silhouette teeth.
    fn disc_under_strip() -> Result<Frame, TestError> {
        let mut pixels = blank(SIDE);
        for y in 0..SIDE {
            for x in 0..SIDE {
                let inside = DISC.contains(x, y);
                let on_strip = (9..=11).contains(&y);
                let rgba = match (inside, on_strip) {
                    (true, true) => RED_THROUGH_GREEN,
                    (true, false) => GREEN,
                    (false, true) => RED,
                    (false, false) => GREY,
                };
                paint(&mut pixels, SIDE, x, y, rgba);
            }
        }
        frame(pixels, SIDE)
    }

    #[test]
    fn a_mismatched_buffer_is_refused() {
        assert!(Frame::from_rgba8(vec![0; 15], 2, 2).is_none());
        assert!(Frame::from_rgba8(vec![0; 16], 2, 2).is_some());
    }

    #[test]
    fn each_marker_dominates_its_own_pixel_and_grey_dominates_nothing() {
        assert_eq!(dominant(as_vec(RED)), Some(Marker::Red));
        assert_eq!(dominant(as_vec(GREEN)), Some(Marker::Green));
        assert_eq!(dominant(as_vec(BLUE)), Some(Marker::Blue));
        assert_eq!(dominant(as_vec(YELLOW)), Some(Marker::Yellow));
        assert_eq!(dominant(as_vec(GREY)), None);
        // A blend carries both channels; by dominance alone it is the mix.
        assert_eq!(dominant(as_vec(RED_THROUGH_GREEN)), Some(Marker::Yellow));
        assert!(Marker::Red.present_in(as_vec(RED_THROUGH_GREEN)));
        assert!(Marker::Green.present_in(as_vec(RED_THROUGH_GREEN)));
        assert!(!Marker::Blue.present_in(as_vec(RED_THROUGH_GREEN)));
    }

    /// The four verdicts, each read from the part of the fixture that means it.
    #[test]
    fn a_cell_reads_translucent_solid_missing_or_background_where_each_is() -> Result<(), TestError>
    {
        let frame = disc_under_strip()?;
        let read =
            |x: f32, y: f32| read_cell(&frame, Some(Vec2::new(x, y)), Marker::Green, Marker::Red);
        // The strip crossing the disc: green face, red showing through.
        assert_eq!(read(16.0, 10.0), Some(CellVerdict::Translucent));
        // The middle of the disc: green, nothing behind.
        assert_eq!(read(16.0, 16.0), Some(CellVerdict::Solid));
        // The strip clear of the disc: only what is behind.
        assert_eq!(read(3.0, 10.0), Some(CellVerdict::Missing));
        // Grey corner: neither.
        assert_eq!(read(28.0, 28.0), Some(CellVerdict::Background));
        // A point that did not project reads nothing.
        assert_eq!(read_cell(&frame, None, Marker::Green, Marker::Red), None);
        Ok(())
    }

    /// One stray pixel does not decide a patch.
    #[test]
    fn a_single_stray_pixel_does_not_flip_a_cell() -> Result<(), TestError> {
        const SIZE: u32 = 8;
        let mut pixels = blank(SIZE);
        paint(&mut pixels, SIZE, 4, 4, GREEN);
        let frame = frame(pixels, SIZE)?;
        assert_eq!(
            read_cell(
                &frame,
                Some(Vec2::new(4.0, 4.0)),
                Marker::Green,
                Marker::Red
            ),
            Some(CellVerdict::Background)
        );
        Ok(())
    }

    #[test]
    fn a_silhouette_measures_its_own_disc_and_not_the_frame() -> Result<(), TestError> {
        let frame = disc_under_strip()?;
        // Most of the disc is green (the strip takes three rows of it).
        let green = coverage(&frame, DISC, Marker::Green);
        assert!(green > 0.8, "green covers {green} of its own disc");
        // Red dominates nowhere inside the disc — the overlap is a blend — even
        // though the strip is plainly red elsewhere in the frame.
        assert!(
            coverage(&frame, DISC, Marker::Red) < 1e-6,
            "no pixel of the disc is red-dominant"
        );
        assert_eq!(centroid(&frame, DISC, Marker::Red), None);
        // The green centroid sits a little below the centre: the strip ate the top.
        let green_at = centroid(&frame, DISC, Marker::Green).ok_or("green is in the disc")?;
        assert!(
            (green_at.x - 16.0).abs() < 0.5,
            "centred horizontally at {green_at}"
        );
        assert!(green_at.y > 16.0, "pushed down by the strip, at {green_at}");
        // Outside its disc a silhouette sees nothing: the same frame, an empty corner.
        let corner = Silhouette {
            centre: Vec2::new(28.0, 28.0),
            radius: 2.0,
        };
        assert!(
            coverage(&frame, corner, Marker::Green) < 1e-6,
            "the corner holds no green"
        );
        Ok(())
    }

    #[test]
    fn a_changed_frame_differs_only_where_it_changed() -> Result<(), TestError> {
        let before = disc_under_strip()?;
        let mut pixels = before.bytes().to_vec();
        // Repaint the grey corner blue: 4 pixels.
        for (x, y) in [(28, 28), (29, 28), (28, 29), (29, 29)] {
            paint(&mut pixels, SIDE, x, y, BLUE);
        }
        let after = frame(pixels, SIDE)?;
        assert_eq!(differing_pixels(&before, &after, None), 4);
        // Inside the disc nothing changed.
        assert_eq!(differing_pixels(&before, &after, Some(DISC)), 0);
        // Identical frames differ nowhere, and quantisation noise is below the step.
        assert_eq!(differing_pixels(&before, &before, None), 0);
        let mut noisy = before.bytes().to_vec();
        paint(&mut noisy, SIDE, 0, 0, [42, 41, 40, 255]);
        let noisy = frame(noisy, SIDE)?;
        assert_eq!(differing_pixels(&before, &noisy, None), 0);
        // Frames of different sizes cannot be compared and say so with zero.
        let tiny = frame(blank(2), 2)?;
        assert_eq!(differing_pixels(&before, &tiny, None), 0);
        Ok(())
    }

    /// The background-aware coverage sees the disc against the sampled corner
    /// colour, and an empty disc as nothing.
    #[test]
    fn coverage_not_background_measures_paint_and_not_the_backdrop() -> Result<(), TestError> {
        let frame = disc_under_strip()?;
        let background = super::corner_background(&frame);
        // The corners are the grey backdrop.
        assert!((background.x - f32::from(GREY[0]) / 255.0).abs() < 1e-3);
        // The green disc is fully painted against it.
        let painted = super::coverage_not_background(&frame, DISC, background);
        assert!(painted > 0.95, "the disc paints {painted} of itself");
        // An empty grey corner paints nothing.
        let corner = Silhouette {
            centre: Vec2::new(28.0, 28.0),
            radius: 2.0,
        };
        assert!(super::coverage_not_background(&frame, corner, background) < 1e-6);
        Ok(())
    }

    #[test]
    fn health_names_a_black_frame_and_a_transparent_one() -> Result<(), TestError> {
        let black = frame(blank(4), 4)?;
        assert_eq!(
            health(&black),
            FrameHealth {
                all_black: true,
                all_transparent: false
            }
        );
        let transparent = frame(vec![0; 64], 4)?;
        assert_eq!(
            health(&transparent),
            FrameHealth {
                all_black: true,
                all_transparent: true
            }
        );
        assert_eq!(
            health(&disc_under_strip()?),
            FrameHealth {
                all_black: false,
                all_transparent: false
            }
        );
        Ok(())
    }

    /// A band reads back as the mean of its own rows, and an empty range as
    /// nothing at all rather than as black.
    #[test]
    fn a_band_is_the_mean_of_its_rows() -> Result<(), TestError> {
        // Two solid halves, so each band's mean is exactly its own colour and a
        // band spanning both is exactly halfway between them.
        const SIDE: u32 = 8;
        let mut pixels = blank(SIDE);
        for y in 0..SIDE {
            for x in 0..SIDE {
                paint(&mut pixels, SIDE, x, y, if y < 4 { RED } else { GREEN });
            }
        }
        let frame = frame(pixels, SIDE)?;
        let top = super::band_mean(&frame, 0, 4).ok_or("the top band")?;
        let bottom = super::band_mean(&frame, 4, 8).ok_or("the bottom band")?;
        assert!((top.x - f32::from(RED[0]) / 255.0).abs() < 1e-3);
        assert!((bottom.y - f32::from(GREEN[1]) / 255.0).abs() < 1e-3);
        let whole = super::band_mean(&frame, 0, 8).ok_or("the whole frame")?;
        assert!(
            (whole.x - f32::midpoint(top.x, bottom.x)).abs() < 1e-3,
            "{whole} is not halfway between the two halves"
        );
        // Rows past the bottom of the frame are clamped away, so an empty range
        // is `None` and never a black mean a caller would compare against.
        assert_eq!(super::band_mean(&frame, 8, 16), None);
        assert_eq!(super::band_mean(&frame, 5, 4), None);
        Ok(())
    }

    /// Two pixels differ by the same threshold two frames do.
    #[test]
    fn two_pixels_differ_by_the_frame_threshold() {
        let red = as_vec(RED);
        assert!(super::pixels_differ(red, as_vec(GREEN)));
        assert!(!super::pixels_differ(red, red));
        // Four steps of an 8-bit channel is below the eight-step threshold.
        assert!(!super::pixels_differ(red, as_vec([234, 25, 25, 255])));
    }

    /// The change centroid finds where two frames differ, ignores where they
    /// agree, and stays silent when almost nothing changed.
    #[test]
    fn the_change_centroid_points_at_what_moved() -> Result<(), TestError> {
        const SIDE: u32 = 16;
        let solid = |rgba: [u8; 4]| {
            let mut pixels = blank(SIDE);
            for y in 0..SIDE {
                for x in 0..SIDE {
                    paint(&mut pixels, SIDE, x, y, rgba);
                }
            }
            pixels
        };
        // Two frames alike everywhere but a 4×4 block in the top-left quarter,
        // whose centre is (4.5, 2.5).
        let before = frame(solid(GREY), SIDE)?;
        let mut painted = solid(GREY);
        for y in 1..=4 {
            for x in 3..=6 {
                paint(&mut painted, SIDE, x, y, RED);
            }
        }
        let after = frame(painted, SIDE)?;
        let at = super::changed_centroid(&before, &after, None).ok_or("a change to point at")?;
        assert!(
            (at.x - 4.5).abs() < 0.51 && (at.y - 2.5).abs() < 0.51,
            "the change centroid is {at:?}, not the block that changed"
        );
        // Restricted to a disc the block is outside of, there is nothing to
        // point at — and two identical frames never have one.
        let elsewhere = Silhouette {
            centre: Vec2::new(12.0, 12.0),
            radius: 3.0,
        };
        assert_eq!(
            super::changed_centroid(&before, &after, Some(elsewhere)),
            None
        );
        assert_eq!(super::changed_centroid(&before, &before, None), None);
        Ok(())
    }

    /// Luminance orders a darkened pixel below the pixel it was darkened from,
    /// weights green above red above blue, and ignores the alpha channel — which
    /// in the full-stack tier's frames is the glow mask, not opacity.
    #[test]
    fn luminance_orders_bright_above_dark_and_ignores_alpha() {
        let luminance = super::luminance;
        let white = Vec4::new(1.0, 1.0, 1.0, 0.0);
        assert!((luminance(white) - 1.0).abs() < 1e-5);
        assert!((luminance(Vec4::ZERO)).abs() < 1e-5);
        // Half as bright is half the luminance: the weighting is linear, so a
        // "the sky halved" claim is a claim about the light and not about the
        // curve this oracle applies.
        let half = Vec4::new(0.5, 0.5, 0.5, 0.0);
        assert!((luminance(half) - 0.5).abs() < 1e-5);
        // Green above red above blue, at equal channel value.
        let only = |channel: usize| {
            let mut pixel = Vec4::ZERO;
            match channel {
                0 => pixel.x = 1.0,
                1 => pixel.y = 1.0,
                _ => pixel.z = 1.0,
            }
            luminance(pixel)
        };
        assert!(only(1) > only(0) && only(0) > only(2));
        // The glow mask does not brighten a sky.
        assert!(
            (luminance(Vec4::new(0.2, 0.2, 0.2, 1.0)) - luminance(Vec4::splat(0.2))).abs() < 1e-5
        );
    }
}
