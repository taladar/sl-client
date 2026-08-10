//! The shared-mixer audio hand-off: a `playbin3` `audio-sink` bin that pushes
//! decoded PCM into the viewer's [`sl_media::AudioSink`] instead of a sound
//! card.
//!
//! Both GStreamer consumers — the parcel radio stream ([`crate::stream`]) and
//! media-on-a-prim video ([`crate::surface`]) — replace `playbin3`'s default
//! `autoaudiosink` with the bin this module builds:
//! `audioconvert ! capsfilter(F32LE, stereo, interleaved) ! appsink`. The
//! `appsink` copies each decoded block, tells the sink its sample rate (only
//! when it changes), and pushes stereo f32 PCM.
//!
//! **A/V sync and clock ownership.** The `appsink` runs `sync=true`, so
//! GStreamer paces audio delivery to the *pipeline* clock exactly as its video
//! sink is paced — audio and video stay in step by construction, as they did
//! when both went to the pipeline's own sinks. The only new clock is the sound
//! card's, and the mixer's resampling channel (which every push crosses) is
//! precisely the pipeline-clock↔device-clock drift corrector. Pacing to the
//! pipeline clock also keeps a file decoder that can run far ahead of realtime
//! from flooding the channel — the reason the audio sink is *not* run
//! `sync=false`.
//!
//! The sink is attached *after* the pipeline is built (the backend creates a
//! surface, then the viewer hands it a sink), so the `appsink` reads it from a
//! shared slot each block. Until a sink is attached — and in the crate's own
//! tests, and if the viewer has no mixer — the block is simply discarded (the
//! sample is still pulled, so the pipeline never stalls).

use std::sync::{Arc, Mutex};

use gstreamer::prelude::*;
use sl_media::AudioSink;
use tracing::{debug, warn};

/// The channel count the audio sink normalises every source to. Stereo matches
/// the mixer's stream inputs (music: 2-D stereo; media-on-a-prim: stereo into
/// the spatialiser), and lets GStreamer's `audioconvert` do a proper down-/
/// up-mix matrix rather than the naive channel picking the pushed-PCM path would
/// otherwise fall back to.
const SINK_CHANNELS: i32 = 2;

/// The GStreamer raw-audio format name for **native-endian** 32-bit float, so the
/// `appsink` buffer bytes reinterpret to `f32` in host byte order (matching
/// [`f32::from_ne_bytes`]) with no endianness swap.
#[cfg(target_endian = "little")]
const NATIVE_F32: &str = "F32LE";
/// See [`NATIVE_F32`].
#[cfg(target_endian = "big")]
const NATIVE_F32: &str = "F32BE";

/// State shared between a pipeline's audio `appsink` (a GStreamer streaming
/// thread) and the owning surface / stream handle (the caller's thread): the
/// mixer sink to push into — attached after the pipeline is built — and the last
/// source sample rate seen, so the sink is re-[`configure`](AudioSink::configure)d
/// only when the rate actually changes.
#[derive(Default)]
pub(crate) struct AudioSinkState {
    /// The mixer input to push into, once the viewer has attached one.
    sink: Option<Arc<dyn AudioSink>>,
    /// The last source sample rate handed to [`AudioSink::configure`].
    last_rate: Option<i32>,
}

impl std::fmt::Debug for AudioSinkState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AudioSinkState")
            .field("attached", &self.sink.is_some())
            .field("last_rate", &self.last_rate)
            .finish()
    }
}

/// A handle to a pipeline's audio hand-off, shared with its `appsink` callback.
pub(crate) type SharedAudioSink = Arc<Mutex<AudioSinkState>>;

/// A fresh, unattached audio hand-off.
pub(crate) fn shared() -> SharedAudioSink {
    Arc::new(Mutex::new(AudioSinkState::default()))
}

/// Attach `sink` to `state` (the trait's `set_audio_sink`), forcing the next
/// block to re-announce the format so the mixer input is (re)opened.
pub(crate) fn attach(state: &SharedAudioSink, sink: Arc<dyn AudioSink>) {
    if let Ok(mut guard) = state.lock() {
        guard.last_rate = None;
        guard.sink = Some(sink);
    }
}

/// Whether a sink is currently attached (the caller uses this to decide between
/// the mixer path and the interim `autoaudiosink` fallback).
pub(crate) fn is_attached(state: &SharedAudioSink) -> bool {
    state.lock().is_ok_and(|guard| guard.sink.is_some())
}

/// Tell the attached sink (if any) the source stopped — call on stop / close /
/// end-of-stream. Leaves the sink attached so a later restart re-opens it.
pub(crate) fn mark_stopped(state: &SharedAudioSink) {
    if let Ok(mut guard) = state.lock() {
        guard.last_rate = None;
        if let Some(sink) = guard.sink.clone() {
            sink.stopped();
        }
    }
}

