//! Stand-ins for the **built-in library textures** a viewer asks every grid for:
//! the sun and moon discs, the cloud noise, the rainbow and halo overlays, the
//! star bloom, the water wave normal, and the blank plywood a prim face falls
//! back to.
//!
//! None of these ship with a viewer. Firestorm marks the sky ones `// dataserver`
//! and its `static_assets` folders hold only animations, wearables and gestures,
//! so on a real grid all eight are ordinary library assets fetched over
//! `GetTexture`. A grid that serves nothing for them leaves an arriving viewer
//! burning its whole retry budget on eight fetches that will never succeed, and
//! renders a sky with no sun in it.
//!
//! Serving a *stand-in* under a real Linden id is honest here in a way it would
//! not be for, say, a fixture animation wearing a Linden animation id: a fake
//! grid **is** a grid with a library, and a library id is exactly what these are.
//! What the stand-ins are not is Linden's own pixels — so they are built to be
//! *recognisable in the role*: a disc reads as a sun rather than as a square, a
//! flat `(128, 128, 255)` normal leaves the sea unrippled rather than warped, and
//! the halo's bright band sits at the 22° radius the shader samples it at.
//!
//! Each is keyed by the id `sl-proto` names for it, so nothing here restates a
//! UUID the renderer already knows.

use sl_texture::EncodeError;
use uuid::Uuid;

use crate::{RgbaImage, round_to_u8};

/// The side, in pixels, of the shaped textures below (the discs, the noise, the
/// overlays). Large enough that a disc is round rather than octagonal and that
/// the halo's band is several rows deep, small enough that eight of them encode
/// in well under a second.
pub const SHAPED_SIZE: u32 = 64;

/// The side, in pixels, of the flat textures below (the wave normal, the
/// plywood). A solid needs no resolution at all; 32 is what the terrain detail
/// solids use.
pub const FLAT_SIZE: u32 = 32;

/// The colour of the sun disc — the near-white warm of a midday sun, which the
/// disc shader multiplies by the sky's own brightness.
const SUN_RGB: [u8; 3] = [255, 246, 214];

/// The colour of the moon disc: paler and cooler than the sun, so a capture of a
/// night sky can tell which body it is looking at.
const MOON_RGB: [u8; 3] = [206, 212, 226];

/// What the reference's moon texture puts in its transparent texels
/// (`<0x55, 0x55, 0x55, 0x00>`, which `moonF.glsl` discards on). The stand-in
/// uses the same grey outside its disc so a viewer that samples the surround
/// sees what it would on a real grid.
const MOON_TRANSPARENT_RGB: [u8; 3] = [0x55, 0x55, 0x55];

/// The blank-plywood colour: the light tan of a freshly rezzed prim, well clear
/// of every [marker colour](crate::markers) so an untextured face never reads as
/// a fixture that failed to apply its texture.
const PLYWOOD_RGB: [u8; 3] = [190, 158, 116];

/// The water plane's stand-in colour: a translucent blue-green, so a capture
/// showing water shows something water-coloured rather than a hole.
const WATER_PLANE_RGBA: [u8; 4] = [64, 110, 120, 200];

/// The tangent-space "no bump at all" normal, `(0, 0, 1)` encoded into a byte
/// per axis. The sea then takes its shape entirely from the wave maths rather
/// than from a stand-in's invented ripples.
const FLAT_NORMAL_RGBA: [u8; 4] = [128, 128, 255, 255];

/// Where the sun / moon disc stops being opaque, as a fraction of the half-width
/// of the image — a disc that reached the edge would be clipped square by the
/// quad it is drawn on.
const DISC_INNER_RADIUS: f32 = 0.78;

/// Where the sun / moon disc has faded out entirely. The gap to
/// [`DISC_INNER_RADIUS`] is the soft rim, which also keeps the lossy JPEG2000
/// encoder from ringing along a hard alpha step.
const DISC_OUTER_RADIUS: f32 = 0.96;

