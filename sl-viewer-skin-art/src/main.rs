//! Draws the nine-sliced widget art an image-backed skin wears
//! (`viewer-skin-image-backed-widgets`, `viewer-vintage-skin`).
//!
//! A skin can change a widget's **shape**, not only its colour, by pointing a
//! CSS rule at a nine-sliced image: `graphite/themes/relief.css` dresses the
//! viewer's push button in a raised bevel that way, and the `vintage` skin
//! dresses every widget that carries a shape. This draws the files those rules
//! name.
//!
//! # Why the art has a generator at all
//!
//! Provenance, reproducibility and review. The PNG files are **ours** — only the
//! *geometry* is borrowed from the reference viewer (a lit edge where the light
//! is, a shaded one opposite, over a hairline frame), and geometry is not what
//! a licence covers; this file is where that is visible. The Vintage set is
//! drawn to the *measured* palette and banding recorded in
//! `roadmap/context/vintage-skin.md`, never by copying a pixel of the
//! reference's own textures, which are not ours to ship. A different size,
//! inset or palette is an edit to a constant rather than a fresh hand-drawn
//! set. And a diff of twenty PNG blobs says nothing, where a diff of a state
//! table says everything.
//!
//! It was a Python script first, which is the wrong shape for a tool that may
//! not run again for a year: this one is pinned by `Cargo.lock` and the
//! toolchain, and cannot quietly stop working while nobody is looking.
//!
//! # Running it
//!
//! Build and run this package with no arguments to redraw every shipped skin's
//! `widgets/` directory, or pass a directory to draw somewhere else — one
//! subdirectory per skin, named as the skin is.
//!
//! Every PNG is written with its `.meta` beside it, asking for a **nearest**
//! sampler: `ImagePlugin`'s default is linear, which blurs a one-pixel bevel
//! the moment the UI scale is anything but 1.0.
//!
//! It prints what it wrote, which makes a run self-checking: the corner of a
//! raised file reads lighter than its centre, and of a pressed file darker,
//! so the bevel inversion is visible as numbers.

use std::path::{Path, PathBuf};

use image::{Rgba, RgbaImage};

/// Opaque, from three channels — the fourth is always 255 here: a widget
/// surface that let the panel through would defeat the point of drawing one.
/// The one transparency the art has is outside a rounded corner, which
/// `draw` cuts from the coverage, not from the colour.
const fn rgb(red: u8, green: u8, blue: u8) -> [u8; 4] {
    [red, green, blue, 255]
}

/// The outermost ring of a surface, one colour per side.
///
/// Per side because the surfaces are not all framed alike: a Graphite button is
/// ringed in one hairline, a Vintage button sits in a groove that is dark above
/// and light below, a Vintage tab stands on a gold baseline, and a floater's
/// header strip carries only the rule under it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Frame {
    /// The top edge.
    top: [u8; 4],
    /// The left edge.
    left: [u8; 4],
    /// The bottom edge.
    bottom: [u8; 4],
    /// The right edge.
    right: [u8; 4],
}

impl Frame {
    /// One colour all round.
    const fn all(colour: [u8; 4]) -> Self {
        Self {
            top: colour,
            left: colour,
            bottom: colour,
            right: colour,
        }
    }

    /// One colour on the top and left edges, another on the bottom and right.
    const fn split(top_left: [u8; 4], bottom_right: [u8; 4]) -> Self {
        Self {
            top: top_left,
            left: top_left,
            bottom: bottom_right,
            right: bottom_right,
        }
    }
}

/// One widget surface: the face it is filled with, the two bevel tones, how
/// wide the bevel is, and the frame around all of it.
#[derive(Debug, Clone, Copy)]
struct Surface {
    /// The flat centre.
    face: [u8; 4],
    /// The top and left bevel bands — lit on a raised surface, shaded on a
    /// sunken one.
    light: [u8; 4],
    /// The bottom and right bevel bands.
    shade: [u8; 4],
    /// How far each corner is rounded, in pixels.
    corners: Corners,
    /// A vertical shading of the face, `(top, bottom)`, or `None` for a flat
    /// one. The face colour is the tone a third of the way down — where the
    /// reference's buttons and tabs are lightest — and the face runs from
    /// `top` to it and on down to `bottom`.
    gradient: Option<([u8; 4], [u8; 4])>,
    /// How wide the bevel bands are, in pixels; zero for a flat plate.
    bevel: u16,
    /// The single-pixel ring around the whole tile.
    frame: Frame,
}

/// How far each corner of a surface is rounded, in pixels — zero for a
/// square one.
///
/// Per corner because the shapes are not all rounded alike: a Vintage push
/// button is rounded all round, a tab only along the top it stands up from.
/// The frame and the bevel follow the curve, and its edge is anti-aliased, so
/// the panel behind shows through outside it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Corners {
    /// The top-left corner.
    top_left: u16,
    /// The top-right corner.
    top_right: u16,
    /// The bottom-right corner.
    bottom_right: u16,
    /// The bottom-left corner.
    bottom_left: u16,
}

impl Corners {
    /// No rounding.
    const SQUARE: Self = Self::all(0);

    /// The same radius on all four corners.
    const fn all(radius: u16) -> Self {
        Self {
            top_left: radius,
            top_right: radius,
            bottom_right: radius,
            bottom_left: radius,
        }
    }

