//! Ogg Vorbis encoding of Second Life / OpenSim **sound assets**.
//!
//! `AssetType::Sound` is Ogg Vorbis on both grids: a short, usually mono clip a
//! simulator serves over `ViewerAsset` and a viewer plays through its mixer.
//! Decoding one is the mixer's job — `sl-audio`'s `decode_clip` hands
//! `symphonium` the bytes and gets back a resampled sample resource — but
//! *writing* one has no engine ties at all, so it lives here, where a fixture
//! generator, a fake grid and (eventually) the viewer's own sound upload can
//! reach it without linking an audio device.
//!
//! [`encode_sl_sound`] is the exact shape of the reference viewer's
//! `encode_vorbis_file` (`indra/llaudio/llvorbisencode.cpp`): mono, 44.1 kHz,
//! quality `0.05` — the level Linden picked in SL-52913 as "good enough at a
//! nice low bitrate", equivalent to `oggenc -q0.5`. [`encode_vorbis`] is the
//! same encoder without the upload rules, for a fixture that wants a different
//! rate or a stereo signal.
//!
//! # Determinism
//!
//! The Ogg stream serial is [`STREAM_SERIAL`], a constant, rather than the
//! random one the Ogg specification suggests. A serial only exists to tell two
//! *chained* logical bitstreams apart, and a sound asset is always exactly one;
//! in exchange, the same samples always encode to the same bytes, which is what
//! lets a fixture asset be compared to a recorded one and a seeded run repeat
//! itself.

use core::num::{NonZeroU8, NonZeroU32};

use vorbis_rs::{VorbisBitrateManagementStrategy, VorbisEncoderBuilder};

/// The sample rate every Second Life sound asset is stored at
/// (`LLVORBIS_CLIP_SAMPLE_RATE`). The reference viewer rejects an upload at any
/// other rate rather than resampling it.
pub const SL_SAMPLE_RATE: NonZeroU32 = match NonZeroU32::new(44_100) {
    Some(rate) => rate,
    // Unreachable: the literal is non-zero. `NonZeroU32::new(…).unwrap()` would
    // say this in one line but is not available under the workspace lints.
    None => NonZeroU32::MIN,
};

/// The perceptual quality [`encode_sl_sound`] encodes at, the reference
/// viewer's own (`llvorbisencode.cpp`: "SL-52913 & SL-53779 determined this
/// quality level to be our 'good enough' general-purpose quality level with a
/// nice low bitrate").
pub const SL_QUALITY: f32 = 0.05;

/// The longest sound Second Life accepts, in seconds (`LLVORBIS_CLIP_MAX_TIME`).
///
/// This is the *Second Life* limit on purpose: OpenSim grids commonly raise it
/// to 60 s, which Firestorm carries as `LLVORBIS_CLIP_MAX_TIME_OPENSIM`. A clip
/// written for the stricter grid plays on both.
pub const SL_MAX_SECONDS: u32 = 30;

/// The most channels a Second Life sound may have
/// (`LLVORBIS_CLIP_MAX_CHANNELS`). Note that the reference viewer encodes even
/// a stereo source down to mono on upload; this is the limit on what it will
/// *read*.
pub const SL_MAX_CHANNELS: usize = 2;

/// The Ogg stream serial every sound this crate writes carries — see the
/// determinism note in the crate documentation.
pub const STREAM_SERIAL: i32 = 0x534C_0001;

/// How many sample frames are handed to libvorbis at a time. The library's own
/// documentation calls 1024 "a reasonable choice"; blocks far larger than the
/// 8192-sample maximum encoding window degrade sharply.
const BLOCK_FRAMES: usize = 1024;

