//! The maths half of the 360° panorama ([`super`]): six square cube-map faces
//! in, one **equirectangular** (2:1 lat/long) image out.
//!
//! Nothing here touches the ECS, a GPU or a file — it is the part of the
//! capture that can be, and is, tested on synthetic cubes.
//!
//! # The frame the cube is built in
//!
//! The six faces are defined in the **capture frame**, not in world axes: `+X`
//! is the capture's right, `+Y` its up, and `-Z` the direction the camera was
//! facing when the shutter fired (Bevy's camera convention, because that frame
//! *is* the camera's — [`CubeFace::rotation`] is the rotation handed to the
//! viewer camera to shoot that face).
//!
//! That is the one decision the rest of this module falls out of. Firestorm
//! shoots its six faces along **world** axes and leaves the camera's heading to
//! the metadata, so its panorama's seam lands wherever east happens to be. Ours
//! is built around the shot the photographer composed: the **centre column of
//! the image is the direction the camera was pointing**, so a viewer that opens
//! on the middle of the panorama opens on the composed view, and the seam — the
//! one place a stitch can show — falls directly *behind* the camera, which is
//! the least-looked-at direction in the frame. The compass heading of that
//! centre is not lost: it is written into the XMP as
//! `GPano:PoseHeadingDegrees` ([`super::xmp`]).
//!
//! Because the faces and the output share that frame, the reprojection needs no
//! second basis and no world→camera change: [`direction_for`] produces a ray in
//! the same frame [`face_uv`] consumes.
//!
//! # Seams, poles, and why the sampling looks the way it does
//!
//! A naive six-shots-and-stitch looks wrong in two places, and each has its own
//! answer here:
//!
//! - **The seams between faces.** A bilinear tap near a face edge wants texels
//!   from the *neighbouring* face, which a per-face image cannot offer. The tap
//!   is therefore clamped to its own face, which blurs the joint by at most half
//!   a texel — a filtering difference, not a content one, because the two faces
//!   photographed the same continuous scene from the same eye point. (This is
//!   what a GL cube map without seamless filtering does, which is the path the
//!   reference viewer's WebGL stitcher takes.)
//! - **The poles, and the equator.** The mapping is wildly anisotropic: an
//!   output pixel at the zenith covers a sliver of the cube, one at a face
//!   centre may cover several texels. The second is the aliasing one, so each
//!   output pixel is **supersampled** — [`supersamples`] picks the grid from the
//!   ratio between the cube's angular resolution and the output's, so a capture
//!   shot at twice the output's resolution is averaged rather than point-sampled.
//!
//! Every average is taken in **linear light**: the faces
//! are tone-mapped 8-bit sRGB, and averaging sRGB code values darkens every
//! high-contrast edge in the panorama (the classic half-black-half-white pair
//! averaging to 128 instead of 188).
//!
//! Reference (Firestorm, read-only):
//! `skins/default/html/common/equirectangular/js/CubemapToEquirectangular.js`
//! (the fragment shader that does this on the GPU, from
//! `THREE.CubemapToEquirectangular`).

use core::f32::consts::{FRAC_PI_2, PI, TAU};
use std::sync::LazyLock;

use bevy::math::{Quat, Vec2, Vec3};
use bevy::tasks::{ComputeTaskPool, TaskPool};
use image::{Rgb, RgbImage};

/// The smallest cube-face edge that can be reprojected at all: a bilinear tap
/// needs two texels to interpolate between.
const MIN_FACE_SIZE: u32 = 2;

/// The largest cube-face edge accepted, and the largest output width. Both are
/// held inside `u16` so every pixel coordinate converts to `f32` exactly and
/// without a cast (see `coord_to_f32`).
pub const MAX_PANORAMA_SIZE: u32 = 8192;

/// How many rows of the output one parallel band holds. Large enough that the
/// per-band overhead disappears, small enough that a 2048-row panorama still
/// splits across every core.
const BAND_ROWS: u32 = 16;

/// The most sub-samples per axis [`supersamples`] will ask for. Four means
/// sixteen taps per output pixel, which is where the returns stop being visible
/// against a cube that is only ever a few times the output's resolution.
const MAX_SUPERSAMPLES: u32 = 4;

// ---------------------------------------------------------------------------
// Faces.
// ---------------------------------------------------------------------------

