//! The image diff: **a number, not a verdict**.
//!
//! Two viewers photographing one scene do not share a renderer, so tone mapping,
//! exposure, shadow filtering and anti-aliasing differ everywhere at once. A
//! large baseline difference is therefore expected, and this module measures it
//! rather than judging it. What is worth attention is a *change* in the
//! difference between runs, or a difference **localised** to one part of the
//! frame — which is why every pair reports its worst tiles as well as its
//! totals.
//!
//! Three numbers, because no one of them is enough:
//!
//! - [`mean_abs`](FramePair::mean_abs) — the average absolute channel
//!   difference. Cheap, and dominated by the global tone difference, so it is
//!   the *baseline* number: it says how far apart these two renderers are today.
//! - [`differing_fraction`](FramePair::differing_fraction) — how much of the
//!   frame differs by more than a threshold. This separates "everything is
//!   slightly darker" from "one object is wrong".
//! - [`ssim`](FramePair::ssim) — structural similarity on luma, over 8×8
//!   windows. Deliberately insensitive to a uniform brightness or contrast
//!   shift and sensitive to structure, which is the half of the comparison a
//!   mean cannot do: a mesh at the wrong level of detail moves SSIM while
//!   barely moving the mean.
//!
//! **Nothing here is a test.** It never enters `cargo nextest` and never fails a
//! build, for the same reason this workspace has no golden images: a pixel
//! comparison across two renderers, two GPUs and two driver versions measures
//! the environment at least as much as the code, and a check that fails on a
//! Mesa upgrade is one that gets disabled and then ignored. The tiered harness
//! says *wrong*; this says *different*, and a person decides which viewer is
//! right.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// The side of a square tile the localisation grid uses, in pixels.
pub const TILE: u32 = 32;

/// How many of the worst tiles a pair reports.
const WORST_TILES: usize = 4;

/// How far a pixel's worst channel must move to count as differing, out of 255.
///
/// Eight: two renderers agreeing to within 3% of full range are agreeing as
/// closely as two different tone-mapping curves ever will, and counting those
/// as differences would put every pixel of every frame in the count.
pub const DIFFERENT_ENOUGH: u8 = 8;

/// One frame from each viewer, and how far apart they are.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FramePair {
    /// Which frame of the capture sequence this is.
    pub index: usize,
    /// This viewer's frame.
    pub left: PathBuf,
    /// The reference's frame.
    pub right: PathBuf,
    /// The size both were rendered at.
    pub size: (u32, u32),
    /// The mean absolute channel difference, `0.0..=1.0`.
    pub mean_abs: f64,
    /// The fraction of pixels differing by more than [`DIFFERENT_ENOUGH`].
    pub differing_fraction: f64,
    /// Structural similarity on luma, `0.0..=1.0`; 1.0 is identical structure.
    pub ssim: f64,
    /// The worst tiles, worst first — where the difference actually is.
    pub worst_tiles: Vec<Tile>,
    /// The difference image, when one was written.
    pub heatmap: Option<PathBuf>,
}

impl FramePair {
    /// The line a report prints for this pair.
    #[must_use]
    pub fn describe(&self) -> String {
        let tiles: Vec<String> = self
            .worst_tiles
            .iter()
            .map(|tile| format!("<{},{}> {:.3}", tile.x, tile.y, tile.mean_abs))
            .collect();
        format!(
            "  frame {:03}: mean {:.4}, {:.1}% of pixels differ, ssim {:.4}\n    worst tiles: {}",
            self.index,
            self.mean_abs,
            self.differing_fraction * 100.0,
            self.ssim,
            if tiles.is_empty() {
                "none".to_owned()
            } else {
                tiles.join(", ")
            }
        )
    }
}

/// One square of the frame, and how far apart the two viewers are inside it.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Tile {
    /// The tile's left edge, in pixels.
    pub x: u32,
    /// The tile's top edge, in pixels.
    pub y: u32,
    /// The tile's side, in pixels.
    pub side: u32,
    /// The mean absolute channel difference inside it.
    pub mean_abs: f64,
}

/// What became of one pair of frames.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum Compared {
    /// The two frames were compared.
    Pair(FramePair),
    /// The two frames are different sizes, so they were not compared.
    ///
    /// Not resampled first: a resample invents pixels, and every number
    /// downstream would then be measuring the resampler. Both viewers are told
    /// one capture size, so this is a run that did not do what it was told.
    Mismatched {
        /// Which frame of the sequence.
        index: usize,
        /// This viewer's size.
        left: (u32, u32),
        /// The reference's size.
        right: (u32, u32),
    },
    /// One of the two could not be read.
    Unreadable {
        /// Which frame of the sequence.
        index: usize,
        /// The file that could not be read.
        path: PathBuf,
        /// Why not.
        problem: String,
    },
}

