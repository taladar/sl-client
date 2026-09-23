//! Draws the nine-sliced widget art an image-backed skin wears
//! (`viewer-skin-image-backed-widgets`).
//!
//! A skin can change a widget's **shape**, not only its colour, by pointing a
//! CSS rule at a nine-sliced image: `themes/relief.css` dresses the viewer's
//! push button in a raised bevel that way. This draws the files that rule
//! names.
//!
//! # Why the art has a generator at all
//!
//! Provenance, reproducibility and review. The PNG files are **ours** — only the
//! *geometry* is borrowed from the reference viewer (a lit edge where the light
//! is, a shaded one opposite, over a hairline frame), and geometry is not what
//! a licence covers; this file is where that is visible. A different size,
//! inset or palette is an edit to a constant rather than a fresh hand-drawn
//! set. And a diff of four PNG blobs says nothing, where a diff of [`STATES`]
//! says everything.
//!
//! It was a Python script first, which is the wrong shape for a tool that may
//! not run again for a year: this one is pinned by `Cargo.lock` and the
//! toolchain, and cannot quietly stop working while nobody is looking.
//!
//! # Running it
//!
//! Build and run this package with no arguments to redraw the shipped skin's
//! `widgets/` directory, or pass a directory to draw somewhere else.
//!
//! It prints what it wrote, which makes a run self-checking: the corner of the
//! resting file reads lighter than its centre, and of the pressed file darker,
//! so the bevel inversion is visible as numbers.

use std::path::{Path, PathBuf};

use image::{Rgba, RgbaImage};

/// The edge of each file, in pixels.
///
/// Small on purpose: every pixel of a nine-slice is either a corner drawn 1:1
/// or an edge stretched along one axis, so the art carries no detail a larger
/// canvas would hold.
const SIZE: u32 = 24;

/// The slice inset, in pixels — the number the CSS `sliced()` rule repeats.
///
/// A third of [`SIZE`], so the corners are 8x8 and the centre that stretches is
/// the same again. Art narrower than twice this would have its corners overlap,
/// which the viewer's `image_backed_widgets` test is what catches.
const INSET: u32 = 8;

/// How wide the lit and shaded bevel edges are, in pixels.
const BEVEL: u32 = 2;

/// One widget surface's four tones: the face it is filled with, the lit edge,
/// the shaded edge, and the hairline frame around all of it.
#[derive(Debug, Clone, Copy)]
struct Surface {
    /// The flat centre.
    face: [u8; 4],
    /// The top and left edges — where the light comes from.
    light: [u8; 4],
    /// The bottom and right edges.
    shade: [u8; 4],
    /// The single-pixel frame around the whole tile.
    frame: [u8; 4],
}

/// Opaque, from three channels — the fourth is always 255 here: a widget
/// surface that let the panel through would defeat the point of drawing one.
const fn rgb(red: u8, green: u8, blue: u8) -> [u8; 4] {
    [red, green, blue, 255]
}

/// The four states the skin selects on, and the file each is written to.
///
/// Read them as a table: the hover row is the resting one lifted, and the
/// pressed row is the resting one with its **light and shade exchanged**, which
/// is the whole of what "sunken" means — the same geometry, lit from the other
/// side. The refused row has no bevel worth the name and every tone pulled
/// together, because a control that will not answer should not look like it is
/// waiting to.
const STATES: [(&str, Surface); 4] = [
    (
        "push-button",
        Surface {
            face: rgb(0x2a, 0x30, 0x38),
            light: rgb(0x59, 0x63, 0x7a),
            shade: rgb(0x10, 0x14, 0x1a),
            frame: rgb(0x05, 0x07, 0x0a),
        },
    ),
    (
        "push-button-hover",
        Surface {
            face: rgb(0x34, 0x3d, 0x49),
            light: rgb(0x6a, 0x76, 0x8f),
            shade: rgb(0x14, 0x19, 0x20),
            frame: rgb(0x05, 0x07, 0x0a),
        },
    ),
    (
        "push-button-pressed",
        Surface {
            face: rgb(0x24, 0x2a, 0x31),
            light: rgb(0x10, 0x14, 0x1a),
            shade: rgb(0x59, 0x63, 0x7a),
            frame: rgb(0x05, 0x07, 0x0a),
        },
    ),
    (
        "push-button-disabled",
        Surface {
            face: rgb(0x23, 0x28, 0x30),
            light: rgb(0x2d, 0x33, 0x3d),
            shade: rgb(0x1c, 0x20, 0x27),
            frame: rgb(0x0b, 0x0e, 0x12),
        },
    ),
];

