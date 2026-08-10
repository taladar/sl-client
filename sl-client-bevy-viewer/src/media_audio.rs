//! The viewer's bridge from a media engine's PCM to the shared [`sl_audio`]
//! mixer (`viewer-gst-audio-mixer-handoff`, `viewer-cef-audio-mixer-handoff`).
//!
//! GStreamer (parcel radio, prim video) and CEF (page audio) each produce PCM on
//! their own thread and their own clock. This module is the piece that joins
//! those to the one mixer:
//!
//! - [`MixerSink`] implements [`sl_media::AudioSink`] — the object each engine
//!   pushes samples into, on whatever thread it produces on. It normalises the
//!   PCM to stereo and hands it to a mixer input; it holds no mixer or Bevy
//!   state, only a realtime-safe channel, so it is safe to touch from CEF's
//!   audio thread and GStreamer's streaming threads.
//! - [`MixerStream`] is the viewer-thread handle: it owns the mixer's
//!   [`StreamId`], (re)opens the mixer input when the source announces a format
//!   (a fresh stream, or a mid-page rate change), closes it when the source
//!   stops, and — for a prim surface — keeps its spatial position on the prim.
//!
//! The two are linked by a shared [`SinkControl`]: the engine thread writes a
//! *pending format* and pushes PCM into a *producer* slot; the viewer thread
//! reads the pending format, opens the mixer input, and drops the resulting
//! producer into the slot. Because the resampling channel's input rate is fixed
//! at open time, a format change is a producer swap the viewer performs — the
//! same shape the mixer already uses for a device hot-plug.

use std::num::NonZeroU32;
use std::sync::{Arc, Mutex};

use bevy::math::Vec3;
use sl_audio::{Bus, Mixer, PushProducer, PushStreamConfig, StreamId};
use sl_media::AudioSink;

/// The PCM format a source announced. Channels are always normalised to stereo
/// downstream, so only the sample rate reaches the mixer input.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct PcmFormat {
    /// The source sample rate in Hz.
    sample_rate: u32,
}

/// State shared between a media source's audio thread (through [`MixerSink`]) and
/// the viewer thread (through [`MixerStream`]).
#[derive(Default)]
struct SinkControl {
    /// The mixer input the source pushes into; `None` before the viewer opens it
    /// and during the brief reopen window on a format change (pushes drop then).
    producer: Option<PushProducer>,
    /// A format the source announced that the viewer must (re)open the mixer
    /// input for. Consumed by [`MixerStream::service`].
    pending_format: Option<PcmFormat>,
    /// The source stopped producing; the viewer should close the mixer input.
    stopped: bool,
    /// Whether this source is muted at the mixer input (fed as silence).
    muted: bool,
    /// Reusable stereo-interleave scratch, so a push does not allocate.
    scratch: Vec<f32>,
}

/// The [`AudioSink`] handed to a media engine: normalises PCM to stereo and
/// pushes it into the mixer input the viewer opened for it.
struct MixerSink {
    /// The state shared with the [`MixerStream`] viewer-thread handle.
    control: Arc<Mutex<SinkControl>>,
}

impl AudioSink for MixerSink {
    fn configure(&self, sample_rate: u32, _channels: u16) {
        if let Ok(mut control) = self.control.lock() {
            control.pending_format = Some(PcmFormat { sample_rate });
            // Stop pushing at the old rate until the viewer reopens the input.
            control.producer = None;
            control.stopped = false;
        }
    }

    fn push_interleaved(&self, samples: &[f32], channels: u16) {
        if let Ok(mut control) = self.control.lock() {
            let SinkControl {
                producer,
                muted,
                scratch,
                ..
            } = &mut *control;
            let Some(producer) = producer.as_mut() else {
                return;
            };
            interleaved_to_stereo(samples, channels, scratch);
            if *muted {
                scratch.fill(0.0);
            }
            let _outcome = producer.push_interleaved(scratch);
        }
    }

    fn push_planar(&self, planes: &[&[f32]]) {
        if let Ok(mut control) = self.control.lock() {
            let SinkControl {
                producer,
                muted,
                scratch,
                ..
            } = &mut *control;
            let Some(producer) = producer.as_mut() else {
                return;
            };
            planar_to_stereo(planes, scratch);
            if *muted {
                scratch.fill(0.0);
            }
            let _outcome = producer.push_interleaved(scratch);
        }
    }