/// Route a mute change to the attached sink (if any). Returns `true` when a sink
/// handled it, so the caller can fall back to the pipeline's own mute otherwise.
pub(crate) fn set_muted(state: &SharedAudioSink, muted: bool) -> bool {
    match state.lock() {
        Ok(guard) => match guard.sink.clone() {
            Some(sink) => {
                sink.set_muted(muted);
                true
            }
            None => false,
        },
        Err(_poisoned) => false,
    }
}

/// Build the audio-sink bin (`audioconvert ! capsfilter(F32LE, stereo,
/// interleaved) ! appsink`) that pushes stereo f32 PCM into `state`'s sink. Set
/// it as `playbin3`'s `audio-sink` so the mixer, not the sound card, owns
/// playback. Returns `None` (with a warning) if the base elements are missing.
pub(crate) fn build_bin(state: &SharedAudioSink) -> Option<gstreamer::Element> {
    let convert = match gstreamer::ElementFactory::make("audioconvert").build() {
        Ok(element) => element,
        Err(error) => {
            warn!("audio sink: no audioconvert element ({error}); audio disabled");
            return None;
        }
    };
    // Native-endian interleaved-float stereo: the format the pushed-PCM path
    // expects, in host byte order so the samples reinterpret without an
    // endianness swap. Rate is left open so no resampling happens here — the
    // mixer's resampling channel owns the device-rate conversion.
    let caps = gstreamer::Caps::builder("audio/x-raw")
        .field("format", NATIVE_F32)
        .field("layout", "interleaved")
        .field("channels", SINK_CHANNELS)
        .build();
    let appsink = gstreamer_app::AppSink::builder()
        .caps(&caps)
        // Pace to the pipeline clock (see the module docs on A/V sync).
        .sync(true)
        .max_buffers(8)
        .drop(false)
        .build();
    let state_cb = Arc::clone(state);
    appsink.set_callbacks(
        gstreamer_app::AppSinkCallbacks::builder()
            .new_sample(move |sink| {
                let sample = sink
                    .pull_sample()
                    .map_err(|_gone| gstreamer::FlowError::Eos)?;
                push_sample(&sample, &state_cb);
                Ok(gstreamer::FlowSuccess::Ok)
            })
            .build(),
    );
    let bin = gstreamer::Bin::builder().name("sl-audio-sink").build();
    if let Err(error) = bin.add_many([&convert, appsink.upcast_ref()]) {
        warn!("audio sink: assembling the bin failed ({error}); audio disabled");
        return None;
    }
    if let Err(error) = convert.link(&appsink) {
        warn!("audio sink: linking the bin failed ({error}); audio disabled");
        return None;
    }
    let sink_pad = convert.static_pad("sink")?;
    let ghost = match gstreamer::GhostPad::with_target(&sink_pad) {
        Ok(pad) => pad,
        Err(error) => {
            warn!("audio sink: ghost pad failed ({error}); audio disabled");
            return None;
        }
    };
    if let Err(error) = bin.add_pad(&ghost) {
        warn!("audio sink: adding the ghost pad failed ({error}); audio disabled");
        return None;
    }
    Some(bin.upcast())
}

/// Copy one `appsink` block and push it into the attached sink: read the source
/// sample rate from the caps (re-`configure`ing only on a change), reinterpret
/// the F32LE bytes as f32, and push the stereo interleaved samples. Runs on a
/// GStreamer streaming thread.
fn push_sample(sample: &gstreamer::Sample, state: &SharedAudioSink) {
    let Some(rate) = sample
        .caps()
        .and_then(|caps| caps.structure(0).map(ToOwned::to_owned))
        .and_then(|structure| structure.get::<i32>("rate").ok())
    else {
        return;
    };
    let Some(buffer) = sample.buffer() else {
        return;
    };
    let Ok(map) = buffer.map_readable() else {
        return;
    };
    // Native-endian float bytes → f32 (the caps pinned the buffer to host byte
    // order, so `from_ne_bytes` is the correct, endianness-agnostic read).
    let samples: Vec<f32> = map
        .as_slice()
        .chunks_exact(4)
        .map(|bytes| f32::from_ne_bytes(<[u8; 4]>::try_from(bytes).unwrap_or([0; 4])))
        .collect();

    let mut guard = match state.lock() {
        Ok(guard) => guard,
        Err(_poisoned) => return,
    };
    let Some(sink) = guard.sink.clone() else {
        return;
    };
    if guard.last_rate != Some(rate) {
        guard.last_rate = Some(rate);
        if let Ok(rate) = u32::try_from(rate) {
            debug!("audio sink: source now {rate} Hz, {SINK_CHANNELS}ch");
            sink.configure(rate, 2);
        }
    }
    drop(guard);
    sink.push_interleaved(&samples, 2);
}