/// Draw one surface.
///
/// Every pixel is decided by its distance to the nearest border: the outermost
/// ring is the frame, the next [`BEVEL`] rings are the lit or shaded edge, and
/// the rest is the face. Which of the two edges a pixel belongs to is decided
/// by the **anti-diagonal** it falls on, so the corners meet along a clean line
/// instead of one edge overpainting the other.
fn draw(surface: Surface) -> RgbaImage {
    let last = SIZE.saturating_sub(1);
    RgbaImage::from_fn(SIZE, SIZE, |x, y| {
        let edge = x
            .min(y)
            .min(last.saturating_sub(x))
            .min(last.saturating_sub(y));
        let colour = if edge == 0 {
            surface.frame
        } else if edge <= BEVEL {
            if x.saturating_add(y) < last {
                surface.light
            } else {
                surface.shade
            }
        } else {
            surface.face
        };
        Rgba(colour)
    })
}

/// Where the shipped art lives, relative to this crate.
fn shipped_widgets_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("sl-client-bevy-viewer")
        .join("assets")
        .join("skins")
        .join("graphite")
        .join("widgets")
}

/// Write every state's PNG into `into`, saying what each one came out as.
///
/// # Errors
///
/// If the directory cannot be created or a file cannot be written.
#[expect(
    clippy::print_stdout,
    reason = "a CLI binary writes its primary output to stdout"
)]
fn write_states(into: &Path) -> Result<(), image::ImageError> {
    fs_err::create_dir_all(into)?;
    for (name, surface) in STATES {
        let image = draw(surface);
        let path = into.join(format!("{name}.png"));
        image.save(&path)?;
        let corner = image.get_pixel(1, 1);
        let centre = image.get_pixel(SIZE / 2, SIZE / 2);
        // The inset is printed because it is the one number that has to agree
        // with something outside this crate: the `sliced()` in the rule that
        // points at the file.
        println!(
            "{}: {}x{} inset={INSET} corner={:?} centre={:?}",
            path.display(),
            image.width(),
            image.height(),
            corner.0,
            centre.0
        );
    }
    Ok(())
}

/// Draw the art, into the directory named on the command line or the shipped
/// one.
///
/// # Errors
///
/// If any file cannot be written.
fn main() -> Result<(), image::ImageError> {
    let into = std::env::args()
        .nth(1)
        .map_or_else(shipped_widgets_dir, PathBuf::from);
    write_states(&into)
}

#[cfg(test)]
mod tests {
    use super::{BEVEL, INSET, SIZE, STATES, draw};
    use pretty_assertions::{assert_eq, assert_ne};

    /// A boxed error so tests can use `?` instead of the disallowed
    /// `unwrap` / `expect`.
    type TestError = Box<dyn core::error::Error>;

    /// **The bevel is where the light says it is.**
    ///
    /// A surface whose lit and shaded edges came out the same — the mistake a
    /// careless edit to [`STATES`] makes — is a flat tile that still passes
    /// every "the file exists and is 24x24" check downstream.
    #[test]
    fn a_resting_button_is_lit_from_the_top_left() -> Result<(), TestError> {
        let (_name, resting) = STATES.first().copied().ok_or("the state table is empty")?;
        let image = draw(resting);
        let top_left = image.get_pixel(1, 1).0;
        let bottom_right = image
            .get_pixel(SIZE.saturating_sub(2), SIZE.saturating_sub(2))
            .0;
        assert_eq!(top_left, resting.light, "the top-left corner is not lit");
        assert_eq!(
            bottom_right, resting.shade,
            "the bottom-right corner is not shaded"
        );
        assert_ne!(
            top_left, bottom_right,
            "a bevel with one tone is not a bevel"
        );
        Ok(())
    }

    /// **Pressed is the same geometry lit from the other side.**
    ///
    /// Which is what makes the four files a *set* rather than four drawings: if
    /// the pressed row ever stopped being the resting one exchanged, the button
    /// would still look right at rest and wrong under the pointer.
    #[test]
    fn pressed_is_the_resting_surface_inverted() -> Result<(), TestError> {
        let (_resting_name, resting) = STATES.first().copied().ok_or("no resting row")?;
        let (_pressed_name, pressed) = STATES.get(2).copied().ok_or("no pressed row")?;
        assert_eq!(resting.light, pressed.shade);
        assert_eq!(resting.shade, pressed.light);
        Ok(())
    }

    /// **The frame, the bevel and the face each get their own band**, and the
    /// centre is wide enough to stretch.
    #[test]
    fn the_bands_are_the_widths_the_css_assumes() -> Result<(), TestError> {
        let (_name, resting) = STATES.first().copied().ok_or("the state table is empty")?;
        let image = draw(resting);
        assert_eq!(image.get_pixel(0, 0).0, resting.frame, "no hairline frame");
        assert_eq!(
            image
                .get_pixel(BEVEL.saturating_add(1), BEVEL.saturating_add(1))
                .0,
            resting.face,
            "the bevel is wider than BEVEL says"
        );
        assert!(
            SIZE >= INSET.saturating_mul(2),
            "the corners overlap: {SIZE} is narrower than two {INSET} px insets"
        );
        Ok(())
    }
}