/// One face of the capture cube, named for where it lies in the **capture
/// frame** (see [the module docs](self)).
///
/// The order of the variants is the order the faces are shot in, which is also
/// the order [`CubeFaces::from_faces`] expects them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CubeFace {
    /// Straight ahead: the direction the camera was facing.
    Front,
    /// The capture's right.
    Right,
    /// Directly behind the camera — where the panorama's seam falls.
    Back,
    /// The capture's left.
    Left,
    /// The zenith.
    Up,
    /// The nadir.
    Down,
}

/// Every face, in shooting order.
pub const CUBE_FACES: [CubeFace; 6] = [
    CubeFace::Front,
    CubeFace::Right,
    CubeFace::Back,
    CubeFace::Left,
    CubeFace::Up,
    CubeFace::Down,
];

impl CubeFace {
    /// The camera rotation that shoots this face, in the capture frame.
    ///
    /// This is the authoritative definition of the face: [`Self::direction`]
    /// and [`face_uv`] are both derived from it, so the rotation the camera was
    /// actually given and the rotation the reprojection samples through cannot
    /// drift apart.
    #[must_use]
    pub fn rotation(self) -> Quat {
        match self {
            Self::Front => Quat::IDENTITY,
            Self::Right => Quat::from_rotation_y(-FRAC_PI_2),
            Self::Back => Quat::from_rotation_y(PI),
            Self::Left => Quat::from_rotation_y(FRAC_PI_2),
            Self::Up => Quat::from_rotation_x(FRAC_PI_2),
            Self::Down => Quat::from_rotation_x(-FRAC_PI_2),
        }
    }

    /// The unit direction this face looks along, in the capture frame.
    #[must_use]
    pub fn direction(self) -> Vec3 {
        // `mul_vec3` rather than the `*` operator: the workspace's
        // `arithmetic_side_effects` lint fires on glam's overloaded operators.
        self.rotation().mul_vec3(Vec3::NEG_Z)
    }

    /// A short, file-name-safe name — what a debug dump of the six faces calls
    /// them.
    #[must_use]
    pub const fn slug(self) -> &'static str {
        match self {
            Self::Front => "front",
            Self::Right => "right",
            Self::Back => "back",
            Self::Left => "left",
            Self::Up => "up",
            Self::Down => "down",
        }
    }
}

/// The six captured faces, all square and all the same size.
///
/// Six named fields rather than an array: a face is looked up by
/// [`CubeFaces::face`] on a `match`, so no index can be out of range and no
/// caller can hand the constructor a differently-ordered array without saying
/// so.
#[derive(Debug, Clone)]
pub struct CubeFaces {
    /// The face looking along the capture heading.
    front: RgbImage,
    /// The face to its right.
    right: RgbImage,
    /// The face behind the camera.
    back: RgbImage,
    /// The face to its left.
    left: RgbImage,
    /// The zenith face.
    up: RgbImage,
    /// The nadir face.
    down: RgbImage,
}

/// What can go wrong turning a set of captured faces into a panorama.
#[derive(Debug, thiserror::Error)]
pub enum EquirectError {
    /// Fewer or more than the six faces of a cube were handed in.
    #[error("a cube map needs exactly six faces, got {count}")]
    FaceCount {
        /// How many faces the caller offered.
        count: usize,
    },
    /// A face was not square, or not the same size as the others.
    #[error(
        "cube face {face} is {width}x{height}; every face must be square and {expected}x{expected}"
    )]
    FaceShape {
        /// Which face is the odd one out.
        face: &'static str,
        /// Its width in pixels.
        width: u32,
        /// Its height in pixels.
        height: u32,
        /// The edge length every face must have.
        expected: u32,
    },
    /// The requested face or output size is outside what this code will do.
    #[error("{what} of {value} px is outside the supported range {min}..={max}")]
    Size {
        /// Which size was rejected.
        what: &'static str,
        /// The value rejected.
        value: u32,
        /// The smallest accepted value.
        min: u32,
        /// The largest accepted value.
        max: u32,
    },
}

impl CubeFaces {
    /// Take the six faces in [`CUBE_FACES`] order, checking that they are
    /// square, equal and within the supported size range.
    ///
    /// # Errors
    ///
    /// [`EquirectError`] when the count, the shapes or the size do not hold.
    pub fn from_faces(faces: Vec<RgbImage>) -> Result<Self, EquirectError> {
        let count = faces.len();
        let faces: [RgbImage; 6] = faces
            .try_into()
            .map_err(|_ignored: Vec<RgbImage>| EquirectError::FaceCount { count })?;
        let [front, right, back, left, up, down] = faces;
        let size = front.width();
        if !(MIN_FACE_SIZE..=MAX_PANORAMA_SIZE).contains(&size) {
            return Err(EquirectError::Size {
                what: "a cube face",
                value: size,
                min: MIN_FACE_SIZE,
                max: MAX_PANORAMA_SIZE,
            });
        }
        let cube = Self {
            front,
            right,
            back,
            left,
            up,
            down,
        };
        for face in CUBE_FACES {
            let image = cube.face(face);
            if image.width() != size || image.height() != size {
                return Err(EquirectError::FaceShape {
                    face: face.slug(),
                    width: image.width(),
                    height: image.height(),
                    expected: size,
                });
            }
        }
        Ok(cube)
    }

