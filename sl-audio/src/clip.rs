//! Clip decode and caching.
//!
//! SL sounds are short Ogg Vorbis assets (`AssetType::Sound` /
//! `AssetType::SoundWav`). They are decoded **once**, on the asset-load path,
//! into a [`DecodedClip`] holding a firewheel sample resource, and cached by
//! asset id in a [`ClipCache`] — never decoded per trigger, and never through a
//! GStreamer pipeline (that would be milliseconds of latency and hundreds of KB
//! for a footstep, and SL fires dozens at once).
//!
//! Decoding goes through `symphonium` (firewheel's own symphonia wrapper), which
//! also resamples the clip to the mixer's device sample rate at load time so the
//! sampler never resamples per play.
#![expect(
    clippy::module_name_repetitions,
    reason = "DecodedClip / ClipCache are the natural public names, re-exported at the crate root"
)]

use std::collections::HashMap;
use std::hash::Hash;
use std::io::Cursor;
use std::num::{NonZeroU32, NonZeroUsize};

use firewheel::collector::ArcGc;
use firewheel::sample_resource::SampleResource;

use crate::error::AudioError;

/// A decoded, ready-to-play sound clip.
///
/// Cheap to clone — it holds a reference-counted firewheel sample resource, so a
/// cached clip is shared by every voice that plays it rather than copied.
#[derive(Clone)]
pub struct DecodedClip {
    /// The decoded samples as a firewheel sample resource, resampled to the
    /// mixer's device sample rate.
    resource: ArcGc<dyn SampleResource + Send + Sync + 'static>,
    /// Number of channels (1 = mono, 2 = stereo).
    channels: NonZeroUsize,
    /// The sample rate the clip was decoded to (the device rate).
    sample_rate: NonZeroU32,
    /// The number of sample frames.
    frames: u64,
}

impl std::fmt::Debug for DecodedClip {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DecodedClip")
            .field("channels", &self.channels)
            .field("sample_rate", &self.sample_rate)
            .field("frames", &self.frames)
            .finish_non_exhaustive()
    }
}

impl DecodedClip {
    /// The decoded samples as a firewheel sample resource (for the sampler
    /// node). Cloning the returned handle shares the samples.
    #[must_use]
    pub fn resource(&self) -> ArcGc<dyn SampleResource + Send + Sync + 'static> {
        self.resource.clone()
    }

    /// The number of channels.
    #[must_use]
    pub const fn channels(&self) -> NonZeroUsize {
        self.channels
    }

    /// The sample rate the clip was decoded to (the mixer's device rate).
    #[must_use]
    pub const fn sample_rate(&self) -> NonZeroU32 {
        self.sample_rate
    }

    /// The number of sample frames.
    #[must_use]
    pub const fn frames(&self) -> u64 {
        self.frames
    }

    /// The clip's duration in seconds.
    #[must_use]
    pub fn duration_seconds(&self) -> f64 {
        f64::from(u32::try_from(self.frames).unwrap_or(u32::MAX))
            / f64::from(self.sample_rate.get())
    }
}

/// Decode encoded audio bytes (Ogg Vorbis or WAV) into a [`DecodedClip`],
/// resampling to `target_sample_rate` (the mixer's device rate).
///
/// # Errors
/// Returns [`AudioError::Decode`] if the bytes are not a recognised format or
/// decoding fails.
pub fn decode_clip(
    bytes: Vec<u8>,
    target_sample_rate: NonZeroU32,
) -> Result<DecodedClip, AudioError> {
    // `Cursor<Vec<u8>>` is a `symphonia` `MediaSource` (Read + Seek + Send +
    // Sync); the probe detects Ogg/WAV from content, so no filename hint.
    let probed = symphonium::probe_from_source(Box::new(Cursor::new(bytes)), None, None)
        .map_err(|e| AudioError::Decode(e.to_string()))?;

    let decoded = symphonium::decode(
        probed,
        &symphonium::DecodeConfig::default(),
        Some(target_sample_rate),
        None,
        None,
    )
    .map_err(|e| AudioError::Decode(e.to_string()))?;

    let audio = firewheel::SymphoniumAudio(decoded);
    let channels = NonZeroUsize::new(audio.0.channels())
        .ok_or_else(|| AudioError::Decode("decoded clip has zero channels".to_owned()))?;
    let sample_rate = audio.sample_rate();
    let frames = u64::try_from(audio.0.frames()).unwrap_or(0);

    Ok(DecodedClip {
        resource: audio.into_dyn_resource(),
        channels,
        sample_rate,
        frames,
    })
}