/// Why a sound could not be encoded.
#[derive(Debug, thiserror::Error)]
pub enum EncodeError {
    /// No channels were given, so there is no signal to encode.
    #[error("cannot encode a sound with no channels")]
    NoChannels,
    /// More channels were given than a Second Life sound may carry.
    #[error("a sound has at most 2 channels, not {got}")]
    TooManyChannels {
        /// The channel count that was given.
        got: usize,
    },
    /// The channels are not all the same length, so there is no frame count.
    #[error("channel {channel} holds {got} samples but the first holds {expected}")]
    RaggedChannels {
        /// The index of the channel whose length differs.
        channel: usize,
        /// That channel's length.
        got: usize,
        /// The first channel's length, which every channel must match.
        expected: usize,
    },
    /// The channels hold no samples.
    #[error("cannot encode a sound with no samples")]
    Empty,
    /// The clip is longer than the grid accepts.
    #[error("a Second Life sound is at most {max_frames} sample frames long, not {got}")]
    TooLong {
        /// The frame count that was given.
        got: usize,
        /// The most frames the grid accepts at this sample rate.
        max_frames: usize,
    },
    /// libvorbis or the Ogg framer refused the signal.
    #[error("Vorbis encode failed: {0}")]
    Codec(String),
}

/// Encodes a **mono** signal as the sound asset Second Life accepts: 44.1 kHz
/// ([`SL_SAMPLE_RATE`]) at quality [`SL_QUALITY`], no longer than
/// [`SL_MAX_SECONDS`].
///
/// `samples` are in `-1.0..=1.0` (Vorbis does not enforce the range, but
/// anything outside it clips on playback).
///
/// # Errors
///
/// [`EncodeError::Empty`] for a signal with no samples,
/// [`EncodeError::TooLong`] for one past the grid's clip limit, and
/// [`EncodeError::Codec`] if libvorbis refuses it.
pub fn encode_sl_sound(samples: &[f32]) -> Result<Vec<u8>, EncodeError> {
    let max_frames =
        usize::try_from(SL_SAMPLE_RATE.get().saturating_mul(SL_MAX_SECONDS)).unwrap_or(usize::MAX);
    if samples.len() > max_frames {
        return Err(EncodeError::TooLong {
            got: samples.len(),
            max_frames,
        });
    }
    encode_vorbis(&[samples], SL_SAMPLE_RATE, SL_QUALITY)
}

/// Encodes planar `channels` (one slice of samples per channel, all the same
/// length) as an Ogg Vorbis stream at `sample_rate`.
///
/// `quality` is libvorbis' perceptual quality factor in `-0.2..=1.0`; pass
/// [`SL_QUALITY`] to match what the reference viewer uploads. Unlike
/// [`encode_sl_sound`] this imposes no clip-length limit, because a fixture is
/// allowed to write something a grid would refuse — but it still refuses more
/// than [`SL_MAX_CHANNELS`] channels, since nothing downstream could play them.
///
/// # Errors
///
/// [`EncodeError::NoChannels`], [`EncodeError::TooManyChannels`],
/// [`EncodeError::RaggedChannels`] or [`EncodeError::Empty`] for a signal that
/// is not a sound, and [`EncodeError::Codec`] if libvorbis refuses the
/// parameters (an out-of-range `quality`, say).
pub fn encode_vorbis(
    channels: &[&[f32]],
    sample_rate: NonZeroU32,
    quality: f32,
) -> Result<Vec<u8>, EncodeError> {
    let frames = channels.first().ok_or(EncodeError::NoChannels)?.len();
    if channels.len() > SL_MAX_CHANNELS {
        return Err(EncodeError::TooManyChannels {
            got: channels.len(),
        });
    }
    if frames == 0 {
        return Err(EncodeError::Empty);
    }
    for (channel, samples) in channels.iter().enumerate() {
        if samples.len() != frames {
            return Err(EncodeError::RaggedChannels {
                channel,
                got: samples.len(),
                expected: frames,
            });
        }
    }
    let channel_count = u8::try_from(channels.len())
        .ok()
        .and_then(NonZeroU8::new)
        .ok_or(EncodeError::NoChannels)?;

    let mut builder = VorbisEncoderBuilder::new_with_serial(
        sample_rate,
        channel_count,
        Vec::new(),
        STREAM_SERIAL,
    );
    let mut encoder = builder
        .bitrate_management_strategy(VorbisBitrateManagementStrategy::QualityVbr {
            target_quality: quality,
        })
        .build()
        .map_err(|error| EncodeError::Codec(error.to_string()))?;

    let mut start = 0_usize;
    while start < frames {
        let end = start.saturating_add(BLOCK_FRAMES).min(frames);
        let block: Vec<&[f32]> = channels
            .iter()
            .map(|samples| samples.get(start..end).unwrap_or(&[]))
            .collect();
        encoder
            .encode_audio_block(&block)
            .map_err(|error| EncodeError::Codec(error.to_string()))?;
        start = end;
    }
    encoder
        .finish()
        .map_err(|error| EncodeError::Codec(error.to_string()))
}