    /// The image shot for `face`.
    #[must_use]
    pub const fn face(&self, face: CubeFace) -> &RgbImage {
        match face {
            CubeFace::Front => &self.front,
            CubeFace::Right => &self.right,
            CubeFace::Back => &self.back,
            CubeFace::Left => &self.left,
            CubeFace::Up => &self.up,
            CubeFace::Down => &self.down,
        }
    }

    /// The edge length of every face, in pixels.
    #[must_use]
    pub fn size(&self) -> u32 {
        self.front.width()
    }
}

// ---------------------------------------------------------------------------
// The mapping.
// ---------------------------------------------------------------------------

/// The direction a point on the equirectangular image looks in, in the capture
/// frame.
///
/// `u` runs `0..1` left to right and is the longitude — `0.5`, the centre
/// column, is the capture heading, and longitude **increases to the right**, so
/// panning right in the finished panorama turns clockwise (the way a compass
/// heading grows). `v` runs `0..1` top to bottom and is the polar angle, so
/// `v = 0` is the zenith and `v = 1` the nadir.
#[must_use]
pub fn direction_for(u: f32, v: f32) -> Vec3 {
    let longitude = (u - 0.5) * TAU;
    let polar = v * PI;
    let sin_polar = polar.sin();
    Vec3::new(
        sin_polar * longitude.sin(),
        polar.cos(),
        -sin_polar * longitude.cos(),
    )
}

/// Which face a direction lands on: the one whose axis dominates it.
#[must_use]
pub fn face_of(direction: Vec3) -> CubeFace {
    let magnitude = direction.abs();
    if magnitude.x >= magnitude.y && magnitude.x >= magnitude.z {
        if direction.x >= 0.0 {
            CubeFace::Right
        } else {
            CubeFace::Left
        }
    } else if magnitude.y >= magnitude.z {
        if direction.y >= 0.0 {
            CubeFace::Up
        } else {
            CubeFace::Down
        }
    } else if direction.z >= 0.0 {
        CubeFace::Back
    } else {
        CubeFace::Front
    }
}

/// Where `direction` lands on `face`, in `0..1` face coordinates (`u` to the
/// right, `v` downward, matching the rendered image's rows).
///
/// `None` when the direction is behind that face's camera, which is what a
/// caller that picked the face by hand rather than through [`face_of`] gets.
///
/// The maths is the projection [`CubeFace::rotation`] describes: a 90° vertical
/// field of view on a square target puts the frustum's half-extent at exactly
/// the view depth, so the local `x` and `y` divided by that depth *are* the
/// normalised device coordinates.
#[must_use]
pub fn face_uv(face: CubeFace, direction: Vec3) -> Option<Vec2> {
    let local = face.rotation().inverse().mul_vec3(direction);
    let depth = -local.z;
    if depth <= f32::EPSILON {
        return None;
    }
    Some(Vec2::new(
        0.5f32.mul_add(local.x / depth, 0.5),
        0.5f32.mul_add(-(local.y / depth), 0.5),
    ))
}

/// How many sub-samples per axis an output of `width` deserves against a cube
/// shot at `face_size`.
///
/// At a face's centre the cube carries `face_size / 90` pixels per degree and
/// the output `width / 360`, so the cube out-resolves the output by
/// `4 * face_size / width`. Sampling at that rate is what turns the extra
/// capture resolution into a smoother panorama instead of aliasing; below it
/// there is nothing to average and one tap is honest.
#[must_use]
pub fn supersamples(face_size: u32, width: u32) -> u32 {
    let ratio = face_size
        .saturating_mul(4)
        .checked_div(width.max(1))
        .unwrap_or(1);
    ratio.clamp(1, MAX_SUPERSAMPLES)
}

// ---------------------------------------------------------------------------
// Reprojection.
// ---------------------------------------------------------------------------

