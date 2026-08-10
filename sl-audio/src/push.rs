//! The pushed-PCM path: a realtime-safe channel plus a custom firewheel node
//! that plays whatever samples are pushed into it.
//!
//! Firewheel deleted its built-in stream nodes, so this node is ours — but the
//! hard part is provided by [`fixed_resample`]: a realtime-safe SPSC channel
//! with automatic sample-rate conversion and under/overflow correction, the
//! same primitive firewheel itself uses for its microphone path. Every external
//! audio source (GStreamer's `appsink`, CEF's `OnAudioStreamPacket`, decoded
//! Opus) runs on its own clock, different from the sound card's; pushing through
//! this channel is where that clock drift dies.
//!
//! The producer end ([`PushProducer`]) is handed to the source (on whatever
//! thread it produces on); the consumer end lives inside the RT audio node and
//! is read once per processing block.
#![expect(
    clippy::module_name_repetitions,
    reason = "Push* are the natural public names for the pushed-PCM path, re-exported at the crate root"
)]

use std::num::{NonZeroU32, NonZeroUsize};
use std::sync::{Arc, Mutex};

use fixed_resample::{
    PushStatus, ReadStatus, ResamplingChannelConfig, ResamplingCons, ResamplingProd,
    resampling_channel,
};

/// Interleave `channels` planar slices into `out`, resizing `out` to
/// `frames * channels`. Channels shorter than the first are zero-padded; extra
/// channels beyond `channels` are ignored. Returns the number of frames.
fn interleave_into(input: &[&[f32]], channels: usize, out: &mut Vec<f32>) -> usize {
    let frames = input.first().map_or(0, |c| c.len());
    out.clear();
    out.resize(frames.saturating_mul(channels), 0.0);
    for (ch, src) in input.iter().enumerate().take(channels) {
        for (frame, sample) in src.iter().enumerate() {
            if let Some(slot) = out.get_mut(frame.saturating_mul(channels).saturating_add(ch)) {
                *slot = *sample;
            }
        }
    }
    frames
}

use firewheel::channel_config::{ChannelConfig, ChannelCount};
use firewheel::node::{
    AudioNode, AudioNodeInfo, AudioNodeProcessor, ConstructProcessorContext, EmptyConfig,
    NodeError, ProcBuffers, ProcExtra, ProcInfo, ProcessStatus,
};

/// Configuration for a pushed-PCM stream.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PushStreamConfig {
    /// The number of channels the source produces (1 = mono, 2 = stereo).
    pub channels: NonZeroUsize,
    /// The sample rate of the source stream, in Hz. Need not match the device;
    /// the channel resamples.
    pub in_sample_rate: NonZeroU32,
    /// The buffering latency between the source and the device, in seconds. Too
    /// small risks underflow glitches; the firewheel default of 150 ms is a
    /// sensible starting point for network / decoder streams.
    pub latency_seconds: f64,
    /// The channel capacity in seconds (should be at least twice the latency).
    pub capacity_seconds: f64,
}

impl PushStreamConfig {
    /// A stereo stream at `in_sample_rate` with default latency (150 ms) and
    /// capacity (400 ms).
    #[must_use]
    pub fn stereo(in_sample_rate: NonZeroU32) -> Self {
        Self {
            channels: NonZeroUsize::new(2).unwrap_or(NonZeroUsize::MIN),
            in_sample_rate,
            latency_seconds: 0.15,
            capacity_seconds: 0.4,
        }
    }
}

/// The outcome of pushing a block of samples into a [`PushProducer`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PushOutcome {
    /// All samples were accepted.
    Ok,
    /// The channel overflowed (the device is draining slower than the source
    /// produces) and some samples were discarded to catch up.
    Overflowed,
    /// The channel underflowed and was corrected with silence (the source is
    /// slower than the device).
    Underflowed,
    /// The output stream is not ready yet; the samples were dropped.
    NotReady,
}

/// A slot holding the current producer end of a stream, shared between the
/// [`PushProducer`] the source holds and the mixer (so the mixer can swap in a
/// fresh producer when it rebuilds the graph on a device hot-plug).
pub(crate) type ProducerSlot = Arc<Mutex<Option<ResamplingProd<f32>>>>;

/// The producer end of a pushed-PCM stream: the source pushes decoded PCM here,
/// on whatever thread it runs on.
///
/// The producer is held behind a shared slot so the mixer can replace the
/// underlying channel when the output device changes (hot-plug) without the
/// source having to re-open its stream. Pushes during the brief rebuild window
/// return [`PushOutcome::NotReady`]. This is `Send` so it can live on
/// GStreamer's or CEF's audio thread.
pub struct PushProducer {
    /// The shared producer slot (swapped on device rebuild).
    slot: ProducerSlot,
    /// The number of channels, cached for interleave validation.
    channels: usize,
    /// Reusable scratch buffer for interleaving planar input, so
    /// [`push_deinterleaved`](PushProducer::push_deinterleaved) does not
    /// allocate on every packet.
    interleave_scratch: Vec<f32>,
}