impl Compared {
    /// The pair, when there was one.
    #[must_use]
    pub const fn pair(&self) -> Option<&FramePair> {
        match self {
            Self::Pair(pair) => Some(pair),
            Self::Mismatched { .. } | Self::Unreadable { .. } => None,
        }
    }

    /// The line a report prints.
    #[must_use]
    pub fn describe(&self) -> String {
        match self {
            Self::Pair(pair) => pair.describe(),
            Self::Mismatched { index, left, right } => format!(
                "  frame {index:03}: not compared — {}x{} here against {}x{} there; both viewers \
                 were told one capture size, so this is a run that did not do as it was told",
                left.0, left.1, right.0, right.1
            ),
            Self::Unreadable {
                index,
                path,
                problem,
            } => format!(
                "  frame {index:03}: not compared — {} could not be read: {problem}",
                path.display()
            ),
        }
    }
}

/// Compare each viewer's frames, pairwise by capture index.
///
/// Pairwise by index and not otherwise: the two viewers capture on the same
/// schedule, so frame *n* of one is the same moment of the run as frame *n* of
/// the other. When one viewer wrote fewer frames the extra ones are dropped —
/// and said so by the caller, which knows how many each side wrote.
///
/// `heatmaps` is where a per-pair difference image goes; `None` writes none.
#[must_use]
pub fn compare(left: &[PathBuf], right: &[PathBuf], heatmaps: Option<&Path>) -> Vec<Compared> {
    left.iter()
        .zip(right.iter())
        .enumerate()
        .map(|(index, (left, right))| compare_pair(index, left, right, heatmaps))
        .collect()
}

/// Compare one pair of frames.
fn compare_pair(index: usize, left: &Path, right: &Path, heatmaps: Option<&Path>) -> Compared {
    let read = |path: &Path| {
        image::open(path)
            .map(|image| image.to_rgb8())
            .map_err(|error| error.to_string())
    };
    let unreadable = |path: &Path, problem: String| Compared::Unreadable {
        index,
        path: path.to_path_buf(),
        problem,
    };
    let ours = match read(left) {
        Ok(image) => image,
        Err(problem) => return unreadable(left, problem),
    };
    let theirs = match read(right) {
        Ok(image) => image,
        Err(problem) => return unreadable(right, problem),
    };
    if ours.dimensions() != theirs.dimensions() {
        return Compared::Mismatched {
            index,
            left: ours.dimensions(),
            right: theirs.dimensions(),
        };
    }
    let (width, height) = ours.dimensions();
    let measured = measure(&ours, &theirs);
    let heatmap = heatmaps.and_then(|directory| {
        let path = directory.join(format!("diff_{index:03}.png"));
        write_heatmap(&measured.per_pixel, width, height, &path).map(|()| path)
    });
    Compared::Pair(FramePair {
        index,
        left: left.to_path_buf(),
        right: right.to_path_buf(),
        size: (width, height),
        mean_abs: measured.mean_abs,
        differing_fraction: measured.differing_fraction,
        ssim: measured.ssim,
        worst_tiles: measured.worst_tiles,
        heatmap,
    })
}

/// Everything one pass over the two frames measures.
struct Measured {
    /// The worst channel difference at each pixel, row-major.
    per_pixel: Vec<u8>,
    /// The mean absolute channel difference over the whole frame.
    mean_abs: f64,
    /// The fraction of pixels differing by more than [`DIFFERENT_ENOUGH`].
    differing_fraction: f64,
    /// Structural similarity on luma.
    ssim: f64,
    /// The worst tiles, worst first.
    worst_tiles: Vec<Tile>,
}