#[cfg(test)]
mod tests {
    use super::{
        EncodeError, SL_MAX_SECONDS, SL_QUALITY, SL_SAMPLE_RATE, encode_sl_sound, encode_vorbis,
    };
    use core::num::NonZeroU32;
    use pretty_assertions::assert_eq;
    use vorbis_rs::VorbisDecoder;

    /// The error type the tests bubble out through `?`.
    type TestError = Box<dyn core::error::Error>;

    /// A sine of `frequency` hertz, `frames` long, at [`SL_SAMPLE_RATE`].
    fn sine(frequency: f32, frames: usize) -> Vec<f32> {
        let step = core::f32::consts::TAU * frequency / rate();
        let mut phase = 0.0_f32;
        let mut samples = Vec::with_capacity(frames);
        for _frame in 0..frames {
            samples.push(phase.sin() * 0.5);
            phase += step;
            if phase >= core::f32::consts::TAU {
                phase -= core::f32::consts::TAU;
            }
        }
        samples
    }

    /// [`SL_SAMPLE_RATE`] as an `f32` (it is small enough to be exact).
    fn rate() -> f32 {
        f32::from(u16::try_from(SL_SAMPLE_RATE.get()).unwrap_or(u16::MAX))
    }

    /// Decodes `ogg` back to its planar channels through libvorbis itself.
    fn decode(ogg: &[u8]) -> Result<Vec<Vec<f32>>, TestError> {
        let mut decoder = VorbisDecoder::new(std::io::Cursor::new(ogg.to_vec()))?;
        let mut channels: Vec<Vec<f32>> = Vec::new();
        while let Some(block) = decoder.decode_audio_block()? {
            for (index, samples) in block.samples().iter().enumerate() {
                if channels.len() <= index {
                    channels.resize(index.saturating_add(1), Vec::new());
                }
                if let Some(channel) = channels.get_mut(index) {
                    channel.extend_from_slice(samples);
                }
            }
        }
        Ok(channels)
    }

    /// The magnitude of `samples` at `frequency`, by the Goertzel recurrence —
    /// a one-bin discrete Fourier transform, which is all a test that asks
    /// "is this the tone it was written as" needs.
    fn magnitude_at(samples: &[f32], frequency: f32) -> f32 {
        let omega = core::f32::consts::TAU * frequency / rate();
        let coefficient = 2.0 * omega.cos();
        let (mut previous, mut older) = (0.0_f32, 0.0_f32);
        for sample in samples {
            let current = *sample + coefficient * previous - older;
            older = previous;
            previous = current;
        }
        (previous * previous + older * older - coefficient * previous * older)
            .max(0.0)
            .sqrt()
    }