impl std::fmt::Debug for PushProducer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PushProducer")
            .field("channels", &self.channels)
            .finish_non_exhaustive()
    }
}

impl PushProducer {
    /// The number of channels this stream carries.
    #[must_use]
    pub const fn channels(&self) -> usize {
        self.channels
    }

    /// A clone of the shared producer slot, for the mixer to swap on rebuild.
    pub(crate) fn slot_handle(&self) -> ProducerSlot {
        Arc::clone(&self.slot)
    }

    /// Push interleaved PCM (frame 0 ch 0, frame 0 ch 1, frame 1 ch 0, …). This
    /// is the shape GStreamer's `appsink` delivers.
    pub fn push_interleaved(&mut self, input: &[f32]) -> PushOutcome {
        match self.slot.lock() {
            Ok(mut guard) => match guard.as_mut() {
                Some(prod) => map_push_status(prod.push_interleaved(input)),
                None => PushOutcome::NotReady,
            },
            Err(_) => PushOutcome::NotReady,
        }
    }

    /// Push de-interleaved (planar) PCM, one slice per channel — the shape CEF's
    /// `OnAudioStreamPacket` delivers. Extra channels beyond [`channels`] are
    /// ignored.
    ///
    /// [`channels`]: PushProducer::channels
    pub fn push_deinterleaved(&mut self, input: &[&[f32]]) -> PushOutcome {
        let mut scratch = std::mem::take(&mut self.interleave_scratch);
        let _frames = interleave_into(input, self.channels, &mut scratch);
        let outcome = match self.slot.lock() {
            Ok(mut guard) => match guard.as_mut() {
                Some(prod) => map_push_status(prod.push_interleaved(&scratch)),
                None => PushOutcome::NotReady,
            },
            Err(_) => PushOutcome::NotReady,
        };
        self.interleave_scratch = scratch;
        outcome
    }

    /// The number of frames currently buffered and available to the device.
    pub fn available_frames(&mut self) -> usize {
        match self.slot.lock() {
            Ok(mut guard) => guard.as_mut().map_or(0, ResamplingProd::available_frames),
            Err(_) => 0,
        }
    }
}

/// Map a [`fixed_resample`] push status onto our simplified [`PushOutcome`].
const fn map_push_status(status: PushStatus) -> PushOutcome {
    match status {
        PushStatus::Ok => PushOutcome::Ok,
        PushStatus::OverflowOccurred { .. } => PushOutcome::Overflowed,
        PushStatus::UnderflowCorrected { .. } => PushOutcome::Underflowed,
        PushStatus::OutputNotReady => PushOutcome::NotReady,
    }
}

/// Build a resampling channel for `config` feeding a device at `out_sample_rate`,
/// install its producer into `slot`, and return the [`PushNode`] holding the
/// consumer. Used both to open a new stream and to re-establish one after a
/// device rebuild (the same `slot` is reused so the source's [`PushProducer`]
/// keeps working).
pub(crate) fn install_stream(
    slot: &ProducerSlot,
    config: PushStreamConfig,
    out_sample_rate: NonZeroU32,
) -> PushNode {
    let channels = config.channels.get();
    let channel_config = ResamplingChannelConfig {
        latency_seconds: config.latency_seconds,
        capacity_seconds: config.capacity_seconds,
        ..Default::default()
    };
    let (prod, cons) = resampling_channel::<f32>(
        channels,
        config.in_sample_rate.get(),
        out_sample_rate.get(),
        false,
        channel_config,
    );
    if let Ok(mut guard) = slot.lock() {
        *guard = Some(prod);
    }
    PushNode {
        cons: Arc::new(Mutex::new(Some(cons))),
        channels,
    }
}

/// Create a pushed-PCM stream feeding a device running at `out_sample_rate`.
///
/// Returns the producer (for the source) and the audio node (for the mixer to
/// add to its graph).
#[must_use]
pub fn push_stream(
    config: PushStreamConfig,
    out_sample_rate: NonZeroU32,
) -> (PushProducer, PushNode) {
    let slot: ProducerSlot = Arc::new(Mutex::new(None));
    let node = install_stream(&slot, config, out_sample_rate);
    (
        PushProducer {
            slot,
            channels: config.channels.get(),
            interleave_scratch: Vec::new(),
        },
        node,
    )
}

/// The custom firewheel node that reads a [`PushProducer`]'s samples and writes
/// them to its outputs.
///
/// The consumer is moved into the RT processor when the graph is activated. The
/// node produces silence if it is ever re-activated after the consumer was taken
/// (the mixer rebuilds its graph rather than re-activating, so that is a
/// defensive fallback, not a normal path).
pub struct PushNode {
    /// The consumer end, taken into the processor on activation.
    cons: Arc<Mutex<Option<ResamplingCons<f32>>>>,
    /// The number of output channels.
    channels: usize,
}