    /// The radius of the corner in whose quadrant a point lies.
    fn at(self, x: f32, y: f32, size: f32) -> f32 {
        let left = x < size / 2.0;
        let top = y < size / 2.0;
        f32::from(match (top, left) {
            (true, true) => self.top_left,
            (true, false) => self.top_right,
            (false, false) => self.bottom_right,
            (false, true) => self.bottom_left,
        })
    }
}

/// One skin's art: where it goes, its geometry, and its states.
#[derive(Debug, Clone, Copy)]
struct ArtSet {
    /// The skin directory under `assets/skins/` the files belong to.
    skin: &'static str,
    /// The edge of each file, in pixels.
    ///
    /// Small on purpose: every pixel of a nine-slice is either a corner drawn
    /// 1:1 or an edge stretched along one axis, so the art carries no detail a
    /// larger canvas would hold.
    size: u32,
    /// The slice inset, in pixels — the number the CSS `sliced()` rules
    /// repeat. At least the frame plus the bevel, so no band is stretched
    /// across a corner, and at most half of [`ArtSet::size`], so the corners
    /// never overlap.
    inset: u32,
    /// Every file in the set, by name, and the surface drawn into it.
    states: &'static [(&'static str, Surface)],
}

/// Graphite's *Relief* theme: the four push-button states.
///
/// Read them as a table: the hover row is the resting one lifted, and the
/// pressed row is the resting one with its **light and shade exchanged**, which
/// is the whole of what "sunken" means — the same geometry, lit from the other
/// side. The refused row has no bevel worth the name and every tone pulled
/// together, because a control that will not answer should not look like it is
/// waiting to.
const GRAPHITE: ArtSet = ArtSet {
    skin: "graphite",
    size: 24,
    inset: 8,
    states: &[
        (
            "push-button",
            Surface {
                face: rgb(0x2a, 0x30, 0x38),
                light: rgb(0x59, 0x63, 0x7a),
                shade: rgb(0x10, 0x14, 0x1a),
                corners: Corners::SQUARE,
                gradient: None,
                bevel: 2,
                frame: Frame::all(rgb(0x05, 0x07, 0x0a)),
            },
        ),
        (
            "push-button-hover",
            Surface {
                face: rgb(0x34, 0x3d, 0x49),
                light: rgb(0x6a, 0x76, 0x8f),
                shade: rgb(0x14, 0x19, 0x20),
                corners: Corners::SQUARE,
                gradient: None,
                bevel: 2,
                frame: Frame::all(rgb(0x05, 0x07, 0x0a)),
            },
        ),
        (
            "push-button-pressed",
            Surface {
                face: rgb(0x24, 0x2a, 0x31),
                light: rgb(0x10, 0x14, 0x1a),
                shade: rgb(0x59, 0x63, 0x7a),
                corners: Corners::SQUARE,
                gradient: None,
                bevel: 2,
                frame: Frame::all(rgb(0x05, 0x07, 0x0a)),
            },
        ),
        (
            "push-button-disabled",
            Surface {
                face: rgb(0x23, 0x28, 0x30),
                light: rgb(0x2d, 0x33, 0x3d),
                shade: rgb(0x1c, 0x20, 0x27),
                corners: Corners::SQUARE,
                gradient: None,
                bevel: 2,
                frame: Frame::all(rgb(0x0b, 0x0e, 0x12)),
            },
        ),
    ],
};

/// The chrome grey every Vintage floater, menu bar and toolbar is (`DkGray`).
const VINTAGE_CHROME: [u8; 4] = rgb(0x3e, 0x3e, 0x3e);

/// Black at half strength over [`VINTAGE_CHROME`]: the hairline a Vintage
/// field is sunk behind.
const VINTAGE_FIELD_RIM: [u8; 4] = rgb(0x1f, 0x1f, 0x1f);

/// The warm gold a pressed or toggled Vintage button is framed in.
const VINTAGE_GOLD: [u8; 4] = rgb(0xff, 0xd7, 0x94);

/// The darker gold a Vintage tab stands on.
const VINTAGE_TAB_GOLD: [u8; 4] = rgb(0xf2, 0xaf, 0x37);

