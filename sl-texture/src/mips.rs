//! A decoded texture's **mip chain**: every level below its full image, each a
//! 2×2 box average of the one above, down to 1×1.
//!
//! The reference builds one for every fetched texture
//! (`LLViewerFetchedTexture` defaults to mipmaps, and `LLImageGL::setImage`
//! calls `glGenerateMipmap` after the upload). Without one, a face whose texels
//! are smaller than a pixel is sampled from the full image alone, and a distant
//! or oblique texture aliases and shimmers instead of settling to its average.
//!
//! Built here, where the pixels are made, rather than where they are uploaded:
//! the store decodes on its CPU pool, and a chain built there costs the frame
//! nothing. [`mip_chain`] is the one implementation, used by that pool and by
//! any upload of pixels that did not come through it.

use bytes::Bytes;

/// The levels of an RGBA8 image below its first: how many there are in all,
/// and their texels.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MipChain {
    /// How many levels the whole chain has, **counting the full image**: what a
    /// GPU texture's `mip_level_count` is.
    pub levels: u32,
    /// Every level after the first, largest first, each tightly packed RGBA8
    /// and row-major: the bytes an upload appends to the full image's.
    pub below: Bytes,
}

/// The mip chain below `pixels` — tightly packed RGBA8, `width` × `height` —
/// or `None` when there is none: a 1×1 image is its own last level, and a
/// buffer that is not `width * height * 4` bytes holds no image to average.
///
/// Each level is the one above averaged over 2×2 blocks, each side halved and
/// rounded down, never below one texel: an odd last row or column is left out
/// of the level below, and a side already one texel wide pairs each texel with
/// itself. The average is of the **stored bytes**, sRGB-encoded or not,
/// because that is what the reference averages: its fetched textures are plain
/// `GL_RGBA8`, decoded from sRGB in the shader, so `glGenerateMipmap` filters
/// the encoded values. A far face therefore settles to the colour it does
/// there.
#[must_use]
pub fn mip_chain(width: u32, height: u32, pixels: &[u8]) -> Option<MipChain> {
    let (Ok(mut w), Ok(mut h)) = (usize::try_from(width), usize::try_from(height)) else {
        return None;
    };
    let expected = w.checked_mul(h).and_then(|texels| texels.checked_mul(4));
    if w == 0 || h == 0 || expected != Some(pixels.len()) || (w == 1 && h == 1) {
        return None;
    }
    // Every level below the first is a third of the first's size, all told.
    let mut below = Vec::with_capacity((pixels.len() / 3).saturating_add(4));
    let (next_w, next_h, mut level) = half_level(pixels, w, h);
    (w, h) = (next_w, next_h);
    let mut levels: u32 = 2;
    below.extend_from_slice(&level);
    while w > 1 || h > 1 {
        let (next_w, next_h, next) = half_level(&level, w, h);
        below.extend_from_slice(&next);
        (w, h, level) = (next_w, next_h, next);
        levels = levels.saturating_add(1);
    }
    Some(MipChain {
        levels,
        below: Bytes::from(below),
    })
}

/// The next mip level below the RGBA8 level `pixels` (`width` × `height`, both
/// at least one): its width, height and texels, each the rounded average of the
/// 2×2 block above it.
///
/// Walks the level a row pair at a time and a texel pair at a time, into a
/// buffer sized up front, rather than addressing each sample by index: this
/// shape is what the compiler can vectorise, and it is several times faster.
fn half_level(pixels: &[u8], width: usize, height: usize) -> (usize, usize, Vec<u8>) {
    let half_w = (width / 2).max(1);
    let half_h = (height / 2).max(1);
    let rows: Vec<&[u8]> = pixels.chunks_exact(width.saturating_mul(4)).collect();
    let mut out = vec![0_u8; half_w.saturating_mul(half_h).saturating_mul(4)];
    for (pair, out_row) in out.chunks_exact_mut(half_w.saturating_mul(4)).enumerate() {
        let top_index = pair.saturating_mul(2);
        let Some(top) = rows.get(top_index).copied() else {
            break;
        };
        // An odd last row is left out; a one-row level pairs a row with itself.
        let bottom = rows
            .get(top_index.saturating_add(1))
            .copied()
            .unwrap_or(top);
        let out_texels = out_row.as_chunks_mut::<4>().0;
        if width == 1 {
            // A one-column level: each texel pairs with itself across.
            for ((texel, upper), lower) in out_texels
                .iter_mut()
                .zip(top.as_chunks::<4>().0)
                .zip(bottom.as_chunks::<4>().0)
            {
                *texel = average_block(*upper, *upper, *lower, *lower);
            }
            continue;
        }
        // An odd last column is left out: `as_chunks` stops at the last whole
        // pair, which is the `width / 2` the level is wide.
        for ((texel, upper), lower) in out_texels
            .iter_mut()
            .zip(top.as_chunks::<8>().0)
            .zip(bottom.as_chunks::<8>().0)
        {
            let [r0, g0, b0, a0, r1, g1, b1, a1] = *upper;
            let [r2, g2, b2, a2, r3, g3, b3, a3] = *lower;
            *texel = average_block(
                [r0, g0, b0, a0],
                [r1, g1, b1, a1],
                [r2, g2, b2, a2],
                [r3, g3, b3, a3],
            );
        }
    }
    (half_w, half_h, out)
}