impl std::fmt::Debug for PushNode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PushNode")
            .field("channels", &self.channels)
            .finish_non_exhaustive()
    }
}

impl AudioNode for PushNode {
    type Configuration = EmptyConfig;

    fn info(&self, _config: &Self::Configuration) -> Result<AudioNodeInfo, NodeError> {
        let num_outputs = match self.channels {
            1 => ChannelCount::MONO,
            _ => ChannelCount::STEREO,
        };
        Ok(AudioNodeInfo::new()
            .debug_name("sl_audio_push")
            .channel_config(ChannelConfig {
                num_inputs: ChannelCount::ZERO,
                num_outputs,
            }))
    }

    fn construct_processor(
        &self,
        _config: &Self::Configuration,
        cx: ConstructProcessorContext,
    ) -> Result<impl AudioNodeProcessor, NodeError> {
        let cons = self.cons.lock().ok().and_then(|mut guard| guard.take());
        let max_frames = usize::try_from(cx.stream_info.max_block_frames.get()).unwrap_or(1024);
        let scratch_len = self.channels.saturating_mul(max_frames);
        Ok(PushProcessor {
            cons,
            channels: self.channels.max(1),
            scratch: vec![0.0; scratch_len],
        })
    }
}

/// The RT processor for [`PushNode`]: reads the channel each block and
/// de-interleaves it into the output buffers.
struct PushProcessor {
    /// The consumer end, or `None` if the node was re-activated (produces
    /// silence).
    cons: Option<ResamplingCons<f32>>,
    /// The number of output channels (at least 1).
    channels: usize,
    /// Pre-allocated interleaved scratch buffer (`channels * max_block_frames`),
    /// so no allocation happens on the RT thread.
    scratch: Vec<f32>,
}

impl AudioNodeProcessor for PushProcessor {
    fn process(
        &mut self,
        info: &ProcInfo,
        buffers: ProcBuffers,
        _extra: &mut ProcExtra,
    ) -> ProcessStatus {
        let Some(cons) = self.cons.as_mut() else {
            return ProcessStatus::ClearAllOutputs;
        };
        let needed = info.frames.saturating_mul(self.channels);
        let Some(scratch) = self.scratch.get_mut(..needed) else {
            return ProcessStatus::ClearAllOutputs;
        };
        // `read_interleaved` fills the whole slice, zero-padding on underflow.
        let _status: ReadStatus = cons.read_interleaved(scratch, false);

        for (ch, out) in buffers.outputs.iter_mut().enumerate() {
            for (out_sample, frame) in out.iter_mut().zip(scratch.chunks_exact(self.channels)) {
                *out_sample = frame.get(ch).copied().unwrap_or(0.0);
            }
        }
        ProcessStatus::OutputsModified
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    /// Build a `NonZeroU32` for tests without `unwrap`/`expect`.
    fn nz(v: u32) -> NonZeroU32 {
        NonZeroU32::new(v).unwrap_or(NonZeroU32::MIN)
    }

    #[test]
    fn stereo_config_defaults() {
        let sr = nz(44_100);
        let cfg = PushStreamConfig::stereo(sr);
        assert_eq!(cfg.channels.get(), 2);
        assert_eq!(cfg.in_sample_rate, sr);
        assert!((cfg.latency_seconds - 0.15).abs() < 1e-9);
    }

    #[test]
    fn interleave_planar_into_scratch() {
        let left = [1.0f32, 3.0, 5.0];
        let right = [2.0f32, 4.0, 6.0];
        let planar: [&[f32]; 2] = [&left, &right];
        let mut out = Vec::new();
        let frames = interleave_into(&planar, 2, &mut out);
        assert_eq!(frames, 3);
        assert_eq!(out, vec![1.0, 2.0, 3.0, 4.0, 5.0, 6.0]);
    }

    #[test]
    fn round_trips_once_consumer_is_ready() {
        let sr = nz(48_000);
        let (mut prod, node) = push_stream(PushStreamConfig::stereo(sr), sr);
        assert_eq!(prod.channels(), 2);
        assert_eq!(node.channels, 2);

        // Before the RT node runs the output is not ready, so a push is dropped.
        let block = vec![0.25f32; 200];
        assert_eq!(prod.push_interleaved(&block), PushOutcome::NotReady);

        // Drive the consumer once (as the RT processor would), which marks the
        // output ready; now the producer's samples are accepted.
        let taken = node.cons.lock().ok().and_then(|mut g| g.take());
        let Some(mut cons) = taken else {
            unreachable!("consumer present in a freshly built node")
        };
        let mut out = vec![0.0f32; 64];
        let _status: ReadStatus = cons.read_interleaved(&mut out, false);

        let outcome = prod.push_interleaved(&block);
        assert!(
            matches!(
                outcome,
                PushOutcome::Ok | PushOutcome::Overflowed | PushOutcome::Underflowed
            ),
            "expected the push to be accepted, got {outcome:?}"
        );
    }
}