    fn set_muted(&self, muted: bool) {
        if let Ok(mut control) = self.control.lock() {
            control.muted = muted;
        }
    }

    fn stopped(&self) {
        if let Ok(mut control) = self.control.lock() {
            control.stopped = true;
            control.producer = None;
        }
    }
}

/// The viewer-thread handle to one media source's mixer input: opens / closes it
/// following the source's format announcements and keeps a spatial source on its
/// prim. Create it with [`MixerStream::new`], hand the returned [`AudioSink`] to
/// the media surface / stream player, and call [`service`](Self::service) each
/// frame with the mixer.
pub(crate) struct MixerStream {
    /// State shared with the source's [`MixerSink`].
    control: Arc<Mutex<SinkControl>>,
    /// The bus the input plays on (music for the parcel stream, media for
    /// video / page audio).
    bus: Bus,
    /// Whether the input is spatialised (a prim surface) or 2-D (parcel radio,
    /// UI panels).
    spatial: bool,
    /// The prim world position for a spatial input (ignored when `spatial` is
    /// false). Kept up to date by [`set_position`](Self::set_position).
    position: [f32; 3],
    /// The mixer input's id while open.
    stream_id: Option<StreamId>,
    /// Set once [`close`](Self::close) has run, so servicing stops.
    closed: bool,
}

impl MixerStream {
    /// Create a mixer input on `bus` (spatial for a prim, 2-D otherwise) and the
    /// [`AudioSink`] to hand the source. The input is not opened until the source
    /// announces a format (see [`service`](Self::service)).
    pub(crate) fn new(bus: Bus, spatial: bool) -> (Self, Arc<dyn AudioSink>) {
        let control = Arc::new(Mutex::new(SinkControl::default()));
        let sink: Arc<dyn AudioSink> = Arc::new(MixerSink {
            control: Arc::clone(&control),
        });
        (
            Self {
                control,
                bus,
                spatial,
                position: [0.0, 0.0, 0.0],
                stream_id: None,
                closed: false,
            },
            sink,
        )
    }

    /// Update a spatial input's world position (a no-op for a 2-D input). Takes
    /// effect on the next [`service`](Self::service).
    pub(crate) const fn set_position(&mut self, position: Vec3) {
        if self.spatial {
            self.position = position.to_array();
        }
    }

    /// Reconcile the mixer input with the source: (re)open it for a newly
    /// announced format, close it when the source stopped, and keep a spatial
    /// input on its prim. Call once per frame with the mixer.
    pub(crate) fn service(&mut self, mixer: &mut Mixer) {
        if self.closed {
            return;
        }
        let (stopped, pending) = {
            let Ok(mut control) = self.control.lock() else {
                return;
            };
            (control.stopped, control.pending_format.take())
        };

        if stopped && pending.is_none() {
            if let Some(id) = self.stream_id.take() {
                mixer.close_stream(id);
            }
            return;
        }

        if let Some(format) = pending {
            if let Some(id) = self.stream_id.take() {
                mixer.close_stream(id);
            }
            let rate = NonZeroU32::new(format.sample_rate).unwrap_or(NonZeroU32::MIN);
            let position = self.spatial.then_some(self.position);
            if let Some(handle) =
                mixer.open_stream(self.bus, position, PushStreamConfig::stereo(rate))
            {
                self.stream_id = Some(handle.id);
                if let Ok(mut control) = self.control.lock() {
                    control.producer = Some(handle.producer);
                }
            }
        }

        if self.spatial
            && let Some(id) = self.stream_id
        {
            mixer.set_stream_position(id, self.position);
        }
    }

    /// Close the mixer input for good (the surface / stream is going away).
    pub(crate) fn close(&mut self, mixer: &mut Mixer) {
        if let Some(id) = self.stream_id.take() {
            mixer.close_stream(id);
        }
        self.closed = true;
    }
}

