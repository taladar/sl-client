//! The contact sheet: the two viewers' frames, tiled and named.
//!
//! This is the output that gets looked at first and the one that gets pasted
//! into an issue, so it carries its own context: which scene, which camera,
//! which viewer, which frame. A sheet that has to be read next to the command
//! line that produced it is a sheet nobody can forward.
//!
//! The layout is one **row per capture index** and one **column per viewer**, so
//! the eye compares left to right — the two viewers at the same moment of the
//! run — and scans top to bottom for the moment something changed. A one-sided
//! run gets one column rather than an apology: a capture is worth looking at
//! even when there is nothing to compare it with.
//!
//! # Frames are dropped, and the sheet says so
//!
//! A thirty-frame run at 1920×1080 is sixty images; a sheet of all of them is
//! 40 000 pixels tall and is opened by nobody. So a sheet takes a bounded number
//! of rows, spread evenly across the run, and [`Sheet::dropped`] says how many
//! it left out. Never a silent cap: a list that stops without saying so reads as
//! a list that ended.
//!
//! # An animated subject compares two phases, not two viewers
//!
//! Two frames photographed half a second apart out of a two-second looping
//! motion are at two arbitrary points in the loop, and the avatars in them will
//! be posed differently no matter how right both viewers are. That nearly cost a
//! wrong bug report already. The sheet cannot fix it — the scene dump's
//! `loop_time` is what says where each clock had reached — so it says so in its
//! own caption instead of letting a reader draw the conclusion the pictures
//! invite.

use std::path::{Path, PathBuf};

use crate::font;

/// The colour a sheet is drawn on.
const BACKGROUND: [u8; 3] = [22, 22, 26];

/// The colour a label is drawn in.
const LABEL: [u8; 3] = [225, 225, 230];

/// The colour of a cell that has no frame in it.
const EMPTY: [u8; 3] = [48, 30, 30];

/// The gap between cells, in pixels.
const PAD: u32 = 8;

/// What went wrong building a sheet.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// There were no frames at all to tile.
    #[error("no frames to tile: neither viewer left one")]
    Empty,
    /// The sheet could not be written.
    #[error("could not write {path}: {source}")]
    Write {
        /// Where it was going.
        path: String,
        /// Why it could not be written.
        source: image::ImageError,
    },
}

/// One viewer's column.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Column {
    /// The viewer's name, as the label reads.
    pub viewer: String,
    /// Its frames, in capture order.
    pub frames: Vec<PathBuf>,
}

/// What a sheet was asked for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Spec {
    /// The line printed across the top: the scene and the camera.
    pub title: String,
    /// A second line, for whatever else a reader needs to know.
    pub subtitle: String,
    /// One column per viewer.
    pub columns: Vec<Column>,
    /// How wide one cell is drawn, in pixels.
    pub cell_width: u32,
    /// At most this many rows.
    pub rows: usize,
}

impl Spec {
    /// A sheet of `columns`, at this crate's defaults.
    #[must_use]
    pub fn new(title: impl Into<String>, columns: Vec<Column>) -> Self {
        Self {
            title: title.into(),
            subtitle: "an animated subject differs by phase: two frames of one loop are not two \
                       viewers disagreeing — read loop_time in the scene dump"
                .to_owned(),
            columns,
            // 640: two of them side by side is a 1300-pixel image, which fits in
            // an issue comment without being scrolled and still shows a prim.
            cell_width: 640,
            rows: 6,
        }
    }
}

/// A sheet that was written.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sheet {
    /// Where it went.
    pub path: PathBuf,
    /// The capture indices it drew, in order.
    pub drawn: Vec<usize>,
    /// How many frames it left out.
    pub dropped: usize,
}