/// The Vintage push button — and every button that wears its art, the bottom
/// toolbar's and the action columns' — in a set of its own, because it is a
/// **stadium**: the reference's art rounds each end on a radius of half its
/// height, so its bottom bar reads as a row of pills. A 28 px file with a
/// 12 px radius and 13 px insets holds the whole curve in the corners drawn
/// 1:1; on a shorter button Bevy's slicer scales the corners down with it, so
/// the ends stay round rather than colliding.
///
/// The face is **shaded**, lightest a third of the way down and darkest at
/// the foot, inside a dark outline. Pressed or toggled it takes a **gold**
/// double ring and a darker face: a change of hue family, not a tint, which is
/// why the states are files and not colours.
const VINTAGE_BUTTONS: ArtSet = ArtSet {
    skin: "vintage",
    size: 28,
    inset: 13,
    states: &[
        (
            "push-button",
            Surface {
                face: rgb(0x64, 0x73, 0xbd),
                light: rgb(0x64, 0x73, 0xbd),
                shade: rgb(0x64, 0x73, 0xbd),
                corners: Corners::all(12),
                gradient: Some((rgb(0x6d, 0x7c, 0xc4), rgb(0x46, 0x58, 0xaa))),
                bevel: 0,
                frame: Frame::all(rgb(0x2b, 0x24, 0x20)),
            },
        ),
        (
            "push-button-hover",
            Surface {
                face: rgb(0x72, 0x82, 0xc8),
                light: rgb(0x72, 0x82, 0xc8),
                shade: rgb(0x72, 0x82, 0xc8),
                corners: Corners::all(12),
                gradient: Some((rgb(0x7b, 0x89, 0xcd), rgb(0x53, 0x64, 0xb4))),
                bevel: 0,
                frame: Frame::all(rgb(0x2b, 0x24, 0x20)),
            },
        ),
        (
            "push-button-pressed",
            Surface {
                face: rgb(0x51, 0x5d, 0x9a),
                light: rgb(0xd8, 0xbb, 0x90),
                shade: rgb(0xd3, 0xb1, 0x7a),
                corners: Corners::all(12),
                gradient: Some((rgb(0x5a, 0x66, 0xa2), rgb(0x39, 0x47, 0x8a))),
                bevel: 1,
                frame: Frame::all(VINTAGE_GOLD),
            },
        ),
        (
            // A LIT button under the pointer: still gold, and lifted. The
            // reference has no hover image at all — it draws whatever the
            // button currently wears with a quarter-strength additive glow — so
            // a hovered lit button is gold and brighter, never the plain hover.
            "push-button-selected-hover",
            Surface {
                face: rgb(0x5d, 0x69, 0xa6),
                light: rgb(0xe4, 0xcb, 0xa4),
                shade: rgb(0xdc, 0xc0, 0x8e),
                corners: Corners::all(12),
                gradient: Some((rgb(0x66, 0x72, 0xad), rgb(0x43, 0x51, 0x9a))),
                bevel: 1,
                frame: Frame::all(VINTAGE_GOLD),
            },
        ),
        (
            "push-button-disabled",
            Surface {
                face: rgb(0x36, 0x3b, 0x54),
                light: rgb(0x36, 0x3b, 0x54),
                shade: rgb(0x36, 0x3b, 0x54),
                corners: Corners::all(12),
                gradient: Some((rgb(0x33, 0x37, 0x4f), rgb(0x32, 0x37, 0x4e))),
                bevel: 0,
                frame: Frame::all(rgb(0x27, 0x2a, 0x3d)),
            },
        ),
    ],
};

/// A Vintage tab's corners: barely cut at the leading top, rounded at the
/// trailing top, square along the foot it stands on — the reference draws
/// every tab from its *left* tab's art, and that is its shape.
const TAB_CORNERS: Corners = Corners {
    top_left: 2,
    top_right: 5,
    bottom_right: 0,
    bottom_left: 0,
};

/// The Vintage tab: shaded mauve on a gold baseline at rest, dark violet in a
/// gold frame when it is the open one. Its own set for the 5 px corner.
const VINTAGE_TABS: ArtSet = ArtSet {
    skin: "vintage",
    size: 16,
    inset: 6,
    states: &[
        (
            "tab",
            Surface {
                face: rgb(0x72, 0x69, 0x77),
                light: rgb(0x72, 0x69, 0x77),
                shade: rgb(0x72, 0x69, 0x77),
                corners: TAB_CORNERS,
                gradient: Some((rgb(0x7f, 0x75, 0x84), rgb(0x5c, 0x54, 0x62))),
                bevel: 0,
                frame: Frame {
                    top: rgb(0x2a, 0x26, 0x2c),
                    left: rgb(0x2a, 0x26, 0x2c),
                    bottom: VINTAGE_TAB_GOLD,
                    right: rgb(0x2a, 0x26, 0x2c),
                },
            },
        ),
        (
            "tab-selected",
            Surface {
                face: rgb(0x52, 0x37, 0x5e),
                light: rgb(0xb8, 0x8a, 0x47),
                shade: rgb(0xb8, 0x8a, 0x47),
                corners: TAB_CORNERS,
                gradient: Some((rgb(0x61, 0x49, 0x60), rgb(0x52, 0x37, 0x5e))),
                bevel: 1,
                frame: Frame::all(VINTAGE_TAB_GOLD),
            },
        ),
    ],
};