/// Reproject a cube-map capture into an equirectangular panorama `width` pixels
/// across and `width / 2` tall.
///
/// The work is split into `BAND_ROWS`-row bands across the
/// [`ComputeTaskPool`]: the reprojection of a 4096×2048 panorama is tens of
/// millions of bilinear taps, and it runs while the viewer keeps drawing, so it
/// is worth every core the machine has.
///
/// # Errors
///
/// [`EquirectError::Size`] when `width` is odd, zero, or beyond
/// [`MAX_PANORAMA_SIZE`].
pub fn reproject(faces: &CubeFaces, width: u32) -> Result<RgbImage, EquirectError> {
    if !(MIN_FACE_SIZE..=MAX_PANORAMA_SIZE).contains(&width) || !width.is_multiple_of(2) {
        return Err(EquirectError::Size {
            what: "an even output width",
            value: width,
            min: MIN_FACE_SIZE,
            max: MAX_PANORAMA_SIZE,
        });
    }
    let height = width / 2;
    let row_bytes = usize::try_from(width)
        .ok()
        .and_then(|width| width.checked_mul(3))
        .ok_or(EquirectError::Size {
            what: "an output width",
            value: width,
            min: MIN_FACE_SIZE,
            max: MAX_PANORAMA_SIZE,
        })?;
    let total = usize::try_from(height)
        .ok()
        .and_then(|height| height.checked_mul(row_bytes))
        .ok_or(EquirectError::Size {
            what: "an output width",
            value: width,
            min: MIN_FACE_SIZE,
            max: MAX_PANORAMA_SIZE,
        })?;
    let mut pixels = vec![0_u8; total];

    let grid = supersamples(faces.size(), width);
    let band_rows = usize::try_from(BAND_ROWS).unwrap_or(1).max(1);
    let band_bytes = row_bytes.saturating_mul(band_rows);
    let pool = ComputeTaskPool::get_or_init(TaskPool::default);
    pool.scope(|scope| {
        for (band, rows) in pixels.chunks_mut(band_bytes).enumerate() {
            let first_row = band.saturating_mul(band_rows);
            scope.spawn(async move {
                fill_band(faces, width, height, grid, first_row, rows);
            });
        }
    });

    RgbImage::from_raw(width, height, pixels).ok_or(EquirectError::Size {
        what: "an output width",
        value: width,
        min: MIN_FACE_SIZE,
        max: MAX_PANORAMA_SIZE,
    })
}

/// Fill one horizontal band of the output, whose first row is `first_row`.
///
/// Split out of [`reproject`] as the band worker, and written against plain
/// slices so it is the same code whether one band or thirty run at once.
fn fill_band(
    faces: &CubeFaces,
    width: u32,
    height: u32,
    grid: u32,
    first_row: usize,
    rows: &mut [u8],
) {
    let width_f = coord_to_f32(width);
    let height_f = coord_to_f32(height);
    let grid_f = coord_to_f32(grid);
    let samples = grid.saturating_mul(grid);
    let weight = 1.0 / coord_to_f32(samples).max(1.0);
    let row_len = usize::try_from(width)
        .ok()
        .and_then(|width| width.checked_mul(3))
        .unwrap_or(0);
    if row_len == 0 {
        return;
    }
    for (offset, row) in rows.chunks_mut(row_len).enumerate() {
        let y = coord_to_f32(first_row.saturating_add(offset));
        for (column, pixel) in row.chunks_mut(3).enumerate() {
            let x = coord_to_f32(column);
            let mut accumulated = [0.0_f32; 3];
            for sub_y in 0..grid {
                for sub_x in 0..grid {
                    let u = (x + (coord_to_f32(sub_x) + 0.5) / grid_f) / width_f;
                    let v = (y + (coord_to_f32(sub_y) + 0.5) / grid_f) / height_f;
                    let direction = direction_for(u, v);
                    let sample = sample_cube(faces, direction);
                    for (channel, value) in accumulated.iter_mut().zip(sample) {
                        *channel += value * weight;
                    }
                }
            }
            let Rgb(encoded) = encode_srgb(accumulated);
            for (byte, value) in pixel.iter_mut().zip(encoded) {
                *byte = value;
            }
        }
    }
}

/// The linear-light colour the cube shows in `direction`.
///
/// A direction that somehow lands on no face (only possible for a zero or
/// non-finite vector) reads as black rather than panicking: a panorama with one
/// dark pixel is a better outcome than a capture that dies.
fn sample_cube(faces: &CubeFaces, direction: Vec3) -> [f32; 3] {
    let face = face_of(direction);
    let Some(uv) = face_uv(face, direction) else {
        return [0.0; 3];
    };
    sample_face_linear(faces.face(face), uv)
}