/// The centre of the halo's bright band, in the shader's sample coordinate.
/// `skyF.glsl` samples the halo texture at `v = sqrt(1 - d²)` for the view/sun
/// dot `d`, so the 22° ring lands at `sin(22°)`.
const HALO_BAND_CENTRE: f32 = 0.374_606_6;

/// The half-width of the halo band, in the same coordinate. Wide enough to be
/// several rows of a [`SHAPED_SIZE`] texture, narrow enough to read as a ring
/// rather than a wash.
const HALO_BAND_HALF_WIDTH: f32 = 0.06;

/// The centre of the rainbow band, in the shader's sample coordinate (the
/// vertical axis of `IMG_RAINBOW`, which sweeps the band; the horizontal axis
/// selects a droplet radius, which a stand-in has no variants of).
const RAINBOW_BAND_CENTRE: f32 = 0.3;

/// The half-width of the rainbow band, across which the spectrum runs.
const RAINBOW_BAND_HALF_WIDTH: f32 = 0.15;

/// The hue, in degrees, at the outer edge of the rainbow band (red).
const RAINBOW_OUTER_HUE: f32 = 0.0;

/// The hue, in degrees, at the inner edge of the rainbow band (violet).
const RAINBOW_INNER_HUE: f32 = 270.0;

/// A pixel coordinate as a float, without a lint-triggering cast: every value
/// here is a texture coordinate, far below `u16::MAX`.
fn coordinate(value: u32) -> f32 {
    f32::from(u16::try_from(value).unwrap_or(u16::MAX))
}

/// `value` mapped from `[from, to]` onto `[0, 1]` and clamped, or zero for a
/// degenerate range.
fn ramp(value: f32, from: f32, to: f32) -> f32 {
    let span = to - from;
    if span == 0.0 {
        return 0.0;
    }
    ((value - from) / span).clamp(0.0, 1.0)
}