    /// A quarter-second 440 Hz tone survives the encoder: the stream is Ogg, it
    /// decodes back to as many frames as went in, and the loudest thing in it
    /// is still 440 Hz rather than one of its neighbours.
    #[test]
    fn a_tone_survives_the_round_trip() -> Result<(), TestError> {
        let frames = 11_025;
        let ogg = encode_sl_sound(&sine(440.0, frames))?;
        assert_eq!(ogg.get(0..4), Some(b"OggS".as_slice()));

        let decoded = decode(&ogg)?;
        assert_eq!(decoded.len(), 1, "the encoder wrote a mono stream");
        let samples = decoded.first().ok_or("no channel")?;
        assert_eq!(samples.len(), frames, "the frame count changed");

        let wanted = magnitude_at(samples, 440.0);
        for other in [220.0_f32, 880.0] {
            let neighbour = magnitude_at(samples, other);
            assert!(
                wanted > neighbour * 8.0,
                "440 Hz ({wanted}) is not clearly louder than {other} Hz ({neighbour})"
            );
        }
        Ok(())
    }

    /// The same samples encode to the same bytes, so a fixture asset is a
    /// constant rather than something that changes every run.
    #[test]
    fn encoding_is_deterministic() -> Result<(), TestError> {
        let samples = sine(660.0, 4_096);
        assert_eq!(encode_sl_sound(&samples)?, encode_sl_sound(&samples)?);
        Ok(())
    }

    /// A stereo signal keeps both channels, so the fixture path is not
    /// silently mono-only.
    #[test]
    fn a_stereo_signal_keeps_both_channels() -> Result<(), TestError> {
        let left = sine(440.0, 4_096);
        let right = sine(880.0, 4_096);
        let ogg = encode_vorbis(
            &[left.as_slice(), right.as_slice()],
            SL_SAMPLE_RATE,
            SL_QUALITY,
        )?;
        let decoded = decode(&ogg)?;
        assert_eq!(decoded.len(), 2);
        let left = decoded.first().ok_or("no left channel")?;
        let right = decoded.get(1).ok_or("no right channel")?;
        assert!(magnitude_at(left, 440.0) > magnitude_at(left, 880.0));
        assert!(magnitude_at(right, 880.0) > magnitude_at(right, 440.0));
        Ok(())
    }

    /// A signal that is not a sound is refused rather than encoded to
    /// something a decoder would reject later.
    #[test]
    fn a_malformed_signal_is_refused() {
        assert!(matches!(
            encode_vorbis(&[], SL_SAMPLE_RATE, SL_QUALITY),
            Err(EncodeError::NoChannels)
        ));
        assert!(matches!(
            encode_vorbis(&[&[]], SL_SAMPLE_RATE, SL_QUALITY),
            Err(EncodeError::Empty)
        ));
        assert!(matches!(
            encode_vorbis(&[&[0.0, 0.0], &[0.0], &[0.0]], SL_SAMPLE_RATE, SL_QUALITY),
            Err(EncodeError::TooManyChannels { got: 3 })
        ));
        assert!(matches!(
            encode_vorbis(&[&[0.0, 0.0], &[0.0]], SL_SAMPLE_RATE, SL_QUALITY),
            Err(EncodeError::RaggedChannels {
                channel: 1,
                got: 1,
                expected: 2
            })
        ));
        // One frame past the grid's clip limit, which is refused before
        // libvorbis ever sees it.
        let limit = usize::try_from(SL_SAMPLE_RATE.get())
            .unwrap_or(0)
            .saturating_mul(usize::try_from(SL_MAX_SECONDS).unwrap_or(0));
        let too_long = vec![0.0_f32; limit.saturating_add(1)];
        assert!(matches!(
            encode_sl_sound(&too_long),
            Err(EncodeError::TooLong { got, max_frames }) if got == limit.saturating_add(1) && max_frames == limit
        ));
    }

    /// A rate other than the grid's still encodes — a fixture may write one,
    /// even though an upload may not.
    #[test]
    fn a_non_grid_sample_rate_encodes() -> Result<(), TestError> {
        let rate = NonZeroU32::new(22_050).unwrap_or(SL_SAMPLE_RATE);
        let samples = vec![0.0_f32; 2_048];
        let ogg = encode_vorbis(&[samples.as_slice()], rate, SL_QUALITY)?;
        let decoder = VorbisDecoder::new(std::io::Cursor::new(ogg))?;
        assert_eq!(decoder.sampling_frequency(), rate);
        Ok(())
    }
}