/// The rounded per-channel average of four RGBA8 texels.
///
/// Wrapping arithmetic because it cannot wrap — four bytes and the rounding
/// term sum to at most 1022, a quarter of which always fits a byte — and,
/// unlike saturating arithmetic, it does not stop the loops above from
/// vectorising.
fn average_block(a: [u8; 4], b: [u8; 4], c: [u8; 4], d: [u8; 4]) -> [u8; 4] {
    let average = |w: u8, x: u8, y: u8, z: u8| -> u8 {
        let sum = u16::from(w)
            .wrapping_add(u16::from(x))
            .wrapping_add(u16::from(y))
            .wrapping_add(u16::from(z))
            .wrapping_add(2);
        u8::try_from(sum >> 2_u8).unwrap_or(u8::MAX)
    };
    let [a_red, a_green, a_blue, a_alpha] = a;
    let [b_red, b_green, b_blue, b_alpha] = b;
    let [c_red, c_green, c_blue, c_alpha] = c;
    let [d_red, d_green, d_blue, d_alpha] = d;
    [
        average(a_red, b_red, c_red, d_red),
        average(a_green, b_green, c_green, d_green),
        average(a_blue, b_blue, c_blue, d_blue),
        average(a_alpha, b_alpha, c_alpha, d_alpha),
    ]
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::{MipChain, mip_chain};

    /// A chain runs down to 1×1: an 8×4 image has 4×2, 2×1 and 1×1 below it,
    /// four levels in all.
    #[test]
    fn a_chain_runs_down_to_one_texel() {
        let pixels: Vec<u8> = (0..128_u8).collect();
        let chain = mip_chain(8, 4, &pixels);
        assert_eq!(chain.as_ref().map(|chain| chain.levels), Some(4));
        assert_eq!(
            chain.map(|chain| chain.below.len()),
            Some((4 * 2 + 2 + 1) * 4)
        );
    }

    /// Each level averages the 2×2 block above it, per channel and rounded, in
    /// the stored bytes — sRGB-encoded or not, as the reference's
    /// `glGenerateMipmap` does on its plain `GL_RGBA8` textures.
    #[test]
    fn a_level_averages_the_block_above_it() {
        // One 2×2 block: black, white, and two reds of different alpha.
        let pixels = [
            0, 0, 0, 255, /**/ 255, 255, 255, 255, //
            255, 0, 0, 0, /**/ 255, 0, 0, 100,
        ];
        // (0 + 255 + 255 + 255 + 2) / 4 = 191; (0 + 255 + 0 + 0 + 2) / 4 = 64;
        // (255 + 255 + 0 + 100 + 2) / 4 = 153.
        assert_eq!(
            mip_chain(2, 2, &pixels),
            Some(MipChain {
                levels: 2,
                below: vec![191, 64, 64, 153].into(),
            })
        );
    }

    /// An odd side is halved rounding down, so its last texel is left out of
    /// the level below; a side already one texel wide pairs each texel with
    /// itself.
    #[test]
    fn an_odd_or_thin_level_halves_rounding_down() {
        // 3×1: 30, 60, 90 in every channel. The next level is floor(3 / 2) = 1
        // texel wide and averages the first pair only, each with itself below.
        let pixels: Vec<u8> = [30_u8, 60, 90]
            .iter()
            .flat_map(|value| [*value; 4])
            .collect();
        assert_eq!(
            mip_chain(3, 1, &pixels),
            Some(MipChain {
                levels: 2,
                below: vec![45; 4].into(),
            })
        );
        // 1×2: a column pairs each texel with itself across.
        let column = [10, 10, 10, 10, 30, 30, 30, 30];
        assert_eq!(
            mip_chain(1, 2, &column),
            Some(MipChain {
                levels: 2,
                below: vec![20; 4].into(),
            })
        );
    }

    /// A 1×1 image is already its last level, and a buffer that does not hold
    /// `width * height` texels holds no image to average: neither has a chain.
    #[test]
    fn a_single_texel_or_a_short_buffer_has_no_chain() {
        assert_eq!(mip_chain(1, 1, &[1, 2, 3, 4]), None);
        assert_eq!(mip_chain(4, 4, &[7; 12]), None);
        assert_eq!(mip_chain(0, 4, &[]), None);
    }
}