/// A cubic smoothstep over `[0, 1]`, so a fade has no visible edge where it
/// starts or stops.
fn smoothstep(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// The distance of the pixel `(x, y)` from the centre of a `size`×`size` image,
/// as a fraction of its half-width (`0` at the centre, `1` at the edge midpoints).
fn radius(x: u32, y: u32, size: u32) -> f32 {
    let half = coordinate(size) / 2.0;
    if half <= 0.0 {
        return 0.0;
    }
    let dx = (coordinate(x) + 0.5 - half) / half;
    let dy = (coordinate(y) + 0.5 - half) / half;
    dx.hypot(dy)
}

/// A fully-saturated colour at `hue_degrees`, as three `0..=1` components — the
/// spectrum the rainbow band sweeps.
fn hue_rgb(hue_degrees: f32) -> [f32; 3] {
    let sector = (hue_degrees / 60.0).rem_euclid(6.0);
    let ramp_up = 1.0 - (sector.rem_euclid(2.0) - 1.0).abs();
    if sector < 1.0 {
        [1.0, ramp_up, 0.0]
    } else if sector < 2.0 {
        [ramp_up, 1.0, 0.0]
    } else if sector < 3.0 {
        [0.0, 1.0, ramp_up]
    } else if sector < 4.0 {
        [0.0, ramp_up, 1.0]
    } else if sector < 5.0 {
        [ramp_up, 0.0, 1.0]
    } else {
        [1.0, 0.0, ramp_up]
    }
}

/// An opaque `0..=1` colour as a texel.
fn opaque(rgb: [f32; 3]) -> [u8; 4] {
    let channel = |value: f32| round_to_u8(value.clamp(0.0, 1.0) * 255.0);
    [channel(rgb[0]), channel(rgb[1]), channel(rgb[2]), u8::MAX]
}

/// A soft-edged disc: `inside` where the disc is solid, fading to `outside` at
/// zero alpha beyond its rim.
///
/// The rgb is interpolated along with the alpha rather than left constant, so
/// the lossy encoder has no colour step to ring on where the alpha reaches zero.
fn disc(size: u32, inside: [u8; 3], outside: [u8; 3]) -> RgbaImage {
    RgbaImage::painted(size, |x, y| {
        let mask = 1.0
            - smoothstep(ramp(
                radius(x, y, size),
                DISC_INNER_RADIUS,
                DISC_OUTER_RADIUS,
            ));
        let blend = |inner: u8, outer: u8| {
            round_to_u8(f32::from(outer) + (f32::from(inner) - f32::from(outer)) * mask)
        };
        [
            blend(inside[0], outside[0]),
            blend(inside[1], outside[1]),
            blend(inside[2], outside[2]),
            round_to_u8(mask * 255.0),
        ]
    })
}

/// The **sun disc** stand-in ([`sl_proto::DEFAULT_SUN_TEXTURE`]): a warm
/// near-white disc on a transparent field.
#[must_use]
pub fn sun_disc(size: u32) -> RgbaImage {
    disc(size, SUN_RGB, SUN_RGB)
}

/// The **moon disc** stand-in ([`sl_proto::DEFAULT_MOON_TEXTURE`]): a paler,
/// cooler disc whose surround is the reference's transparent grey, which
/// `moonF.glsl` discards so the quad never hides the stars behind it.
#[must_use]
pub fn moon_disc(size: u32) -> RgbaImage {
    disc(size, MOON_RGB, MOON_TRANSPARENT_RGB)
}

/// The **cloud noise** stand-in ([`sl_proto::DEFAULT_CLOUD_TEXTURE`]): one soft
/// blob per tile, bright at the centre and dark at every edge.
///
/// The cloud shader reads only the red channel, and reads it at half a dozen
/// different uv scales at once — so what matters is that the texture *tiles*
/// without a seam. A separable raised cosine does: it reaches zero on all four
/// edges, which is what a radial blob would not.
#[must_use]
pub fn cloud_noise(size: u32) -> RgbaImage {
    RgbaImage::painted(size, |x, y| {
        let bump = |value: u32| {
            let phase = core::f32::consts::TAU * coordinate(value) / coordinate(size).max(1.0);
            0.5 - 0.5 * phase.cos()
        };
        let density = bump(x) * bump(y);
        opaque([density, density, density])
    })
}

/// The **rainbow overlay** stand-in ([`sl_proto::DEFAULT_RAINBOW_TEXTURE`]): a
/// spectrum band running red on the outside to violet on the inside, black
/// everywhere else.
///
/// `skyF.glsl` samples this by *row* — the vertical axis sweeps across the bow,
/// the horizontal axis picks the droplet-radius variant — so every column here
/// is the same profile: a stand-in has one droplet size.
#[must_use]
pub fn rainbow_band(size: u32) -> RgbaImage {
    RgbaImage::painted(size, |_x, y| {
        let v = (coordinate(y) + 0.5) / coordinate(size).max(1.0);
        let offset = (v - RAINBOW_BAND_CENTRE).abs();
        if offset > RAINBOW_BAND_HALF_WIDTH {
            return opaque([0.0, 0.0, 0.0]);
        }
        let across = ramp(
            v,
            RAINBOW_BAND_CENTRE - RAINBOW_BAND_HALF_WIDTH,
            RAINBOW_BAND_CENTRE + RAINBOW_BAND_HALF_WIDTH,
        );
        let hue = RAINBOW_OUTER_HUE + (RAINBOW_INNER_HUE - RAINBOW_OUTER_HUE) * across;
        // Fade the bow out at both edges of the band so it has no hard rim.
        let fade = smoothstep(1.0 - ramp(offset, 0.0, RAINBOW_BAND_HALF_WIDTH));
        let rgb = hue_rgb(hue);
        opaque([rgb[0] * fade, rgb[1] * fade, rgb[2] * fade])
    })
}

/// The **22° ice-halo overlay** stand-in ([`sl_proto::DEFAULT_HALO_TEXTURE`]): a
/// white band centred on the row the shader samples the 22° ring at, black
/// elsewhere.
///
/// `skyF.glsl` samples this at column zero only, so, like [`rainbow_band`],
/// every column is the same profile.
#[must_use]
pub fn halo_ring(size: u32) -> RgbaImage {
    RgbaImage::painted(size, |_x, y| {
        let v = (coordinate(y) + 0.5) / coordinate(size).max(1.0);
        let offset = (v - HALO_BAND_CENTRE).abs();
        let brightness = smoothstep(1.0 - ramp(offset, 0.0, HALO_BAND_HALF_WIDTH));
        opaque([brightness, brightness, brightness])
    })
}

/// The **star bloom** stand-in ([`sl_proto::DEFAULT_BLOOM_TEXTURE`]): a soft
/// white point that falls off to black.
///
/// The star field is drawn additively, so the dark texels contribute nothing and
/// only the bright centre lights the sky — which is why this is a point on black
/// rather than a disc on transparency.
#[must_use]
pub fn star_bloom(size: u32) -> RgbaImage {
    RgbaImage::painted(size, |x, y| {
        let falloff = 1.0 - smoothstep(ramp(radius(x, y, size), 0.0, 1.0));
        let brightness = falloff * falloff;
        [
            round_to_u8(brightness * 255.0),
            round_to_u8(brightness * 255.0),
            round_to_u8(brightness * 255.0),
            round_to_u8(brightness * 255.0),
        ]
    })
}

/// The **wave normal map** stand-in
/// ([`sl_proto::DEFAULT_WATER_NORMAL_TEXTURE`]): a flat tangent-space normal, so
/// the sea's shape comes from the water shader's own wave maths and a stand-in
/// contributes no ripples of its own.
#[must_use]
pub fn flat_wave_normal(size: u32) -> RgbaImage {
    RgbaImage::solid(size, FLAT_NORMAL_RGBA)
}

/// The **blank plywood** stand-in ([`sl_proto::DEFAULT_PRIM_TEXTURE`]): the tan
/// solid a freshly rezzed prim wears.
#[must_use]
pub fn plywood(size: u32) -> RgbaImage {
    RgbaImage::solid(size, opaque_bytes(PLYWOOD_RGB))
}

/// An opaque texel from three byte channels.
const fn opaque_bytes(rgb: [u8; 3]) -> [u8; 4] {
    [rgb[0], rgb[1], rgb[2], u8::MAX]
}

/// One JPEG2000 stand-in per built-in library texture a viewer asks for on
/// arrival — the seven [`sl_proto::BUILTIN_ENVIRONMENT_TEXTURES`] plus the
/// blank-plywood [`sl_proto::DEFAULT_PRIM_TEXTURE`] — so a fake grid answers the
/// whole set rather than leaving eight fetches to exhaust their retries.
///
/// # Errors
///
/// Returns the encoder's error, which none of these images can produce (they are
/// all small, non-empty and four-component).
pub fn library_textures() -> Result<Vec<(Uuid, Vec<u8>)>, EncodeError> {
    let mut textures: Vec<(Uuid, Vec<u8>)> = vec![
        // The avatar sentinels. These are not decoration: an appearance names
        // every un-baked slot with `IMG_DEFAULT_AVATAR`, and the reference
        // viewer *fetches* the sentinel rather than treating it as a marker, so
        // a grid that does not serve it leaves an avatar retrying a 404 for
        // every unbaked slot -- which is both why the avatar stays a cloud and
        // why the scene never falls quiet for a capture waiting on quiescence.
        // On Second Life both are ordinary dataserver assets.
        (
            sl_proto::avatar_texture::IMG_DEFAULT_AVATAR,
            RgbaImage::solid(SHAPED_SIZE, [128, 128, 128, u8::MAX]).j2c()?,
        ),
        (
            sl_proto::avatar_texture::IMG_INVISIBLE,
            RgbaImage::solid(FLAT_SIZE, [0, 0, 0, 0]).j2c()?,
        ),
        (sl_proto::DEFAULT_SUN_TEXTURE, sun_disc(SHAPED_SIZE).j2c()?),
        (
            sl_proto::DEFAULT_MOON_TEXTURE,
            moon_disc(SHAPED_SIZE).j2c()?,
        ),
        (
            sl_proto::DEFAULT_CLOUD_TEXTURE,
            cloud_noise(SHAPED_SIZE).j2c()?,
        ),
        (
            sl_proto::DEFAULT_RAINBOW_TEXTURE,
            rainbow_band(SHAPED_SIZE).j2c()?,
        ),
        (
            sl_proto::DEFAULT_HALO_TEXTURE,
            halo_ring(SHAPED_SIZE).j2c()?,
        ),
        (
            sl_proto::DEFAULT_BLOOM_TEXTURE,
            star_bloom(SHAPED_SIZE).j2c()?,
        ),
        (
            sl_proto::DEFAULT_WATER_NORMAL_TEXTURE,
            flat_wave_normal(FLAT_SIZE).j2c()?,
        ),
        (sl_proto::DEFAULT_PRIM_TEXTURE, plywood(FLAT_SIZE).j2c()?),
    ];

    // The standard bump maps and the two viewer utility textures are the real
    // upstream pixels, vendored from OpenSimulator (see
    // `opensim-assets/README.md`) rather than stood in for: unlike the sky and
    // water above, a bump map's *content* is what the renderer samples, so a
    // flat stand-in would silently render every bumped face smooth.
    textures.extend(vendored_textures());

    // The water plane's own two textures are not in OpenSimulator's set, so
    // they keep a stand-in: a translucent blue-green, so water reads as water.
    for id in sl_proto::BUILTIN_WATER_PLANE_TEXTURES {
        textures.push((id, RgbaImage::solid(FLAT_SIZE, WATER_PLANE_RGBA).j2c()?));
    }

    Ok(textures)
}

/// The vendored upstream textures, embedded so the fixture crate stays free of
/// filesystem access like the rest of it.
///
/// The bump maps are listed in `std_bump.ini` order, which is the order the
/// bumpiness enum indexes them, and paired with the ids `sl-proto` names — so a
/// wrongly paired file is a compile error at the `zip`, not a wrong texture on
/// a face. See `opensim-assets/README.md` for provenance and licence; the files
/// are unmodified, which is what the licence claim there rests on.
fn vendored_textures() -> Vec<(Uuid, Vec<u8>)> {
    /// The fifteen standard bump maps, in `std_bump.ini` order.
    const BUMPMAPS: [&[u8]; 15] = [
        include_bytes!("../../opensim-assets/textures/058c75c0-a0d5-f2f8-43f3-e9699a89c2fc.j2c"),
        include_bytes!("../../opensim-assets/textures/6c9fa78a-1c69-2168-325b-3e03ffa348ce.j2c"),
        include_bytes!("../../opensim-assets/textures/b8eed5f0-64b7-6e12-b67f-43fa8e773440.j2c"),
        include_bytes!("../../opensim-assets/textures/9deab416-9c63-78d6-d558-9a156f12044c.j2c"),
        include_bytes!("../../opensim-assets/textures/db9d39ec-a896-c287-1ced-64566217021e.j2c"),
        include_bytes!("../../opensim-assets/textures/f2d7b6f6-4200-1e9a-fd5b-96459e950f94.j2c"),
        include_bytes!("../../opensim-assets/textures/d9258671-868f-7511-c321-7baef9e948a4.j2c"),
        include_bytes!("../../opensim-assets/textures/d21e44ca-ff1c-a96e-b2ef-c0753426b7d9.j2c"),
        include_bytes!("../../opensim-assets/textures/4726f13e-bd07-f2fb-feb0-bfa2ac58ab61.j2c"),
        include_bytes!("../../opensim-assets/textures/e569711a-27c2-aad4-9246-0c910239a179.j2c"),
        include_bytes!("../../opensim-assets/textures/073c9723-540c-5449-cdd4-0e87fdc159e3.j2c"),
        include_bytes!("../../opensim-assets/textures/ae874d1a-93ef-54fb-5fd3-eb0cb156afc0.j2c"),
        include_bytes!("../../opensim-assets/textures/92e66e00-f56f-598a-7997-048aa64cde18.j2c"),
        include_bytes!("../../opensim-assets/textures/83b77fc6-10b4-63ec-4de7-f40629f238c5.j2c"),
        include_bytes!("../../opensim-assets/textures/735198cf-6ea0-2550-e222-21d3c6a341ae.j2c"),
    ];
    /// `IMG_SMOKE` and `IMG_FACE_SELECT`, in that order.
    const VIEWER: [&[u8]; 2] = [
        include_bytes!("../../opensim-assets/textures/b4ba225c-373f-446d-9f7e-6cb7b5cf9b3d.j2c"),
        include_bytes!("../../opensim-assets/textures/a85ac674-cb75-4af6-9499-df7c5aaf7a28.j2c"),
    ];

    sl_proto::BUILTIN_BUMPMAP_TEXTURES
        .into_iter()
        .zip(BUMPMAPS)
        .chain(sl_proto::BUILTIN_VIEWER_TEXTURES.into_iter().zip(VIEWER))
        .map(|(id, bytes)| (id, bytes.to_vec()))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{
        FLAT_NORMAL_RGBA, HALO_BAND_CENTRE, SHAPED_SIZE, cloud_noise, flat_wave_normal, halo_ring,
        library_textures, moon_disc, rainbow_band, star_bloom, sun_disc,
    };
    use pretty_assertions::assert_eq;

    type TestError = Box<dyn core::error::Error>;

    /// The whole set is answered, once each, and the ids are the ones the
    /// renderer falls back to — not a private copy that could drift from them.
    #[test]
    fn the_stand_ins_cover_every_built_in_id() -> Result<(), TestError> {
        let textures = library_textures()?;
        let ids: Vec<_> = textures.iter().map(|(id, _bytes)| *id).collect();
        for id in sl_proto::BUILTIN_ENVIRONMENT_TEXTURES {
            assert!(ids.contains(&id), "no stand-in for built-in texture {id}");
        }
        // The sets the viewer fetches unconditionally on arrival. Each is
        // listed by its sl-proto constant rather than by literal id, so adding
        // an id there without an image here fails this rather than showing up
        // as a 404 in a capture run.
        for id in sl_proto::BUILTIN_BUMPMAP_TEXTURES {
            assert!(ids.contains(&id), "no image for standard bump map {id}");
        }
        for id in sl_proto::BUILTIN_VIEWER_TEXTURES {
            assert!(ids.contains(&id), "no image for viewer texture {id}");
        }
        for id in sl_proto::BUILTIN_WATER_PLANE_TEXTURES {
            assert!(
                ids.contains(&id),
                "no stand-in for water plane texture {id}"
            );
        }
        assert!(ids.contains(&sl_proto::avatar_texture::IMG_DEFAULT_AVATAR));
        assert!(ids.contains(&sl_proto::avatar_texture::IMG_INVISIBLE));
        assert!(ids.contains(&sl_proto::DEFAULT_PRIM_TEXTURE));
        let mut unique = ids.clone();
        unique.sort_unstable();
        unique.dedup();
        assert_eq!(unique.len(), ids.len(), "a built-in id was answered twice");
        for (id, bytes) in &textures {
            assert!(
                !bytes.is_empty(),
                "the stand-in for {id} encoded to nothing"
            );
        }
        Ok(())
    }

    /// A disc is a disc: opaque at the centre, gone at the corners. A solid
    /// square would render as a square sun, which is the whole failure this
    /// replaces.
    #[test]
    fn the_sun_and_moon_are_discs_rather_than_squares() -> Result<(), TestError> {
        for image in [sun_disc(SHAPED_SIZE), moon_disc(SHAPED_SIZE)] {
            let centre = image
                .pixel(SHAPED_SIZE / 2, SHAPED_SIZE / 2)
                .ok_or("centre")?;
            assert_eq!(centre[3], u8::MAX, "the disc centre is not opaque");
            let corner = image.pixel(0, 0).ok_or("corner")?;
            assert_eq!(corner[3], 0, "the disc reaches its corners");
            // The mid-edge sits outside the rim too, so the disc is inscribed.
            let edge = image
                .pixel(SHAPED_SIZE / 2, SHAPED_SIZE - 1)
                .ok_or("edge")?;
            assert_eq!(edge[3], 0, "the disc touches its own edge");
        }
        Ok(())
    }

    /// The moon's transparent surround carries the reference's own grey, which
    /// is what `moonF.glsl` was written against.
    #[test]
    fn the_moon_surround_is_the_reference_transparent_grey() -> Result<(), TestError> {
        let corner = moon_disc(SHAPED_SIZE).pixel(0, 0).ok_or("corner")?;
        assert_eq!(corner, [0x55, 0x55, 0x55, 0x00]);
        Ok(())
    }

    /// The cloud blob reaches zero on every edge, so the sky tiles it without a
    /// seam, and is bright in the middle, so it has structure to tile at all.
    #[test]
    fn the_cloud_noise_tiles_without_a_seam() -> Result<(), TestError> {
        let image = cloud_noise(SHAPED_SIZE);
        let red = |x, y| image.pixel(x, y).map(|[r, _g, _b, _a]| r);
        assert_eq!(red(0, 0), Some(0));
        assert_eq!(red(SHAPED_SIZE / 2, 0), Some(0));
        assert_eq!(red(0, SHAPED_SIZE / 2), Some(0));
        let centre = red(SHAPED_SIZE / 2, SHAPED_SIZE / 2).ok_or("centre")?;
        assert!(
            centre > 200,
            "the cloud blob has no bright centre ({centre})"
        );
        Ok(())
    }

    /// The halo's bright band sits on the row `skyF.glsl` samples the 22° ring
    /// at, and nowhere else — a wash over the whole texture would put a glow
    /// across the entire sky.
    #[test]
    fn the_halo_band_sits_at_the_twenty_two_degree_radius() -> Result<(), TestError> {
        let image = halo_ring(SHAPED_SIZE);
        let luminance = |v: f32| {
            let row = crate::round_to_u8(v * 255.0);
            let row = u32::from(row) * SHAPED_SIZE / 256;
            image.pixel(0, row).map(|[r, _g, _b, _a]| r)
        };
        let on_band = luminance(HALO_BAND_CENTRE).ok_or("band")?;
        assert!(on_band > 200, "the halo band is not bright ({on_band})");
        assert_eq!(luminance(0.9), Some(0), "the halo glows off its ring");
        Ok(())
    }

    /// The rainbow's band runs through the spectrum and is black outside it.
    #[test]
    fn the_rainbow_band_sweeps_the_spectrum() -> Result<(), TestError> {
        let image = rainbow_band(SHAPED_SIZE);
        let row = |v: f32| {
            let row = u32::from(crate::round_to_u8(v * 255.0)) * SHAPED_SIZE / 256;
            image.pixel(0, row)
        };
        // Red on the outside of the bow, blue-violet on the inside.
        let outer = row(0.17).ok_or("outer")?;
        assert!(
            outer[0] > outer[2],
            "the outside of the bow is not red ({outer:?})"
        );
        let inner = row(0.43).ok_or("inner")?;
        assert!(
            inner[2] > inner[0],
            "the inside of the bow is not violet ({inner:?})"
        );
        assert_eq!(row(0.8), Some([0, 0, 0, u8::MAX]), "the bow is not banded");
        Ok(())
    }

    /// The bloom is a point on black: additive blending makes anything else a
    /// square of light behind every star.
    #[test]
    fn the_star_bloom_falls_off_to_black() -> Result<(), TestError> {
        let image = star_bloom(SHAPED_SIZE);
        let centre = image
            .pixel(SHAPED_SIZE / 2, SHAPED_SIZE / 2)
            .ok_or("centre")?;
        // Not exactly 255: an even-sided image has no pixel *at* the centre, so
        // the brightest one sits half a texel out.
        assert!(
            centre.iter().all(|channel| *channel > 250),
            "the bloom has no bright centre ({centre:?})"
        );
        assert_eq!(image.pixel(0, 0), Some([0, 0, 0, 0]));
        Ok(())
    }

    /// The wave normal is flat everywhere, so it adds no shape of its own.
    #[test]
    fn the_wave_normal_is_flat() {
        let image = flat_wave_normal(8);
        for y in 0..8 {
            for x in 0..8 {
                assert_eq!(image.pixel(x, y), Some(FLAT_NORMAL_RGBA));
            }
        }
    }
}