/// Walk the two frames once and measure everything at the same time.
///
/// Once rather than three times: a 1080p pair is two million pixels, and the
/// difference between one pass and three is the difference between a report that
/// runs while its reader waits and one they go and make tea during.
#[expect(
    clippy::arithmetic_side_effects,
    clippy::indexing_slicing,
    reason = "every index is derived from the shared dimensions of two images already checked to \
              be the same size, and the accumulators are f64 and u64 over a bounded pixel count"
)]
fn measure(ours: &image::RgbImage, theirs: &image::RgbImage) -> Measured {
    let (width, height) = ours.dimensions();
    let pixels = usize::try_from(width).unwrap_or(0) * usize::try_from(height).unwrap_or(0);
    let mut per_pixel = vec![0_u8; pixels];
    let mut our_luma = vec![0.0_f32; pixels];
    let mut their_luma = vec![0.0_f32; pixels];
    let mut total = 0.0_f64;
    let mut differing = 0_u64;
    let tiles_across = width.div_ceil(TILE);
    let tiles_down = height.div_ceil(TILE);
    let mut tile_totals =
        vec![(0.0_f64, 0_u64); usize::try_from(tiles_across * tiles_down).unwrap_or(0)];

    for (index, (ours, theirs)) in ours.pixels().zip(theirs.pixels()).enumerate() {
        let [our_red, our_green, our_blue] = ours.0;
        let [their_red, their_green, their_blue] = theirs.0;
        let red = our_red.abs_diff(their_red);
        let green = our_green.abs_diff(their_green);
        let blue = our_blue.abs_diff(their_blue);
        let worst = red.max(green).max(blue);
        per_pixel[index] = worst;
        let sum = f64::from(red) + f64::from(green) + f64::from(blue);
        total += sum;
        if worst > DIFFERENT_ENOUGH {
            differing += 1;
        }
        our_luma[index] = luma(ours.0);
        their_luma[index] = luma(theirs.0);
        let x = u32::try_from(index).unwrap_or(0) % width;
        let y = u32::try_from(index).unwrap_or(0) / width;
        let tile = usize::try_from((y / TILE) * tiles_across + (x / TILE)).unwrap_or(0);
        if let Some(entry) = tile_totals.get_mut(tile) {
            entry.0 += sum / 3.0;
            entry.1 += 1;
        }
    }

    let counted = if pixels == 0 { 1.0 } else { count(pixels) };
    let mut worst_tiles: Vec<Tile> = tile_totals
        .into_iter()
        .enumerate()
        .filter(|(_index, (_sum, count))| *count > 0)
        .map(|(index, (sum, count))| {
            let index = u32::try_from(index).unwrap_or(0);
            Tile {
                x: (index % tiles_across) * TILE,
                y: (index / tiles_across) * TILE,
                side: TILE,
                mean_abs: sum / count_u64(count) / 255.0,
            }
        })
        .collect();
    worst_tiles.sort_by(|first, second| second.mean_abs.total_cmp(&first.mean_abs));
    worst_tiles.truncate(WORST_TILES);

    Measured {
        mean_abs: total / (counted * 3.0 * 255.0),
        differing_fraction: count_u64(differing) / counted,
        ssim: ssim(&our_luma, &their_luma, width, height),
        worst_tiles,
        per_pixel,
    }
}

/// The perceived brightness of a pixel, on the reference's own weights.
fn luma(pixel: [u8; 3]) -> f32 {
    let [red, green, blue] = pixel;
    0.299 * f32::from(red) + 0.587 * f32::from(green) + 0.114 * f32::from(blue)
}

/// Structural similarity between two luma planes, averaged over 8×8 windows.
///
/// The textbook formulation with the textbook constants, on 8-bit luma. Windows
/// rather than a global figure because the interesting case is a *localised*
/// difference: one object wrong in a frame that is otherwise identical moves a
/// windowed SSIM and is lost in a global one.
#[expect(
    clippy::arithmetic_side_effects,
    clippy::indexing_slicing,
    reason = "indices are bounded by the plane's own dimensions and the sums are over at most \
              64 8-bit samples"
)]
fn ssim(ours: &[f32], theirs: &[f32], width: u32, height: u32) -> f64 {
    /// The side of an SSIM window, in pixels.
    const WINDOW: u32 = 8;
    /// Stabilises the luminance term where the means are near zero.
    const C1: f64 = 6.5025;
    /// Stabilises the contrast term where the variances are near zero.
    const C2: f64 = 58.5225;

    if width < WINDOW || height < WINDOW {
        return 1.0;
    }
    let mut total = 0.0_f64;
    let mut windows = 0_u64;
    for top in (0..=height - WINDOW).step_by(usize::try_from(WINDOW).unwrap_or(8)) {
        for left in (0..=width - WINDOW).step_by(usize::try_from(WINDOW).unwrap_or(8)) {
            let (mut our_sum, mut their_sum) = (0.0_f64, 0.0_f64);
            let (mut our_square, mut their_square, mut cross) = (0.0_f64, 0.0_f64, 0.0_f64);
            for y in top..top + WINDOW {
                for x in left..left + WINDOW {
                    let index = usize::try_from(y * width + x).unwrap_or(0);
                    let ours = f64::from(ours[index]);
                    let theirs = f64::from(theirs[index]);
                    our_sum += ours;
                    their_sum += theirs;
                    our_square += ours * ours;
                    their_square += theirs * theirs;
                    cross += ours * theirs;
                }
            }
            let count = f64::from(WINDOW * WINDOW);
            let our_mean = our_sum / count;
            let their_mean = their_sum / count;
            // Through a closure rather than twice in a row: two mirror-image
            // lines of `square / count - mean * mean` are exactly the shape
            // clippy reads as a typo, and it is right to — one of them being
            // wrong is a real way to write this.
            let variance = |square: f64, mean: f64| square / count - mean.powi(2);
            let our_variance = variance(our_square, our_mean);
            let their_variance = variance(their_square, their_mean);
            let covariance = cross / count - (our_mean * their_mean);
            let numerator = (2.0 * our_mean * their_mean + C1) * (2.0 * covariance + C2);
            let denominator = (our_mean * our_mean + their_mean * their_mean + C1)
                * (our_variance + their_variance + C2);
            if denominator.abs() > f64::EPSILON {
                total += numerator / denominator;
                windows += 1;
            }
        }
    }
    if windows == 0 {
        1.0
    } else {
        total / count_u64(windows)
    }
}