/// The two round Vintage handles: the scrollbar thumb, a stadium in the
/// reference's flat blue, and the slider handle, a shaded disc. Sized for a
/// 14 px bar and a 10 px handle, which the slicer scales them to.
const VINTAGE_HANDLES: ArtSet = ArtSet {
    skin: "vintage",
    size: 12,
    inset: 6,
    states: &[
        (
            "scroll-thumb",
            Surface {
                face: rgb(0x3c, 0x4c, 0x7c),
                light: rgb(0x3c, 0x4c, 0x7c),
                shade: rgb(0x3c, 0x4c, 0x7c),
                corners: Corners::all(5),
                gradient: None,
                bevel: 0,
                frame: Frame::all(rgb(0x3c, 0x4c, 0x7c)),
            },
        ),
        (
            "scroll-thumb-hover",
            Surface {
                face: rgb(0x4c, 0x5e, 0x94),
                light: rgb(0x4c, 0x5e, 0x94),
                shade: rgb(0x4c, 0x5e, 0x94),
                corners: Corners::all(5),
                gradient: None,
                bevel: 0,
                frame: Frame::all(rgb(0x4c, 0x5e, 0x94)),
            },
        ),
        (
            "slider-thumb",
            Surface {
                face: rgb(0x72, 0x6c, 0x76),
                light: rgb(0x72, 0x6c, 0x76),
                shade: rgb(0x72, 0x6c, 0x76),
                corners: Corners::all(5),
                gradient: Some((rgb(0x7a, 0x74, 0x80), rgb(0x3f, 0x3d, 0x60))),
                bevel: 0,
                frame: Frame::all(rgb(0x25, 0x22, 0x1f)),
            },
        ),
        (
            "slider-thumb-disabled",
            Surface {
                face: rgb(0x57, 0x54, 0x59),
                light: rgb(0x57, 0x54, 0x59),
                shade: rgb(0x57, 0x54, 0x59),
                corners: Corners::all(5),
                gradient: Some((rgb(0x5d, 0x5a, 0x5f), rgb(0x3e, 0x3d, 0x4e))),
                bevel: 0,
                frame: Frame::all(rgb(0x31, 0x2f, 0x2c)),
            },
        ),
    ],
};

/// The Vintage skin: every other surface that carries a shape.
///
/// Measured, then drawn — the faces are the centre pixels the context file
/// records, the bands are the reference's one-pixel frame and one-pixel bevel,
/// and the rest is ours. Two things to read out of it:
///
/// - **Fields are sunken**: a dark rim, shaded above and lit below, on the
///   light sage face that is the whole visual argument of the skin. A
///   read-only field goes back to chrome grey — the surface, not a greyed
///   glyph, says "you cannot type here".
/// - **These are square**, as they are there: fields, the tooltip, the scroll
///   arrows (a dark grey face for a light arrow glyph) and the floater. Only
///   the slider groove is rounded, and slightly.
const VINTAGE: ArtSet = ArtSet {
    skin: "vintage",
    size: 12,
    inset: 4,
    states: &[
        (
            "field",
            Surface {
                face: rgb(0xba, 0xc3, 0xbe),
                light: rgb(0x96, 0x9d, 0x99),
                shade: rgb(0xe6, 0xe8, 0xe3),
                corners: Corners::SQUARE,
                gradient: None,
                bevel: 1,
                frame: Frame::all(VINTAGE_FIELD_RIM),
            },
        ),
        (
            "field-focused",
            Surface {
                face: rgb(0xc8, 0xd1, 0xcc),
                light: rgb(0xa0, 0xa8, 0xa4),
                shade: rgb(0xf3, 0xf5, 0xf0),
                corners: Corners::SQUARE,
                gradient: None,
                bevel: 1,
                frame: Frame::all(VINTAGE_FIELD_RIM),
            },
        ),
        (
            "field-readonly",
            Surface {
                face: VINTAGE_CHROME,
                light: rgb(0x2b, 0x2b, 0x2b),
                shade: rgb(0x73, 0x84, 0x9b),
                corners: Corners::SQUARE,
                gradient: None,
                bevel: 1,
                frame: Frame::all(VINTAGE_FIELD_RIM),
            },
        ),
        (
            "field-disabled",
            Surface {
                face: VINTAGE_CHROME,
                light: rgb(0x1a, 0x1a, 0x1a),
                shade: rgb(0x73, 0x84, 0x9b),
                corners: Corners::SQUARE,
                gradient: None,
                bevel: 1,
                frame: Frame::all(VINTAGE_CHROME),
            },
        ),
        (
            "scroll-arrow",
            Surface {
                face: rgb(0x68, 0x68, 0x68),
                light: rgb(0x68, 0x68, 0x68),
                shade: rgb(0x68, 0x68, 0x68),
                corners: Corners::SQUARE,
                gradient: Some((rgb(0x6c, 0x6c, 0x6c), rgb(0x62, 0x62, 0x62))),
                bevel: 0,
                frame: Frame {
                    top: rgb(0x72, 0x72, 0x72),
                    left: rgb(0x4a, 0x4a, 0x4a),
                    bottom: rgb(0x41, 0x41, 0x41),
                    right: rgb(0x5a, 0x5a, 0x5a),
                },
            },
        ),
        (
            "scroll-arrow-hover",
            Surface {
                face: rgb(0x74, 0x74, 0x74),
                light: rgb(0x74, 0x74, 0x74),
                shade: rgb(0x74, 0x74, 0x74),
                corners: Corners::SQUARE,
                gradient: Some((rgb(0x78, 0x78, 0x78), rgb(0x6c, 0x6c, 0x6c))),
                bevel: 0,
                frame: Frame {
                    top: rgb(0x80, 0x80, 0x80),
                    left: rgb(0x4a, 0x4a, 0x4a),
                    bottom: rgb(0x41, 0x41, 0x41),
                    right: rgb(0x5a, 0x5a, 0x5a),
                },
            },
        ),
        (
            "scroll-arrow-pressed",
            Surface {
                face: rgb(0x5a, 0x5a, 0x5a),
                light: rgb(0x5a, 0x5a, 0x5a),
                shade: rgb(0x5a, 0x5a, 0x5a),
                corners: Corners::SQUARE,
                gradient: Some((rgb(0x55, 0x55, 0x55), rgb(0x5e, 0x5e, 0x5e))),
                bevel: 0,
                frame: Frame::all(rgb(0x41, 0x41, 0x41)),
            },
        ),
        (
            "slider-track",
            Surface {
                face: rgb(0x64, 0x6e, 0x9d),
                light: rgb(0x45, 0x4e, 0x70),
                shade: rgb(0x7c, 0x86, 0xb5),
                corners: Corners::all(2),
                gradient: None,
                bevel: 1,
                frame: Frame::split(rgb(0x2e, 0x29, 0x23), rgb(0x68, 0x68, 0x68)),
            },
        ),
        (
            "floater",
            Surface {
                face: VINTAGE_CHROME,
                light: VINTAGE_CHROME,
                shade: VINTAGE_CHROME,
                corners: Corners::SQUARE,
                gradient: None,
                bevel: 0,
                frame: Frame::all(rgb(0x00, 0x00, 0x00)),
            },
        ),
        (
            // Only the rule under it: the header sits inside the floater's own
            // hairline, so a frame all round would draw that line twice.
            "floater-header",
            Surface {
                face: rgb(0x55, 0x55, 0x55),
                light: rgb(0x55, 0x55, 0x55),
                shade: rgb(0x55, 0x55, 0x55),
                corners: Corners::SQUARE,
                gradient: None,
                bevel: 0,
                frame: Frame {
                    top: rgb(0x55, 0x55, 0x55),
                    left: rgb(0x55, 0x55, 0x55),
                    bottom: rgb(0x00, 0x00, 0x00),
                    right: rgb(0x55, 0x55, 0x55),
                },
            },
        ),
        (
            "tooltip",
            Surface {
                face: rgb(0xb7, 0xb8, 0xbc),
                light: rgb(0xb7, 0xb8, 0xbc),
                shade: rgb(0xb7, 0xb8, 0xbc),
                corners: Corners::SQUARE,
                gradient: None,
                bevel: 0,
                frame: Frame::split(rgb(0x9d, 0x9e, 0xa2), rgb(0x5b, 0x5c, 0x5f)),
            },
        ),
    ],
};