/// A cache of decoded clips keyed by asset id (or any hashable key).
///
/// The mixer / viewer decodes a sound once and stores it here so subsequent
/// triggers reuse the shared samples. The key is generic so this crate stays
/// decoupled from the viewer's UUID type.
#[derive(Debug, Default)]
pub struct ClipCache<K> {
    /// Decoded clips by key.
    clips: HashMap<K, DecodedClip>,
}

impl<K: Eq + Hash> ClipCache<K> {
    /// An empty cache.
    #[must_use]
    pub fn new() -> Self {
        Self {
            clips: HashMap::new(),
        }
    }

    /// The number of cached clips.
    #[must_use]
    pub fn len(&self) -> usize {
        self.clips.len()
    }

    /// Whether the cache is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.clips.is_empty()
    }

    /// Whether a clip with this key is cached.
    #[must_use]
    pub fn contains(&self, key: &K) -> bool {
        self.clips.contains_key(key)
    }

    /// Fetch a cached clip.
    #[must_use]
    pub fn get(&self, key: &K) -> Option<&DecodedClip> {
        self.clips.get(key)
    }

    /// Insert (or replace) a decoded clip, returning a clone of it for
    /// immediate use.
    pub fn insert(&mut self, key: K, clip: DecodedClip) -> DecodedClip {
        self.clips.insert(key, clip.clone());
        clip
    }

    /// Remove a cached clip (e.g. on cache pressure).
    pub fn remove(&mut self, key: &K) -> Option<DecodedClip> {
        self.clips.remove(key)
    }

    /// Drop all cached clips.
    pub fn clear(&mut self) {
        self.clips.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    /// A short 16-bit PCM WAV (mono, 22050 Hz, ~0.2 s) generated with ffmpeg.
    const WAV_MONO_22050: &[u8] = include_bytes!("../tests/fixtures/tone_mono_22050.wav");
    /// A short Ogg Vorbis clip (mono, 44100 Hz, ~0.2 s) — the real SL sound
    /// format — generated with ffmpeg.
    const OGG_MONO_44100: &[u8] = include_bytes!("../tests/fixtures/tone_mono_44100.ogg");

    /// Build a `NonZeroU32` for tests without `unwrap`/`expect`.
    fn nz(v: u32) -> NonZeroU32 {
        NonZeroU32::new(v).unwrap_or(NonZeroU32::MIN)
    }

    #[test]
    fn decode_wav_no_resample() {
        let sr = nz(22_050);
        let Ok(clip) = decode_clip(WAV_MONO_22050.to_vec(), sr) else {
            unreachable!("wav fixture decodes")
        };
        assert_eq!(clip.sample_rate(), sr);
        assert_eq!(clip.channels().get(), 1);
        assert!(clip.frames() > 0);
        assert!((clip.duration_seconds() - 0.2).abs() < 0.02);
    }

    #[test]
    fn decode_ogg_vorbis() {
        // The real SL sound format decodes through the same path.
        let sr = nz(44_100);
        let Ok(clip) = decode_clip(OGG_MONO_44100.to_vec(), sr) else {
            unreachable!("ogg vorbis fixture decodes")
        };
        assert_eq!(clip.sample_rate(), sr);
        assert_eq!(clip.channels().get(), 1);
        assert!(clip.frames() > 0);
    }

    #[test]
    fn decode_resamples_to_target() {
        let target = nz(48_000);
        let Ok(clip) = decode_clip(WAV_MONO_22050.to_vec(), target) else {
            unreachable!("wav fixture decodes + resamples")
        };
        assert_eq!(clip.sample_rate(), target, "clip resampled to device rate");
    }

    #[test]
    fn decode_rejects_garbage() {
        let err = decode_clip(vec![0u8; 32], nz(48_000));
        assert!(matches!(err, Err(AudioError::Decode(_))));
    }

    #[test]
    fn cache_stores_and_reuses() {
        let Ok(clip) = decode_clip(WAV_MONO_22050.to_vec(), nz(48_000)) else {
            unreachable!("wav fixture decodes")
        };
        let mut cache: ClipCache<u32> = ClipCache::new();
        assert!(cache.is_empty());
        cache.insert(7, clip);
        assert!(cache.contains(&7));
        assert_eq!(cache.len(), 1);
        assert!(cache.get(&7).is_some());
        cache.remove(&7);
        assert!(cache.is_empty());
    }
}