/// A clamped bilinear tap into one face, returned in linear light.
///
/// The clamp is the seam policy described in [the module docs](self): a tap that
/// wants a texel past the face edge reuses the edge texel instead of reaching
/// into a neighbour this function cannot see.
fn sample_face_linear(image: &RgbImage, uv: Vec2) -> [f32; 3] {
    let (width, height) = (image.width(), image.height());
    // Texel *centres* sit at half-pixel offsets, so the continuous coordinate
    // of texel `i` is `i + 0.5`; subtracting the half puts the interpolation
    // between the two texels a sample actually falls between.
    let x = uv.x.mul_add(coord_to_f32(width), -0.5);
    let y = uv.y.mul_add(coord_to_f32(height), -0.5);
    let (x0, x1, fraction_x) = texel_span(x, width);
    let (y0, y1, fraction_y) = texel_span(y, height);
    let top_left = linear_rgb(image.get_pixel(x0, y0));
    let top_right = linear_rgb(image.get_pixel(x1, y0));
    let bottom_left = linear_rgb(image.get_pixel(x0, y1));
    let bottom_right = linear_rgb(image.get_pixel(x1, y1));
    let mut blended = [0.0_f32; 3];
    for (channel, index) in blended.iter_mut().zip(0_usize..) {
        let top = lerp(
            top_left.get(index).copied().unwrap_or(0.0),
            top_right.get(index).copied().unwrap_or(0.0),
            fraction_x,
        );
        let bottom = lerp(
            bottom_left.get(index).copied().unwrap_or(0.0),
            bottom_right.get(index).copied().unwrap_or(0.0),
            fraction_x,
        );
        *channel = lerp(top, bottom, fraction_y);
    }
    blended
}

/// The two texels a continuous coordinate falls between and how far it is
/// between them, clamped into `0..extent`.
fn texel_span(coordinate: f32, extent: u32) -> (u32, u32, f32) {
    let last = extent.saturating_sub(1);
    if !coordinate.is_finite() || coordinate <= 0.0 {
        return (0, 0, 0.0);
    }
    let floor = coordinate.floor();
    let index = floor_to_u32(floor);
    if index >= last {
        return (last, last, 0.0);
    }
    (index, index.saturating_add(1), coordinate - floor)
}

/// The linear interpolation `a + (b - a) * t`.
fn lerp(a: f32, b: f32, t: f32) -> f32 {
    (b - a).mul_add(t, a)
}

// ---------------------------------------------------------------------------
// Colour.
// ---------------------------------------------------------------------------

/// The sRGB transfer function's inverse, tabulated for all 256 code values.
///
/// A table rather than a `powf` per tap: sixteen taps per output pixel times
/// three channels times eight million pixels is a lot of `powf`.
static SRGB_TO_LINEAR: LazyLock<[f32; 256]> = LazyLock::new(|| {
    let mut table = [0.0_f32; 256];
    for (entry, code) in table.iter_mut().zip(0_u16..) {
        let value = f32::from(code) / 255.0;
        *entry = if value <= 0.040_45 {
            value / 12.92
        } else {
            ((value + 0.055) / 1.055).powf(2.4)
        };
    }
    table
});

/// One 8-bit sRGB pixel as linear light.
fn linear_rgb(pixel: &Rgb<u8>) -> [f32; 3] {
    let table = &*SRGB_TO_LINEAR;
    let Rgb(channels) = *pixel;
    let mut linear = [0.0_f32; 3];
    for (slot, code) in linear.iter_mut().zip(channels) {
        *slot = table.get(usize::from(code)).copied().unwrap_or(0.0);
    }
    linear
}

/// Linear light back to one 8-bit sRGB pixel.
fn encode_srgb(linear: [f32; 3]) -> Rgb<u8> {
    let mut encoded = [0_u8; 3];
    for (slot, value) in encoded.iter_mut().zip(linear) {
        let clamped = value.clamp(0.0, 1.0);
        let transferred = if clamped <= 0.003_130_8 {
            clamped * 12.92
        } else {
            1.055f32.mul_add(clamped.powf(1.0 / 2.4), -0.055)
        };
        *slot = round_to_u8(transferred * 255.0);
    }
    Rgb(encoded)
}

// ---------------------------------------------------------------------------
// Numeric conversions.
// ---------------------------------------------------------------------------