/// Every set this tool draws.
const SETS: [ArtSet; 5] = [
    GRAPHITE,
    VINTAGE_BUTTONS,
    VINTAGE_TABS,
    VINTAGE_HANDLES,
    VINTAGE,
];

/// The `.meta` every PNG ships beside it: the image loader, sRGB, and a
/// **nearest** sampler, so a one-pixel band stays one crisp pixel at any UI
/// scale. Get one letter of this RON wrong and Bevy ignores it in silence,
/// which the viewer's `image_backed_widgets` test is what catches.
const NEAREST_META: &str = r#"(
    meta_format_version: "1.0",
    asset: Load(
        loader: "bevy_image::image_loader::ImageLoader",
        settings: (
            format: FromExtension,
            is_srgb: true,
            sampler: Descriptor((
                label: None,
                address_mode_u: ClampToEdge,
                address_mode_v: ClampToEdge,
                address_mode_w: ClampToEdge,
                mag_filter: Nearest,
                min_filter: Nearest,
                mipmap_filter: Nearest,
                lod_min_clamp: 0.0,
                lod_max_clamp: 32.0,
                compare: None,
                anisotropy_clamp: 1,
                border_color: None,
            )),
            asset_usage: ("MAIN_WORLD | RENDER_WORLD"),
        ),
    ),
)
"#;

/// Which frame side a pixel on the outermost ring belongs to.
///
/// The ring is split along the **anti-diagonal** first — the top-left half
/// against the bottom-right — exactly as the bevel is, so a split frame and
/// its bevel turn their corners on one line; within each half, the nearer
/// edge wins.
const fn frame_colour(frame: Frame, x: u32, y: u32, last: u32) -> [u8; 4] {
    if x.saturating_add(y) < last {
        if y <= x { frame.top } else { frame.left }
    } else if last.saturating_sub(y) <= last.saturating_sub(x) {
        frame.bottom
    } else {
        frame.right
    }
}

/// How many sub-samples along each pixel edge the anti-aliased outline of a
/// rounded corner is measured at: sixteen per pixel, enough that the curve's
/// alpha ramps smoothly rather than in visible steps.
const SUPERSAMPLE: u16 = 4;

/// A pixel coordinate as a float, for the distance maths. The art is a few
/// dozen pixels across; anything past `u16` is not a size this draws.
fn coord(value: u32) -> f32 {
    u16::try_from(value).map_or(f32::MAX, f32::from)
}

/// How far the point `(x, y)` lies **inside** the outline of a `size`-pixel
/// square with `corners` rounded — its distance to the nearest edge, or to the
/// corner's arc where it sits in a rounded corner. Zero on the outline and
/// negative outside it.
fn inside_distance(corners: Corners, x: f32, y: f32, size: f32) -> f32 {
    let radius = corners.at(x, y, size);
    let across = x.min(size - x);
    let down = y.min(size - y);
    if across < radius && down < radius {
        radius - (radius - across).hypot(radius - down)
    } else {
        across.min(down)
    }
}