/// Tile the frames named in `spec` into a sheet at `path`.
///
/// # Errors
///
/// [`Error::Empty`] when no column has a frame, [`Error::Write`] when
/// the image cannot be saved.
pub fn build(spec: &Spec, path: &Path) -> Result<Sheet, Error> {
    let longest = spec
        .columns
        .iter()
        .map(|column| column.frames.len())
        .max()
        .unwrap_or(0);
    if longest == 0 {
        return Err(Error::Empty);
    }
    let chosen = spread(longest, spec.rows.max(1));
    let cells: Vec<Vec<Cell>> = chosen
        .iter()
        .map(|index| {
            spec.columns
                .iter()
                .map(|column| Cell::load(column.frames.get(*index), spec.cell_width))
                .collect()
        })
        .collect();

    let scale = 2;
    let line = font::height(scale);
    let label_strip = line.saturating_add(PAD);
    let row_heights: Vec<u32> = cells
        .iter()
        .map(|row| {
            row.iter()
                .map(Cell::height)
                .max()
                .unwrap_or(0)
                .saturating_add(label_strip)
                .saturating_add(PAD)
        })
        .collect();
    let columns = u32::try_from(spec.columns.len()).unwrap_or(1).max(1);
    let width = columns
        .saturating_mul(spec.cell_width.saturating_add(PAD))
        .saturating_add(PAD);
    let header = line.saturating_mul(2).saturating_add(PAD.saturating_mul(3));
    let height = row_heights
        .iter()
        .fold(header, |total, row| total.saturating_add(*row));

    let mut sheet = image::RgbImage::from_pixel(width, height, image::Rgb(BACKGROUND));
    write(&mut sheet, &spec.title, PAD, PAD, scale, LABEL);
    write(
        &mut sheet,
        &spec.subtitle,
        PAD,
        PAD.saturating_add(line)
            .saturating_add(PAD.saturating_div(2)),
        1,
        LABEL,
    );

    let mut top = header;
    for ((row, cells), row_height) in chosen.iter().zip(cells.iter()).zip(row_heights.iter()) {
        for (column, cell) in cells.iter().enumerate() {
            let left = PAD.saturating_add(
                u32::try_from(column)
                    .unwrap_or(0)
                    .saturating_mul(spec.cell_width.saturating_add(PAD)),
            );
            let name = spec
                .columns
                .get(column)
                .map_or("?", |column| column.viewer.as_str());
            write(
                &mut sheet,
                &format!("{name}  frame {row:03}"),
                left,
                top,
                scale,
                LABEL,
            );
            cell.blit(
                &mut sheet,
                left,
                top.saturating_add(label_strip),
                spec.cell_width,
            );
        }
        top = top.saturating_add(*row_height);
    }

    sheet.save(path).map_err(|source| Error::Write {
        path: path.display().to_string(),
        source,
    })?;
    Ok(Sheet {
        path: path.to_path_buf(),
        drawn: chosen.clone(),
        dropped: longest.saturating_sub(chosen.len()),
    })
}

/// One cell of the sheet: a frame scaled to the cell width, or the reason there
/// is none.
enum Cell {
    /// A frame, already scaled.
    Frame(image::RgbImage),
    /// No frame, and why — drawn as a coloured block with the reason in it.
    None(String),
}

impl Cell {
    /// Load and scale one frame, or record why there is not one.
    fn load(path: Option<&PathBuf>, cell_width: u32) -> Self {
        let Some(path) = path else {
            return Self::None("no frame at this index".to_owned());
        };
        match image::open(path) {
            Ok(image) => {
                let image = image.to_rgb8();
                let (width, height) = image.dimensions();
                if width == 0 || height == 0 {
                    return Self::None("an empty frame".to_owned());
                }
                let scaled_height = u32::try_from(
                    u64::from(cell_width)
                        .saturating_mul(u64::from(height))
                        .checked_div(u64::from(width))
                        .unwrap_or(0),
                )
                .unwrap_or(height);
                Self::Frame(image::imageops::resize(
                    &image,
                    cell_width,
                    scaled_height.max(1),
                    image::imageops::FilterType::Triangle,
                ))
            }
            Err(error) => Self::None(format!("unreadable: {error}")),
        }
    }