/// A pixel coordinate or count as `f32`.
///
/// Every dimension here is bounded by [`MAX_PANORAMA_SIZE`], which is well
/// inside `u16`, so the conversion is exact; anything larger saturates rather
/// than wrapping into a coordinate that would sample the wrong texel.
fn coord_to_f32(value: impl TryInto<u16>) -> f32 {
    f32::from(value.try_into().unwrap_or(u16::MAX))
}

/// The floor of a non-negative, finite `f32` as `u32`.
#[expect(
    clippy::as_conversions,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "the caller (`texel_span`) has already established that the value is finite and \
              positive, and every coordinate here is bounded by MAX_PANORAMA_SIZE, so the floor \
              fits a u32 exactly"
)]
fn floor_to_u32(value: f32) -> u32 {
    if value.is_finite() && value >= 0.0 {
        value.floor().min(f32::from(u16::MAX)) as u32
    } else {
        0
    }
}

/// Round a `0..=255`-ish `f32` to the nearest `u8`, clamping the ends.
#[expect(
    clippy::as_conversions,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "the value is clamped into 0..=255 and rounded before the conversion"
)]
fn round_to_u8(value: f32) -> u8 {
    if !value.is_finite() || value <= 0.0 {
        0
    } else if value >= 255.0 {
        u8::MAX
    } else {
        value.round() as u8
    }
}

#[cfg(test)]
mod tests {
    use super::{
        CUBE_FACES, CubeFace, CubeFaces, MAX_PANORAMA_SIZE, direction_for, encode_srgb, face_of,
        face_uv, linear_rgb, reproject, supersamples,
    };
    use bevy::math::Vec3;
    use image::{Rgb, RgbImage};
    use pretty_assertions::assert_eq;

    /// A boxed error so tests use `?` rather than the disallowed `expect`.
    type TestError = Box<dyn core::error::Error>;

    /// How far two unit directions may differ and still count as the same one.
    const DIRECTION_EPSILON: f32 = 1.0e-5;

    /// A face's rotation and its named direction are the same statement — the
    /// rotation is what the camera is given, the direction is what the sampler
    /// picks by, and a drift between them would point the reprojection at the
    /// wrong face.
    #[test]
    fn every_face_looks_down_the_axis_it_is_named_for() {
        let expected = [
            (CubeFace::Front, Vec3::NEG_Z),
            (CubeFace::Right, Vec3::X),
            (CubeFace::Back, Vec3::Z),
            (CubeFace::Left, Vec3::NEG_X),
            (CubeFace::Up, Vec3::Y),
            (CubeFace::Down, Vec3::NEG_Y),
        ];
        for (face, axis) in expected {
            assert!(
                face.direction().distance(axis) < DIRECTION_EPSILON,
                "{face:?} looks along {:?}, expected {axis:?}",
                face.direction()
            );
        }
    }

    /// The centre of the panorama is the capture heading, its right-hand
    /// quarter is the capture's right, and its top row is the zenith — the
    /// layout every consumer of an equirectangular image assumes.
    #[test]
    fn the_image_is_laid_out_around_the_capture_heading() {
        let probes = [
            (0.5, 0.5, Vec3::NEG_Z),
            (0.75, 0.5, Vec3::X),
            (0.25, 0.5, Vec3::NEG_X),
            (0.0, 0.5, Vec3::Z),
            (0.5, 0.0, Vec3::Y),
            (0.5, 1.0, Vec3::NEG_Y),
        ];
        for (u, v, axis) in probes {
            let direction = direction_for(u, v);
            assert!(
                direction.distance(axis) < DIRECTION_EPSILON,
                "({u}, {v}) looks along {direction:?}, expected {axis:?}"
            );
        }
    }

    /// A face's own direction lands on that face, dead centre.
    #[test]
    fn a_face_centre_maps_to_its_own_face() -> Result<(), TestError> {
        for face in CUBE_FACES {
            let direction = face.direction();
            assert_eq!(face_of(direction), face, "{face:?} lost its own centre");
            let uv = face_uv(face, direction)
                .ok_or("a face's own direction should be in front of it")?;
            assert!(
                (uv.x - 0.5).abs() < DIRECTION_EPSILON && (uv.y - 0.5).abs() < DIRECTION_EPSILON,
                "{face:?} put its own centre at {uv:?}"
            );
        }
        Ok(())
    }

    /// A direction behind a face has no place on it — the guard that keeps a
    /// hand-picked face from sampling a mirrored, nonsensical texel.
    #[test]
    fn a_direction_behind_a_face_has_no_coordinate_on_it() {
        assert!(face_uv(CubeFace::Front, Vec3::Z).is_none());
        assert!(face_uv(CubeFace::Up, Vec3::NEG_Y).is_none());
    }