/// Fill `out` with stereo-interleaved f32 from `samples` laid out as `channels`
/// interleaved channels: mono is duplicated to both ears, stereo passes through,
/// and more than two channels keep the front two. `out` is cleared first.
fn interleaved_to_stereo(samples: &[f32], channels: u16, out: &mut Vec<f32>) {
    out.clear();
    match channels.max(1) {
        1 => {
            out.reserve(samples.len().saturating_mul(2));
            for &sample in samples {
                out.push(sample);
                out.push(sample);
            }
        }
        2 => out.extend_from_slice(samples),
        more => {
            let channels = usize::from(more);
            out.reserve(samples.len());
            for frame in samples.chunks_exact(channels) {
                out.push(frame.first().copied().unwrap_or(0.0));
                out.push(frame.get(1).copied().unwrap_or(0.0));
            }
        }
    }
}

/// Fill `out` with stereo-interleaved f32 from planar `planes` (one slice per
/// channel): a single plane is duplicated to both ears, otherwise the first two
/// planes become left and right. `out` is cleared first.
fn planar_to_stereo(planes: &[&[f32]], out: &mut Vec<f32>) {
    out.clear();
    let Some(left) = planes.first() else {
        return;
    };
    let right = planes.get(1).copied().unwrap_or(*left);
    out.reserve(left.len().saturating_mul(2));
    for (l, r) in left.iter().zip(right.iter()) {
        out.push(*l);
        out.push(*r);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn stereo_passthrough() {
        let mut out = Vec::new();
        interleaved_to_stereo(&[1.0, 2.0, 3.0, 4.0], 2, &mut out);
        assert_eq!(out, vec![1.0, 2.0, 3.0, 4.0]);
    }

    #[test]
    fn mono_duplicates_to_both_ears() {
        let mut out = Vec::new();
        interleaved_to_stereo(&[1.0, 2.0, 3.0], 1, &mut out);
        assert_eq!(out, vec![1.0, 1.0, 2.0, 2.0, 3.0, 3.0]);
    }

    #[test]
    fn surround_keeps_front_two() {
        let mut out = Vec::new();
        // Two frames of 5.1 (6 channels): only the first two survive.
        let frame0 = [10.0, 11.0, 12.0, 13.0, 14.0, 15.0];
        let frame1 = [20.0, 21.0, 22.0, 23.0, 24.0, 25.0];
        let samples: Vec<f32> = frame0.into_iter().chain(frame1).collect();
        interleaved_to_stereo(&samples, 6, &mut out);
        assert_eq!(out, vec![10.0, 11.0, 20.0, 21.0]);
    }

    #[test]
    fn planar_mono_duplicates() {
        let mut out = Vec::new();
        let mono = [1.0, 2.0, 3.0];
        let planes: [&[f32]; 1] = [&mono];
        planar_to_stereo(&planes, &mut out);
        assert_eq!(out, vec![1.0, 1.0, 2.0, 2.0, 3.0, 3.0]);
    }

    #[test]
    fn planar_stereo_interleaves() {
        let mut out = Vec::new();
        let left = [1.0, 3.0];
        let right = [2.0, 4.0];
        let planes: [&[f32]; 2] = [&left, &right];
        planar_to_stereo(&planes, &mut out);
        assert_eq!(out, vec![1.0, 2.0, 3.0, 4.0]);
    }

    /// The sink writes a pending format on `configure`, drops pushes until a
    /// producer is installed, and flags `stopped` on `stopped`.
    #[test]
    fn sink_control_transitions() {
        let (stream, sink) = MixerStream::new(Bus::Media, false);
        // A push before any producer is installed is a silent no-op.
        sink.push_interleaved(&[0.5, 0.5], 2);
        sink.configure(44_100, 2);
        let (pending, stopped) = {
            let control = stream.control.lock().unwrap_or_else(|p| p.into_inner());
            (control.pending_format, control.stopped)
        };
        assert_eq!(
            pending,
            Some(PcmFormat {
                sample_rate: 44_100
            })
        );
        assert!(!stopped);

        sink.set_muted(true);
        sink.stopped();
        let (stopped, muted, has_producer) = {
            let control = stream.control.lock().unwrap_or_else(|p| p.into_inner());
            (control.stopped, control.muted, control.producer.is_some())
        };
        assert!(stopped);
        assert!(muted);
        assert!(!has_producer);
    }
}