/// How many of a pixel's sub-samples fall inside the outline, out of
/// `SUPERSAMPLE` squared.
fn coverage(corners: Corners, x: f32, y: f32, size: f32) -> u16 {
    let step = f32::from(SUPERSAMPLE);
    let mut covered: u16 = 0;
    for row in 0..SUPERSAMPLE {
        for column in 0..SUPERSAMPLE {
            let sample_x = x + (f32::from(column) + 0.5) / step;
            let sample_y = y + (f32::from(row) + 0.5) / step;
            if inside_distance(corners, sample_x, sample_y, size) > 0.0 {
                covered = covered.saturating_add(1);
            }
        }
    }
    covered
}

/// Mix `from` and `to`, `weight` parts of `to` in `total`, channel by channel.
fn mix(from: [u8; 4], to: [u8; 4], weight: u32, total: u32) -> [u8; 4] {
    let keep = total.saturating_sub(weight);
    let mut out = from;
    for (channel, (&start, &end)) in out.iter_mut().zip(from.iter().zip(to.iter())) {
        let blended = u32::from(start)
            .saturating_mul(keep)
            .saturating_add(u32::from(end).saturating_mul(weight))
            .checked_div(total)
            .unwrap_or_else(|| u32::from(start));
        *channel = u8::try_from(blended).unwrap_or(u8::MAX);
    }
    out
}

/// The face colour on row `y` of a `size`-row surface: flat, or shaded from
/// the gradient's top through the face (a third of the way down) to its
/// bottom. Measured in half-pixels so each row takes the tone at its centre.
fn face_at(surface: Surface, y: u32, size: u32) -> [u8; 4] {
    let Some((top, bottom)) = surface.gradient else {
        return surface.face;
    };
    let position = y.saturating_mul(2).saturating_add(1);
    let total = size.saturating_mul(2);
    let knee = total / 3;
    if position < knee {
        mix(top, surface.face, position, knee)
    } else {
        mix(
            surface.face,
            bottom,
            position.saturating_sub(knee),
            total.saturating_sub(knee),
        )
    }
}

/// Draw one surface on a `size`-pixel square.
///
/// Every pixel is decided by how deep inside the outline its centre lies: the
/// outermost pixel's depth is the frame, the next [`Surface::bevel`] are the
/// lit or shaded band, and the rest is the face. Which band a pixel belongs to
/// is decided by the **anti-diagonal** it falls on, so the corners meet along a
/// clean line instead of one edge overpainting the other. On a square surface
/// that is exactly the distance to the nearest border; on a rounded one the
/// frame and the bevel follow the curve, and the pixels the curve cuts through
/// take its coverage as their alpha.
fn draw(surface: Surface, size: u32) -> RgbaImage {
    let last = size.saturating_sub(1);
    let extent = coord(size);
    let bevel = f32::from(surface.bevel);
    let samples = SUPERSAMPLE.saturating_mul(SUPERSAMPLE);
    RgbaImage::from_fn(size, size, |x, y| {
        let (left, top) = (coord(x), coord(y));
        let depth = inside_distance(surface.corners, left + 0.5, top + 0.5, extent);
        let [red, green, blue, opacity] = if depth < 1.0 {
            frame_colour(surface.frame, x, y, last)
        } else if depth < 1.0 + bevel {
            if x.saturating_add(y) < last {
                surface.light
            } else {
                surface.shade
            }
        } else {
            face_at(surface, y, size)
        };
        let covered = coverage(surface.corners, left, top, extent);
        let alpha = u16::from(opacity)
            .saturating_mul(covered)
            .checked_div(samples)
            .and_then(|alpha| u8::try_from(alpha).ok())
            .unwrap_or(opacity);
        Rgba([red, green, blue, alpha])
    })
}

/// Where the shipped art for `skin` lives, relative to this crate.
fn shipped_widgets_dir(skin: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("sl-client-bevy-viewer")
        .join("assets")
        .join("skins")
        .join(skin)
        .join("widgets")
}

/// Write every state's PNG and `.meta` into `into`, saying what each one came
/// out as.
///
/// # Errors
///
/// If the directory cannot be created or a file cannot be written.
#[expect(
    clippy::print_stdout,
    reason = "a CLI binary writes its primary output to stdout"
)]
fn write_set(set: ArtSet, into: &Path) -> Result<(), image::ImageError> {
    fs_err::create_dir_all(into)?;
    for &(name, surface) in set.states {
        let image = draw(surface, set.size);
        let path = into.join(format!("{name}.png"));
        image.save(&path)?;
        fs_err::write(into.join(format!("{name}.png.meta")), NEAREST_META)?;
        let corner = image.get_pixel(1, 1);
        let centre = image.get_pixel(set.size / 2, set.size / 2);
        // The inset is printed because it is the one number that has to agree
        // with something outside this crate: the `sliced()` in the rule that
        // points at the file.
        println!(
            "{}: {}x{} inset={} corner={:?} centre={:?}",
            path.display(),
            image.width(),
            image.height(),
            set.inset,
            corner.0,
            centre.0
        );
    }
    Ok(())
}