/// Write the difference image: black where the two agree, through red and
/// yellow to white where they are furthest apart.
///
/// A ramp rather than a grey: the eye finds a red patch on black in a moment and
/// hunts for a dark grey one. Returns `None` — rather than failing the report —
/// when the image cannot be written: a missing heatmap is a smaller loss than a
/// report that did not print.
fn write_heatmap(per_pixel: &[u8], width: u32, height: u32, path: &Path) -> Option<()> {
    let mut heatmap = image::RgbImage::new(width, height);
    for (pixel, difference) in heatmap.pixels_mut().zip(per_pixel.iter().copied()) {
        *pixel = image::Rgb(ramp(difference));
    }
    match heatmap.save(path) {
        Ok(()) => Some(()),
        Err(error) => {
            tracing::warn!("could not write {}: {error}", path.display());
            None
        }
    }
}

/// The colour a difference of `amount` is drawn as.
fn ramp(amount: u8) -> [u8; 3] {
    // Three segments: black to red, red to yellow, yellow to white. Each is a
    // third of the range, so an eye reads the ramp as a scale rather than as a
    // gradient.
    let step = |value: u8, from: u8, span: u8| -> u8 {
        value
            .saturating_sub(from)
            .saturating_mul(255_u8.checked_div(span).unwrap_or(1))
    };
    match amount {
        0..=84 => [step(amount, 0, 85), 0, 0],
        85..=169 => [255, step(amount, 85, 85), 0],
        _brightest => [255, 255, step(amount, 170, 85)],
    }
}

/// A pixel count as a `f64`.
fn count(pixels: usize) -> f64 {
    u32::try_from(pixels).map_or_else(|_too_many| f64::from(u32::MAX), f64::from)
}