    /// Build a cube whose every face is the flat colour `colours` names for it.
    fn flat_cube(size: u32, colours: [Rgb<u8>; 6]) -> Result<CubeFaces, TestError> {
        let faces = CUBE_FACES
            .into_iter()
            .zip(colours)
            .map(|(_face, colour)| RgbImage::from_pixel(size, size, colour))
            .collect();
        Ok(CubeFaces::from_faces(faces)?)
    }

    /// A uniformly coloured cube reprojects to that colour everywhere — which
    /// also pins the sRGB → linear → sRGB round trip, since any error in it
    /// would shift a flat field.
    #[test]
    fn a_uniform_cube_reprojects_to_that_colour() -> Result<(), TestError> {
        let grey = Rgb([90, 130, 200]);
        let cube = flat_cube(32, [grey; 6])?;
        let panorama = reproject(&cube, 64)?;
        assert_eq!(panorama.dimensions(), (64, 32));
        for pixel in panorama.pixels() {
            assert_eq!(*pixel, grey);
        }
        Ok(())
    }

    /// Each face owns the part of the horizon it faces: the centre column is
    /// the front face, the quarters are the sides, the top row is the zenith.
    #[test]
    fn each_face_owns_its_share_of_the_sphere() -> Result<(), TestError> {
        let colours = [
            Rgb([255, 0, 0]),
            Rgb([0, 255, 0]),
            Rgb([0, 0, 255]),
            Rgb([255, 255, 0]),
            Rgb([255, 0, 255]),
            Rgb([0, 255, 255]),
        ];
        let cube = flat_cube(32, colours)?;
        let panorama = reproject(&cube, 128)?;
        let probes = [
            (64_u32, 32_u32, 0_usize),
            (96, 32, 1),
            (0, 32, 2),
            (32, 32, 3),
            (64, 0, 4),
            (64, 63, 5),
        ];
        for (x, y, which) in probes {
            let expected = colours.get(which).copied().ok_or("six probes, six faces")?;
            assert_eq!(
                *panorama.get_pixel(x, y),
                expected,
                "the pixel at ({x}, {y}) should belong to {:?}",
                CUBE_FACES.get(which)
            );
        }
        Ok(())
    }

    /// Render one cube face from an analytic function of direction, the way the
    /// renderer would.
    fn render_face(face: CubeFace, size: u32, shade: impl Fn(Vec3) -> Rgb<u8>) -> RgbImage {
        let edge = f32::from(u16::try_from(size).unwrap_or(1));
        RgbImage::from_fn(size, size, |x, y| {
            let u = (f32::from(u16::try_from(x).unwrap_or(0)) + 0.5) / edge;
            let v = (f32::from(u16::try_from(y).unwrap_or(0)) + 0.5) / edge;
            // The inverse of `face_uv`: a ray through the pixel centre.
            let local = Vec3::new(u.mul_add(2.0, -1.0), (1.0 - v).mul_add(2.0, -1.0), -1.0);
            shade(face.rotation().mul_vec3(local).normalize())
        })
    }

    /// The whole pipeline against a smooth, analytically known sphere: shoot
    /// six faces of a gradient that depends only on direction, reproject, and
    /// check the panorama against the function it was made from. This is the
    /// test that would catch a flipped axis, a transposed face or a seam that
    /// does not line up — all of which leave the flat-colour tests green.
    #[test]
    fn a_smooth_sphere_survives_the_round_trip() -> Result<(), TestError> {
        let shade = |direction: Vec3| {
            Rgb([
                super::round_to_u8(direction.x.mul_add(0.5, 0.5) * 255.0),
                super::round_to_u8(direction.y.mul_add(0.5, 0.5) * 255.0),
                super::round_to_u8(direction.z.mul_add(0.5, 0.5) * 255.0),
            ])
        };
        let faces = CUBE_FACES
            .into_iter()
            .map(|face| render_face(face, 128, shade))
            .collect();
        let cube = CubeFaces::from_faces(faces)?;
        let panorama = reproject(&cube, 256)?;
        let mut worst = 0_i32;
        for (x, y, pixel) in panorama.enumerate_pixels() {
            let u = (f32::from(u16::try_from(x).unwrap_or(0)) + 0.5) / 256.0;
            let v = (f32::from(u16::try_from(y).unwrap_or(0)) + 0.5) / 128.0;
            let Rgb(expected) = shade(direction_for(u, v));
            let Rgb(got) = *pixel;
            for (left, right) in expected.into_iter().zip(got) {
                let difference = i32::from(left).saturating_sub(i32::from(right)).abs();
                worst = worst.max(difference);
            }
        }
        // Eight code values: the gradient is smooth, so what is left is the
        // half-texel blur of the clamped taps plus the sRGB round trip.
        assert!(
            worst <= 8,
            "the reprojected sphere is off by {worst} code values"
        );
        Ok(())
    }

