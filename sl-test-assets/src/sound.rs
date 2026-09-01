//! Procedural Second Life **sound assets**: short Ogg Vorbis tones a fake grid
//! can serve and a viewer can play.
//!
//! A tone rather than noise, because a tone is *assertable*. The pixel oracles
//! classify a texture by its dominant colour channel; the audio oracle
//! classifies a clip by its dominant frequency, and two fixture sounds an
//! octave apart are as distinguishable to a decode test as red and green are to
//! a capture. White noise supports neither.
//!
//! The bytes are written by [`sl_sound`], which wraps the same libvorbis a real
//! viewer's decoder reads — so what a fixture serves is a genuine sound asset,
//! not something only our own decoder would accept. That is the whole point:
//! `sl-audio` decodes through `symphonium`, and a fixture that only satisfied
//! `symphonium` would prove nothing about a real grid's bytes.

use core::num::NonZeroU32;

use sl_sound::EncodeError;

/// The frequencies the fixture tones are written at — the audio counterpart of
/// [`markers`](crate::markers), an octave apart so a decode test can tell one
/// from another with a single-bin Fourier transform.
pub mod tones {
    /// 220 Hz (A3).
    pub const LOW: f32 = 220.0;
    /// 440 Hz (A4, concert pitch) — the middle of the three.
    pub const MID: f32 = 440.0;
    /// 880 Hz (A5).
    pub const HIGH: f32 = 880.0;
}

/// How long a [`marker_tone`] lasts, in seconds.
///
/// Short enough to be a UI sound or a footstep — the class of clip a viewer
/// actually triggers dozens of at once — and long enough that a decode test
/// measuring its pitch has thousands of samples to work with.
pub const TONE_SECONDS: f32 = 0.25;

/// The peak amplitude of a fixture tone. Half scale, so mixing two of them
/// cannot clip.
const AMPLITUDE: f32 = 0.5;

/// How long the fade in and out at each end of a tone lasts, in seconds.
///
/// A sine cut off mid-cycle ends on a step, which is an audible click and a
/// broadband smear in the spectrum the fixture is supposed to have a single
/// peak in. Five milliseconds removes both without moving the peak.
const FADE_SECONDS: f32 = 0.005;

/// The mono samples of a `frequency_hz` sine lasting `seconds` at
/// `sample_rate`, with a five-millisecond fade in and out — a sine cut off
/// mid-cycle ends on a step, which is a click and a broadband smear across the
/// one spectral peak the fixture exists to have.
///
/// This is the *signal* behind [`tone`]; a test that wants to compare a decoded
/// clip against what was written asks for it here rather than decoding the
/// asset twice.
///
/// A `sample_rate` above 65 535 Hz is treated as 65 535 — no sound asset has
/// one, and clamping keeps the frame count exactly representable.
#[must_use]
pub fn tone_samples(frequency_hz: f32, seconds: f32, sample_rate: NonZeroU32) -> Vec<f32> {
    let rate = f32::from(u16::try_from(sample_rate.get()).unwrap_or(u16::MAX));
    let exact_frames = (seconds * rate).max(0.0);
    let frames = usize::try_from(round_to_u32(exact_frames)).unwrap_or(0);
    let step = core::f32::consts::TAU * frequency_hz / rate;
    let fade = (FADE_SECONDS * rate).max(1.0);
    let last = (exact_frames - 1.0).max(0.0);

    let mut samples = Vec::with_capacity(frames);
    let mut phase = 0.0_f32;
    // Counted as a float so no frame index has to be widened from an integer;
    // a count this small is exact in `f32` well past any sound asset's length.
    let mut position = 0.0_f32;
    for _frame in 0..frames {
        let ramp = (position / fade)
            .min((last - position) / fade)
            .clamp(0.0, 1.0);
        samples.push(phase.sin() * AMPLITUDE * ramp);
        phase += step;
        if phase >= core::f32::consts::TAU {
            phase -= core::f32::consts::TAU;
        }
        position += 1.0;
    }
    samples
}

/// A **sound asset**: `seconds` of a `frequency_hz` tone at `sample_rate`, as
/// the Ogg Vorbis bytes a grid serves for `AssetType::Sound`.
///
/// # Errors
///
/// The encoder's error — for a zero-length tone, or a sample rate libvorbis
/// refuses.
pub fn tone(
    frequency_hz: f32,
    seconds: f32,
    sample_rate: NonZeroU32,
) -> Result<Vec<u8>, EncodeError> {
    let samples = tone_samples(frequency_hz, seconds, sample_rate);
    sl_sound::encode_vorbis(&[samples.as_slice()], sample_rate, sl_sound::SL_QUALITY)
}

