//! A self-contained audio smoke test for the [`sl_audio`] mixer.
//!
//! Opens the default output device and, over a few seconds:
//!
//! - plays a short Ogg Vorbis clip (the real SL sound format) on the SFX bus;
//! - plays the same clip **spatially**, sweeping it from the listener's left to
//!   the right so panning is audible;
//! - feeds a continuous 220 Hz sine tone through a **pushed-PCM** stream on the
//!   music bus (the GStreamer / CEF path), then demonstrates that muting the
//!   music bus silences the tone while it keeps running (mute retains level),
//!   and unmuting restores it.
//!
//! Run it (release is smoother): `cargo run -p sl-audio --example play_test
//! --release`. You should hear the clip, a panning clip, and a tone that cuts
//! out and comes back. There is nothing to see; this is an ears-on check of the
//! device / decode / bus / spatial / push paths the unit tests cannot cover.

use std::num::NonZeroU32;
use std::time::{Duration, Instant};

use sl_audio::AudioMixer as _;
use sl_audio::{
    Bus, ClipParams, DeviceSelection, Importance, Listener, Mixer, MixerConfig, PushStreamConfig,
    SpatialParams, decode_clip,
};

/// A short Ogg Vorbis clip (mono, ~0.2 s) bundled for the smoke test.
const CLIP_OGG: &[u8] = include_bytes!("../tests/fixtures/tone_mono_44100.ogg");

/// Frames of sine to push per iteration (well under the channel capacity).
const TONE_FRAMES: usize = 1024;

/// Generate `frames` stereo frames of a sine tone into `out` (interleaved),
/// advancing `phase` (in cycles). Returns the updated phase.
fn fill_tone(out: &mut Vec<f32>, frames: usize, phase_inc: f32, mut phase: f32) -> f32 {
    out.clear();
    for _ in 0..frames {
        let s = (phase * std::f32::consts::TAU).sin() * 0.2;
        out.push(s);
        out.push(s);
        phase = (phase + phase_inc).fract();
    }
    phase
}

fn main() -> Result<(), sl_audio::AudioError> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let mut mixer = Mixer::new(&MixerConfig::default())?;
    mixer.start(&DeviceSelection::Default)?;
    let sample_rate = mixer.sample_rate().unwrap_or(NonZeroU32::MIN);
    tracing::info!("device started at {} Hz", sample_rate.get());
    tracing::info!("output devices: {:?}", Mixer::output_devices());

    // Listener at the origin, facing -Z (identity camera pose).
    mixer.set_listener(Listener::default());

    // Decode the clip once (to the device sample rate).
    let clip = decode_clip(CLIP_OGG.to_vec(), sample_rate)?;
    tracing::info!(
        "clip: {} ch, {:.3}s",
        clip.channels().get(),
        clip.duration_seconds()
    );

    // A continuous sine tone on the music bus via the pushed-PCM path.
    let mut tone = mixer
        .open_stream(Bus::Music, None, PushStreamConfig::stereo(sample_rate))
        .ok_or_else(|| sl_audio::AudioError::Stream("could not open tone stream".to_owned()))?;
    let phase_inc = 220.0 / u32_to_f32(sample_rate.get());
    let mut phase = 0.0_f32;
    let mut scratch: Vec<f32> = Vec::new();

    // Play a 2-D clip on the SFX bus right away.
    let _clip_voice = mixer.play_clip(
        &clip,
        ClipParams {
            bus: Bus::Sfx,
            gain: 0.8,
            importance: Importance::OneShot,
            looped: false,
        },
    );

    let start = Instant::now();
    let mut since_clip = 0.0_f32;
    let mut swept = false;
    // 0 = not yet muted, 1 = muted, 2 = unmuted again.
    let mut music_phase = 0_u8;
    let total = Duration::from_secs(6);

    while start.elapsed() < total {
        let t = start.elapsed().as_secs_f32();

        // Keep the tone buffer topped up.
        phase = fill_tone(&mut scratch, TONE_FRAMES, phase_inc, phase);
        let _pushed = tone.producer.push_interleaved(&scratch);

        // Every ~1.2s, sweep a spatial clip from left to right.
        since_clip += 0.016;
        if since_clip >= 1.2 {
            since_clip = 0.0;
            let x = if swept { 8.0 } else { -8.0 };
            swept = !swept;
            let _spatial_voice = mixer.play_spatial(
                &clip,
                SpatialParams {
                    bus: Bus::Sfx,
                    gain: 1.0,
                    importance: Importance::OneShot,
                    looped: false,
                    position: [x, 0.0, -2.0],
                },
            );
            tracing::info!("spatial clip at x={x}");
        }

        // Around the 3s mark, mute the music bus (tone should go silent but keep
        // running); around 4.5s, unmute (tone returns at its prior level).
        if t >= 3.0 && music_phase == 0 {
            music_phase = 1;
            let mut level = mixer.bus_level(Bus::Music);
            level.set_muted(true);
            mixer.set_bus_level(Bus::Music, level);
            tracing::info!("music bus muted (tone still running, silently)");
        }
        if t >= 4.5 && music_phase == 1 {
            music_phase = 2;
            let mut level = mixer.bus_level(Bus::Music);
            level.set_muted(false);
            mixer.set_bus_level(Bus::Music, level);
            tracing::info!("music bus unmuted (tone restored)");
        }

        mixer.update();
        std::thread::sleep(Duration::from_millis(16));
    }

    tracing::info!("done");
    Ok(())
}

/// Convert an audio sample rate to `f32`. Sample rates are at most a few hundred
/// thousand, so the conversion is exact.
#[expect(
    clippy::as_conversions,
    clippy::cast_precision_loss,
    reason = "audio sample rate (<= a few hundred thousand) converts to f32 exactly"
)]
const fn u32_to_f32(v: u32) -> f32 {
    v as f32
}