    /// How tall this cell is drawn.
    fn height(&self) -> u32 {
        match self {
            Self::Frame(image) => image.height(),
            // Tall enough to be seen and short enough not to pretend to be a
            // frame.
            Self::None(_why) => font::height(2).saturating_mul(3),
        }
    }

    /// Draw this cell into `sheet` with its top-left corner at `(x, y)`.
    fn blit(&self, sheet: &mut image::RgbImage, x: u32, y: u32, cell_width: u32) {
        match self {
            Self::Frame(image) => {
                image::imageops::replace(sheet, image, i64::from(x), i64::from(y));
            }
            Self::None(why) => {
                for down in 0..self.height() {
                    for across in 0..cell_width {
                        put(
                            sheet,
                            x.saturating_add(across),
                            y.saturating_add(down),
                            EMPTY,
                        );
                    }
                }
                write(
                    sheet,
                    why,
                    x.saturating_add(PAD),
                    y.saturating_add(PAD),
                    2,
                    LABEL,
                );
            }
        }
    }
}

/// Which capture indices a sheet of at most `rows` rows draws out of `count`.
///
/// Spread evenly rather than taken from the front: the interesting part of a
/// capture run is usually its end, when everything has finished loading, and a
/// sheet of the first six frames is a sheet of a scene still rezzing.
fn spread(count: usize, rows: usize) -> Vec<usize> {
    if count <= rows {
        return (0..count).collect();
    }
    (0..rows)
        .map(|row| {
            row.saturating_mul(count.saturating_sub(1))
                .checked_div(rows.saturating_sub(1).max(1))
                .unwrap_or(0)
        })
        .collect()
}

/// Draw `text` into `sheet`.
fn write(sheet: &mut image::RgbImage, text: &str, x: u32, y: u32, scale: u32, colour: [u8; 3]) {
    font::draw(text, x, y, scale, &mut |x, y| put(sheet, x, y, colour));
}