    /// The seam — the panorama's left and right edges, which are neighbours on
    /// the sphere — must not show a step. Checked on the same smooth sphere,
    /// where any discontinuity is the stitch and not the content.
    #[test]
    fn the_seam_behind_the_camera_is_continuous() -> Result<(), TestError> {
        let shade = |direction: Vec3| {
            Rgb([
                super::round_to_u8(direction.x.mul_add(0.5, 0.5) * 255.0),
                super::round_to_u8(direction.z.mul_add(0.5, 0.5) * 255.0),
                128,
            ])
        };
        let faces = CUBE_FACES
            .into_iter()
            .map(|face| render_face(face, 128, shade))
            .collect();
        let cube = CubeFaces::from_faces(faces)?;
        let panorama = reproject(&cube, 256)?;
        for y in 0..panorama.height() {
            let Rgb(left) = *panorama.get_pixel(0, y);
            let Rgb(right) = *panorama.get_pixel(panorama.width().saturating_sub(1), y);
            for (a, b) in left.into_iter().zip(right) {
                let difference = i32::from(a).saturating_sub(i32::from(b)).abs();
                assert!(
                    difference <= 8,
                    "row {y} steps by {difference} across the seam"
                );
            }
        }
        Ok(())
    }

    /// The supersample grid follows the resolution the cube was shot at.
    #[test]
    fn the_supersample_grid_follows_the_capture_resolution() {
        assert_eq!(supersamples(2048, 4096), 2);
        assert_eq!(supersamples(1024, 4096), 1);
        assert_eq!(supersamples(512, 4096), 1);
        assert_eq!(supersamples(2048, 2048), 4);
        // Capped, however lopsided the pair.
        assert_eq!(supersamples(8192, 1024), 4);
    }

    /// Averaging happens in linear light: half black and half white is the
    /// perceptual midpoint (~188), not the arithmetic one (128). Getting this
    /// wrong darkens every high-contrast edge in a panorama.
    #[test]
    fn colours_are_averaged_in_linear_light() {
        let black = linear_rgb(&Rgb([0, 0, 0]));
        let white = linear_rgb(&Rgb([255, 255, 255]));
        let mut mixed = [0.0_f32; 3];
        for (slot, (dark, light)) in mixed.iter_mut().zip(black.into_iter().zip(white)) {
            *slot = f32::midpoint(dark, light);
        }
        let Rgb(encoded) = encode_srgb(mixed);
        for value in encoded {
            assert!(
                (186..=190).contains(&value),
                "a half-and-half mix encoded to {value}, which is not the linear midpoint"
            );
        }
    }

    /// A cube has six faces; five or seven is a bug in the capture, not
    /// something to reproject.
    #[test]
    fn a_short_cube_is_refused() {
        let faces = core::iter::repeat_with(|| RgbImage::new(8, 8))
            .take(5)
            .collect();
        assert!(matches!(CubeFaces::from_faces(faces), Err(_error)));
    }

    /// Faces must be square and equal — one of the wrong size would sample off
    /// the end of its own image.
    #[test]
    fn a_mismatched_face_is_refused() {
        let mut faces: Vec<RgbImage> = core::iter::repeat_with(|| RgbImage::new(8, 8))
            .take(5)
            .collect();
        faces.push(RgbImage::new(8, 4));
        assert!(matches!(CubeFaces::from_faces(faces), Err(_error)));
    }

    /// An odd or oversized output width has no 2:1 panorama, and is refused
    /// rather than rounded behind the caller's back.
    #[test]
    fn an_impossible_output_width_is_refused() -> Result<(), TestError> {
        let cube = flat_cube(8, [Rgb([0, 0, 0]); 6])?;
        assert!(matches!(reproject(&cube, 63), Err(_odd)));
        assert!(matches!(reproject(&cube, 0), Err(_zero)));
        assert!(matches!(
            reproject(&cube, MAX_PANORAMA_SIZE.saturating_add(2)),
            Err(_huge)
        ));
        Ok(())
    }
}