/// Draw the art, into the directory named on the command line (one
/// subdirectory per skin) or into each skin's shipped `widgets/`.
///
/// # Errors
///
/// If any file cannot be written.
fn main() -> Result<(), image::ImageError> {
    let root = std::env::args().nth(1).map(PathBuf::from);
    for set in SETS {
        let into = root
            .as_ref()
            .map_or_else(|| shipped_widgets_dir(set.skin), |root| root.join(set.skin));
        write_set(set, &into)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        ArtSet, Corners, Frame, GRAPHITE, SETS, Surface, VINTAGE, VINTAGE_BUTTONS, draw, face_at,
    };
    use pretty_assertions::{assert_eq, assert_ne};

    /// A boxed error so tests can use `?` instead of the disallowed
    /// `unwrap` / `expect`.
    type TestError = Box<dyn core::error::Error>;

    /// A named state of a set.
    fn state(set: ArtSet, name: &str) -> Result<Surface, TestError> {
        set.states
            .iter()
            .find(|(candidate, _surface)| *candidate == name)
            .map(|&(_name, surface)| surface)
            .ok_or_else(|| format!("{} has no `{name}` state", set.skin).into())
    }

    /// **The bevel is where the light says it is.**
    ///
    /// A surface whose lit and shaded edges came out the same — the mistake a
    /// careless edit to a state table makes — is a flat tile that still passes
    /// every "the file exists and is the right size" check downstream.
    #[test]
    fn a_resting_button_is_lit_from_the_top_left() -> Result<(), TestError> {
        let set = GRAPHITE;
        let resting = state(set, "push-button")?;
        let image = draw(resting, set.size);
        // Mid-edge, not the corner pixel: a rounded button's corner is the
        // curve, and the bevel is what runs along the straight edges.
        let top_left = image.get_pixel(set.size / 2, 1).0;
        let bottom_right = image.get_pixel(set.size / 2, set.size.saturating_sub(2)).0;
        assert_eq!(top_left, resting.light, "{}: not lit", set.skin);
        assert_eq!(bottom_right, resting.shade, "{}: not shaded", set.skin);
        assert_ne!(
            top_left, bottom_right,
            "{}: a bevel with one tone is not a bevel",
            set.skin
        );
        Ok(())
    }

    /// **Graphite's pressed button is the same geometry lit from the other
    /// side.**
    ///
    /// Which is what makes its four files a *set* rather than four drawings: if
    /// the pressed row ever stopped being the resting one exchanged, the button
    /// would still look right at rest and wrong under the pointer. (Vintage's
    /// pressed button is a different hue family altogether, which is the next
    /// test.)
    #[test]
    fn pressed_is_the_resting_surface_inverted() -> Result<(), TestError> {
        let resting = state(GRAPHITE, "push-button")?;
        let pressed = state(GRAPHITE, "push-button-pressed")?;
        assert_eq!(resting.light, pressed.shade);
        assert_eq!(resting.shade, pressed.light);
        Ok(())
    }

    /// **A pressed Vintage button is framed in gold**, and a resting one is
    /// not — the reference changes hue family on a press, which is the one
    /// thing a Vintage button is recognised by.
    #[test]
    fn a_pressed_vintage_button_turns_gold() -> Result<(), TestError> {
        let size = VINTAGE_BUTTONS.size;
        let resting = draw(state(VINTAGE_BUTTONS, "push-button")?, size);
        let pressed = draw(state(VINTAGE_BUTTONS, "push-button-pressed")?, size);
        let gold = [0xff, 0xd7, 0x94, 0xff];
        assert_eq!(pressed.get_pixel(0, size / 2).0, gold);
        assert_eq!(pressed.get_pixel(size / 2, 0).0, gold);
        assert_ne!(resting.get_pixel(0, size / 2).0, gold);
        Ok(())
    }

    /// **A Vintage field is light and sunken** — a dark rim, shaded above,
    /// lit below, and a light face — which is the skin's whole argument: data
    /// on light sage inside dark grey chrome.
    #[test]
    fn a_vintage_field_is_a_light_well() -> Result<(), TestError> {
        let field = state(VINTAGE, "field")?;
        let image = draw(field, VINTAGE.size);
        let luminance = |pixel: [u8; 4]| -> u32 {
            let [red, green, blue, _alpha] = pixel;
            u32::from(red)
                .saturating_add(u32::from(green))
                .saturating_add(u32::from(blue))
        };
        let face = image.get_pixel(VINTAGE.size / 2, VINTAGE.size / 2).0;
        let above = image.get_pixel(VINTAGE.size / 2, 1).0;
        let below = image
            .get_pixel(VINTAGE.size / 2, VINTAGE.size.saturating_sub(2))
            .0;
        assert!(
            luminance(face) > 3 * 0x90,
            "the face is not light: {face:?}"
        );
        assert!(luminance(above) < luminance(face), "not shaded above");
        assert!(luminance(below) > luminance(face), "not lit below");
        Ok(())
    }

    /// **A Vintage button is a shaded stadium**: its ends are round on a
    /// radius of about half its height (the reference's bottom bar is a row
    /// of pills), and its face is lighter near the top than at the foot.
    #[test]
    fn a_vintage_button_is_a_shaded_stadium() -> Result<(), TestError> {
        let button = state(VINTAGE_BUTTONS, "push-button")?;
        let size = VINTAGE_BUTTONS.size;
        let image = draw(button, size);
        let alpha = |x, y| image.get_pixel(x, y).0.last().copied().unwrap_or(0);
        let radius = u32::from(button.corners.top_left);
        assert!(
            radius.saturating_mul(2) >= size.saturating_sub(4),
            "a {radius} px corner on a {size} px button is a rounded box, not a pill"
        );
        assert_eq!(alpha(1, 3), 0, "the end is not rounded away at the top");
        let luminance = |pixel: [u8; 4]| -> u32 {
            let [red, green, blue, _alpha] = pixel;
            u32::from(red)
                .saturating_add(u32::from(green))
                .saturating_add(u32::from(blue))
        };
        let upper = image.get_pixel(size / 2, size / 3).0;
        let foot = image.get_pixel(size / 2, size.saturating_sub(3)).0;
        assert!(
            luminance(upper) > luminance(foot),
            "the face is not shaded: {upper:?} above {foot:?}"
        );
        Ok(())
    }

    /// **A rounded corner is cut away, and a square one is not.**
    ///
    /// The reference's Vintage push button is rounded on a six-pixel radius,
    /// which is what makes its bottom bar a row of pills; a square button in
    /// its place is the first thing an eye comparing the two notices. The
    /// corner pixel is fully clear, the edge between is part-covered, and the
    /// face is opaque; and every square surface in every set comes out opaque
    /// edge to edge, so the rounding cannot leak into art that has none.
    #[test]
    fn a_rounded_corner_is_cut_and_a_square_one_is_not() -> Result<(), TestError> {
        let button = state(VINTAGE_BUTTONS, "push-button")?;
        let image = draw(button, VINTAGE_BUTTONS.size);
        let alpha = |x, y| image.get_pixel(x, y).0.last().copied().unwrap_or(0);
        assert_eq!(alpha(0, 0), 0, "the corner pixel is not cut away");
        let part_covered = (0..6_u32)
            .flat_map(|y| (0..6_u32).map(move |x| (x, y)))
            .map(|(x, y)| alpha(x, y))
            .any(|value| value > 0 && value < 255);
        assert!(part_covered, "the curve is not anti-aliased");
        assert_eq!(
            alpha(VINTAGE_BUTTONS.size / 2, 0),
            255,
            "the straight edge is not opaque"
        );

        for set in SETS {
            for &(name, surface) in set.states {
                if surface.corners != Corners::SQUARE {
                    continue;
                }
                let image = draw(surface, set.size);
                assert!(
                    image.pixels().all(|pixel| pixel.0.last() == Some(&255)),
                    "{}/{name} is square but not opaque",
                    set.skin
                );
            }
        }
        Ok(())
    }

    /// **A split frame turns its corners on the bevel's diagonal.**
    ///
    /// The top-right and bottom-left corner pixels are the two a per-side
    /// frame has to decide; the rule is the anti-diagonal, so each goes to
    /// the half the bevel beside it belongs to.
    #[test]
    fn a_split_frame_meets_on_the_anti_diagonal() {
        let dark = [0, 0, 0, 255];
        let light = [255, 255, 255, 255];
        let surface = Surface {
            face: [128, 128, 128, 255],
            light: dark,
            shade: light,
            corners: Corners::SQUARE,
            gradient: None,
            bevel: 1,
            frame: Frame::split(dark, light),
        };
        let image = draw(surface, 8);
        assert_eq!(image.get_pixel(0, 0).0, dark, "top-left");
        assert_eq!(image.get_pixel(7, 7).0, light, "bottom-right");
        assert_eq!(
            image.get_pixel(7, 0).0,
            light,
            "top-right is past the diagonal"
        );
        assert_eq!(
            image.get_pixel(0, 6).0,
            dark,
            "left edge above the diagonal"
        );
    }

    /// **The frame, the bevel and the face each get their own band**, and the
    /// insets fit the art: wide enough to hold frame and bevel in a corner,
    /// narrow enough that two corners never overlap.
    #[test]
    fn the_bands_are_the_widths_the_css_assumes() -> Result<(), TestError> {
        for set in SETS {
            assert!(
                set.size >= set.inset.saturating_mul(2),
                "{}: the corners overlap: {} is narrower than two {} px insets",
                set.skin,
                set.size,
                set.inset
            );
            for &(name, surface) in set.states {
                assert!(
                    set.inset > u32::from(surface.bevel),
                    "{}/{name}: the bevel runs past the inset, so it stretches",
                    set.skin
                );
                let widest = surface
                    .corners
                    .top_left
                    .max(surface.corners.top_right)
                    .max(surface.corners.bottom_right)
                    .max(surface.corners.bottom_left);
                assert!(
                    set.inset > u32::from(widest),
                    "{}/{name}: the corner's curve runs past the inset, so it \
                     would be stretched along the edge",
                    set.skin
                );
                let image = draw(surface, set.size);
                let past_bevel = u32::from(surface.bevel).saturating_add(1);
                assert_eq!(
                    image.get_pixel(set.size / 2, past_bevel).0,
                    face_at(surface, past_bevel, set.size),
                    "{}/{name}: the bevel is wider than it says",
                    set.skin
                );
            }
        }
        let resting = state(GRAPHITE, "push-button")?;
        let image = draw(resting, GRAPHITE.size);
        assert_eq!(
            image.get_pixel(0, 0).0,
            resting.frame.top,
            "no hairline frame"
        );
        Ok(())
    }
}