/// Set one pixel, ignoring one that falls outside the sheet.
fn put(sheet: &mut image::RgbImage, x: u32, y: u32, colour: [u8; 3]) {
    if x < sheet.width() && y < sheet.height() {
        sheet.put_pixel(x, y, image::Rgb(colour));
    }
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::{Column, Spec, build, spread};

    /// The boxed error every test in this module reports through.
    type TestError = Box<dyn core::error::Error>;

    /// A scratch directory of this test's own.
    fn scratch(name: &str) -> Result<std::path::PathBuf, TestError> {
        let dir =
            std::env::temp_dir().join(format!("sl-crosscheck-sheet-{name}-{}", std::process::id()));
        let _ignored = fs_err::remove_dir_all(&dir);
        fs_err::create_dir_all(&dir)?;
        Ok(dir)
    }

    /// Write `count` frames of a solid colour.
    fn frames(
        dir: &std::path::Path,
        prefix: &str,
        count: usize,
        colour: [u8; 3],
    ) -> Result<Vec<std::path::PathBuf>, TestError> {
        let mut paths = Vec::new();
        for index in 0..count {
            let path = dir.join(format!("{prefix}_{index:03}.png"));
            image::RgbImage::from_pixel(160, 90, image::Rgb(colour)).save(&path)?;
            paths.push(path);
        }
        Ok(paths)
    }

    /// A two-column sheet is written, and it is wide enough for both columns —
    /// the eye compares left to right.
    #[test]
    fn a_pair_becomes_two_columns() -> Result<(), TestError> {
        let dir = scratch("pair")?;
        let spec = Spec::new(
            "catalogue — camera at <120, 100, 26>",
            vec![
                Column {
                    viewer: "sl-client".to_owned(),
                    frames: frames(&dir, "ours", 3, [30, 90, 30])?,
                },
                Column {
                    viewer: "firestorm".to_owned(),
                    frames: frames(&dir, "theirs", 3, [90, 30, 30])?,
                },
            ],
        );
        let path = dir.join("contact-sheet.png");
        let sheet = build(&spec, &path)?;
        assert_eq!(sheet.drawn, vec![0, 1, 2]);
        assert_eq!(sheet.dropped, 0);
        let written = image::open(&path)?.to_rgb8();
        assert!(written.width() > spec.cell_width.saturating_mul(2));
        fs_err::remove_dir_all(&dir)?;
        Ok(())
    }

    /// A one-sided run gets one column rather than an error: a capture is worth
    /// looking at even when there is nothing to compare it against.
    #[test]
    fn a_one_sided_run_still_gets_a_sheet() -> Result<(), TestError> {
        let dir = scratch("one-sided")?;
        let spec = Spec::new(
            "catalogue",
            vec![Column {
                viewer: "sl-client".to_owned(),
                frames: frames(&dir, "ours", 2, [30, 30, 90])?,
            }],
        );
        let sheet = build(&spec, &dir.join("sheet.png"))?;
        assert_eq!(sheet.drawn, vec![0, 1]);
        fs_err::remove_dir_all(&dir)?;
        Ok(())
    }

    /// A frame one viewer did not write is drawn as a labelled hole, not as a
    /// black frame: a reader must never mistake "there is nothing here" for
    /// "this viewer drew black".
    #[test]
    fn a_missing_frame_is_drawn_as_a_hole() -> Result<(), TestError> {
        let dir = scratch("hole")?;
        let spec = Spec::new(
            "catalogue",
            vec![
                Column {
                    viewer: "sl-client".to_owned(),
                    frames: frames(&dir, "ours", 2, [30, 90, 30])?,
                },
                Column {
                    viewer: "firestorm".to_owned(),
                    frames: Vec::new(),
                },
            ],
        );
        let path = dir.join("sheet.png");
        let sheet = build(&spec, &path)?;
        assert_eq!(sheet.drawn, vec![0, 1]);
        let written = image::open(&path)?.to_rgb8();
        let has_empty = written.pixels().any(|pixel| pixel.0 == super::EMPTY);
        assert!(has_empty, "the missing half should be a visible hole");
        fs_err::remove_dir_all(&dir)?;
        Ok(())
    }

    /// A run longer than the sheet is spread across the whole run and says how
    /// many frames it left out. The end of a capture is the interesting part —
    /// a sheet of the first six frames is a sheet of a scene still rezzing.
    #[test]
    fn a_long_run_is_spread_and_says_what_it_dropped() -> Result<(), TestError> {
        assert_eq!(spread(30, 6), vec![0, 5, 11, 17, 23, 29]);
        assert_eq!(spread(3, 6), vec![0, 1, 2]);
        assert_eq!(spread(1, 1), vec![0]);

        let dir = scratch("spread")?;
        let mut spec = Spec::new(
            "catalogue",
            vec![Column {
                viewer: "sl-client".to_owned(),
                frames: frames(&dir, "ours", 10, [30, 90, 30])?,
            }],
        );
        spec.rows = 3;
        let sheet = build(&spec, &dir.join("sheet.png"))?;
        assert_eq!(sheet.drawn, vec![0, 4, 9]);
        assert_eq!(sheet.dropped, 7);
        fs_err::remove_dir_all(&dir)?;
        Ok(())
    }

    /// A sheet of nothing is an error rather than an empty image: a zero-byte
    /// picture is a thing a reader stares at trying to see something.
    #[test]
    fn a_sheet_of_nothing_is_an_error() -> Result<(), TestError> {
        let dir = scratch("empty")?;
        let spec = Spec::new(
            "catalogue",
            vec![Column {
                viewer: "sl-client".to_owned(),
                frames: Vec::new(),
            }],
        );
        let error = build(&spec, &dir.join("sheet.png"))
            .err()
            .ok_or("a sheet of nothing should be an error")?;
        assert!(error.to_string().contains("no frames to tile"));
        fs_err::remove_dir_all(&dir)?;
        Ok(())
    }
}