/// A counted quantity as a `f64`.
fn count_u64(counted: u64) -> f64 {
    u32::try_from(counted).map_or_else(|_too_many| f64::from(u32::MAX), f64::from)
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::{Compared, compare};

    /// The boxed error every test in this module reports through.
    type TestError = Box<dyn core::error::Error>;

    /// A scratch directory of this test's own.
    fn scratch(name: &str) -> Result<std::path::PathBuf, TestError> {
        let dir = std::env::temp_dir().join(format!(
            "sl-crosscheck-frames-{name}-{}",
            std::process::id()
        ));
        let _ignored = fs_err::remove_dir_all(&dir);
        fs_err::create_dir_all(&dir)?;
        Ok(dir)
    }

    /// Write a frame whose pixels come from `paint`.
    fn frame(
        dir: &std::path::Path,
        name: &str,
        size: (u32, u32),
        paint: impl Fn(u32, u32) -> [u8; 3],
    ) -> Result<std::path::PathBuf, TestError> {
        let path = dir.join(name);
        let mut image = image::RgbImage::new(size.0, size.1);
        for (x, y, pixel) in image.enumerate_pixels_mut() {
            *pixel = image::Rgb(paint(x, y));
        }
        image.save(&path)?;
        Ok(path)
    }

    /// Two identical frames are identical by every measure. The floor has to be
    /// exact, or no number above it means anything.
    #[test]
    fn two_identical_frames_measure_as_identical() -> Result<(), TestError> {
        let dir = scratch("identical")?;
        let paint = |x: u32, y: u32| {
            [
                u8::try_from(x % 256).unwrap_or(0),
                u8::try_from(y % 256).unwrap_or(0),
                128,
            ]
        };
        let ours = frame(&dir, "ours.png", (64, 64), paint)?;
        let theirs = frame(&dir, "theirs.png", (64, 64), paint)?;
        let compared = compare(&[ours], &[theirs], None);
        let pair = compared.first().and_then(Compared::pair).ok_or("no pair")?;
        assert!(pair.mean_abs < 1e-9);
        assert!(pair.differing_fraction < 1e-9);
        assert!((pair.ssim - 1.0).abs() < 1e-9);
        fs_err::remove_dir_all(&dir)?;
        Ok(())
    }

    /// A difference confined to one corner is found *there*: the worst tile
    /// names the corner, which is the whole point of tiling. A frame-wide mean
    /// would report the same small number for a wrong object and for a slightly
    /// wrong exposure.
    #[test]
    fn a_localised_difference_is_localised() -> Result<(), TestError> {
        let dir = scratch("localised")?;
        let ours = frame(&dir, "ours.png", (128, 128), |_x, _y| [40, 40, 40])?;
        let theirs = frame(&dir, "theirs.png", (128, 128), |x, y| {
            if x >= 96 && y >= 96 {
                [240, 40, 40]
            } else {
                [40, 40, 40]
            }
        })?;
        let compared = compare(&[ours], &[theirs], Some(&dir));
        let pair = compared.first().and_then(Compared::pair).ok_or("no pair")?;
        let worst = pair.worst_tiles.first().ok_or("no tiles")?;
        assert!(
            worst.x >= 96 && worst.y >= 96,
            "the worst tile should be the corner that differs, not <{}, {}>",
            worst.x,
            worst.y
        );
        // A sixteenth of the frame differs, and the report says so rather than
        // averaging it away.
        assert!((pair.differing_fraction - 0.0625).abs() < 1e-6);
        assert!(pair.ssim < 1.0);
        assert!(pair.heatmap.is_some(), "a heatmap was asked for");
        fs_err::remove_dir_all(&dir)?;
        Ok(())
    }

    /// A uniform brightness shift moves the mean and leaves the structure alone:
    /// the two numbers answer different questions, and a report that had only
    /// the mean could not tell an exposure difference from a wrong object.
    #[test]
    fn a_brightness_shift_moves_the_mean_more_than_the_structure() -> Result<(), TestError> {
        let dir = scratch("brightness")?;
        let ours = frame(&dir, "ours.png", (64, 64), |x, y| {
            let value = u8::try_from((x * 3 + y) % 200).unwrap_or(0);
            [value, value, value]
        })?;
        let theirs = frame(&dir, "theirs.png", (64, 64), |x, y| {
            let value = u8::try_from((x * 3 + y) % 200)
                .unwrap_or(0)
                .saturating_add(20);
            [value, value, value]
        })?;
        let compared = compare(&[ours], &[theirs], None);
        let pair = compared.first().and_then(Compared::pair).ok_or("no pair")?;
        assert!(pair.mean_abs > 0.05, "the mean moved: {}", pair.mean_abs);
        assert!(pair.ssim > 0.9, "the structure did not: ssim {}", pair.ssim);
        fs_err::remove_dir_all(&dir)?;
        Ok(())
    }

    /// Two frames of different sizes are not compared and not resampled: a
    /// resample would make every number downstream a measurement of the
    /// resampler. Both viewers were told one capture size, so this is a run that
    /// did not do as it was told, and it says so in those words.
    #[test]
    fn frames_of_different_sizes_are_not_compared() -> Result<(), TestError> {
        let dir = scratch("mismatched")?;
        let ours = frame(&dir, "ours.png", (64, 64), |_x, _y| [0, 0, 0])?;
        let theirs = frame(&dir, "theirs.png", (32, 32), |_x, _y| [0, 0, 0])?;
        let compared = compare(&[ours], &[theirs], None);
        let first = compared.first().ok_or("nothing compared")?;
        assert!(matches!(first, Compared::Mismatched { .. }));
        assert!(first.describe().contains("not compared"));
        fs_err::remove_dir_all(&dir)?;
        Ok(())
    }

    /// A frame that cannot be read is reported as one, not as a difference of
    /// zero — the mistake this whole crate is organised around.
    #[test]
    fn an_unreadable_frame_is_not_a_difference_of_zero() -> Result<(), TestError> {
        let dir = scratch("unreadable")?;
        let ours = frame(&dir, "ours.png", (8, 8), |_x, _y| [0, 0, 0])?;
        let broken = dir.join("broken.png");
        fs_err::write(&broken, b"not a png")?;
        let compared = compare(&[ours], &[broken], None);
        let first = compared.first().ok_or("nothing compared")?;
        assert_eq!(first.pair(), None);
        assert!(matches!(first, Compared::Unreadable { .. }));
        fs_err::remove_dir_all(&dir)?;
        Ok(())
    }
}