/// One of the [`tones`] as a sound asset at the settings a real Second Life
/// sound has: mono, 44.1 kHz, quality `0.05`, [`TONE_SECONDS`] long.
///
/// This is the fixture form — `marker_tone(tones::MID)` is to a sound what
/// `RgbaImage::solid(size, markers::RED)` is to a texture.
///
/// # Errors
///
/// The encoder's error, which [`TONE_SECONDS`] of a tone cannot produce.
pub fn marker_tone(frequency_hz: f32) -> Result<Vec<u8>, EncodeError> {
    let samples = tone_samples(frequency_hz, TONE_SECONDS, sl_sound::SL_SAMPLE_RATE);
    sl_sound::encode_sl_sound(&samples)
}

/// Rounds a non-negative value to the `u32` frame count it names.
#[expect(
    clippy::as_conversions,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "the value is clamped into 0..=u32::MAX before the cast; no From impl exists"
)]
const fn round_to_u32(value: f32) -> u32 {
    value.round().clamp(0.0, 4_294_967_295.0) as u32
}

#[cfg(test)]
mod tests {
    use super::{TONE_SECONDS, marker_tone, tone_samples, tones};
    use core::num::NonZeroU32;
    use pretty_assertions::assert_eq;

    /// The error type the tests bubble out through `?`.
    type TestError = Box<dyn core::error::Error>;

    /// [`sl_sound::SL_SAMPLE_RATE`] as an `f32`.
    fn rate() -> f32 {
        f32::from(u16::try_from(sl_sound::SL_SAMPLE_RATE.get()).unwrap_or(u16::MAX))
    }

    /// The magnitude of `samples` at `frequency`, by the Goertzel recurrence: a
    /// single-bin discrete Fourier transform, which is all "is this the tone it
    /// was written as" needs.
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

    /// Decodes an asset through **the viewer's own decoder** — `symphonium`,
    /// which is what `sl-audio`'s `decode_clip` hands the bytes to.
    fn decode(asset: &[u8]) -> Result<Vec<Vec<f32>>, TestError> {
        let probed = symphonium::probe_from_source(
            Box::new(std::io::Cursor::new(asset.to_vec())),
            None,
            None,
        )?;
        let decoded = symphonium::decode_f32(
            probed,
            &symphonium::DecodeConfig::default(),
            None,
            None,
            None,
        )?;
        Ok(decoded.data)
    }

    /// A fixture tone decodes back through `symphonium` to the tone it was
    /// written as: one channel, the frames that went in, and a spectrum whose
    /// peak is at its own frequency rather than at either of its neighbours'.
    #[test]
    fn every_marker_tone_decodes_to_its_own_pitch() -> Result<(), TestError> {
        let expected_frames =
            tone_samples(tones::MID, TONE_SECONDS, sl_sound::SL_SAMPLE_RATE).len();
        let all = [tones::LOW, tones::MID, tones::HIGH];
        for (index, wanted) in all.into_iter().enumerate() {
            let decoded = decode(&marker_tone(wanted)?)?;
            assert_eq!(decoded.len(), 1, "{wanted} Hz is not mono");
            let samples = decoded.first().ok_or("no channel")?;
            assert_eq!(
                samples.len(),
                expected_frames,
                "{wanted} Hz lost or gained frames"
            );
            let peak = magnitude_at(samples, wanted);
            // By index, so the neighbours are "the other two fixtures" rather
            // than "the frequencies that differ", which would be a float
            // comparison.
            for (other_index, other) in all.into_iter().enumerate() {
                if other_index == index {
                    continue;
                }
                let neighbour = magnitude_at(samples, other);
                assert!(
                    peak > neighbour * 8.0,
                    "{wanted} Hz ({peak}) is not clearly louder than {other} Hz ({neighbour})"
                );
            }
        }
        Ok(())
    }

    /// The tone starts and ends at silence, so a fixture sound is a tone rather
    /// than a tone bracketed by two clicks.
    #[test]
    fn a_tone_fades_in_and_out() -> Result<(), TestError> {
        let samples = tone_samples(tones::MID, TONE_SECONDS, sl_sound::SL_SAMPLE_RATE);
        let first = samples.first().ok_or("no first sample")?;
        let last = samples.last().ok_or("no last sample")?;
        assert!(first.abs() < 0.01, "the tone starts at {first}");
        assert!(last.abs() < 0.01, "the tone ends at {last}");
        // The body is at full amplitude: the fade is an envelope, not a
        // volume change.
        let loudest = samples.iter().fold(0.0_f32, |peak, s| peak.max(s.abs()));
        assert!(loudest > 0.49, "the tone peaks at only {loudest}");
        Ok(())
    }

    /// A tone at a rate other than the grid's has the frames that rate implies,
    /// so a fixture that wants a cheaper clip gets one.
    #[test]
    fn a_tone_honours_its_sample_rate() {
        let rate = NonZeroU32::new(8_000).unwrap_or(sl_sound::SL_SAMPLE_RATE);
        assert_eq!(tone_samples(tones::MID, 0.5, rate).len(), 4_000);
    }
}
