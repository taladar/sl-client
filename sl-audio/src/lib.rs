//! Engine-agnostic audio mixer for Second Life / OpenSim viewers.
//!
//! This crate owns the **one mixer** that every audio source in the viewer
//! feeds — the foundation the `viewer-audio-backend` roadmap task builds. No
//! source may open its own audio device: in-world sound effects, UI sounds, the
//! parcel music stream, media-on-a-prim and page audio, and voice all route
//! through here, so mute, per-category volume, spatialisation and the source cap
//! are decided in one place (which is exactly what lets media-on-a-prim audio be
//! genuinely positional — something no SL viewer manages today).
//!
//! # What is here
//!
//! - [`Bus`] / [`BusLevel`] — the fixed set of volume categories and their
//!   gain/mute state (mute retains the previous level and never stops a source).
//! - [`Listener`] / [`EarMode`] — where the ears are (camera vs. avatar head)
//!   and the math turning a world position into the listener-relative offset the
//!   spatial node wants.
//! - [`SoundPriority`] / [`VoicePool`] — the source cap and priority eviction:
//!   SL asks for more simultaneous sounds than any device wants.
//! - [`DecodedClip`] / [`ClipCache`] / [`decode_clip`] — decode short SL sounds
//!   once (Ogg Vorbis / WAV) and cache them by asset id.
//! - [`Mixer`] — the firewheel-backed graph: a master volume node feeding the
//!   device, one volume node per category feeding the master, clip playback
//!   (2-D and spatial), and a realtime-safe pushed-PCM path for GStreamer / CEF
//!   / decoded-Opus streams whose clock is not the sound card's.
//! - [`AudioMixer`] — the trait the mixer implements, so the backend
//!   (firewheel today) stays a swap rather than a rewrite.
//!
//! # Backend
//!
//! The mixer is built on the engine-agnostic [`firewheel`] audio graph (its
//! `volume`, `spatial_basic` and `sampler` nodes), with `symphonium` for clip
//! decode and a custom node fed by a [`fixed_resample`] resampling channel for
//! pushed PCM. There is deliberately no Bevy dependency here; the viewer wires
//! this crate to the ECS with its own thin glue.

pub mod bus;
pub mod clip;
pub mod error;
pub mod eviction;
pub mod listener;
pub mod mixer;
pub mod push;

pub use bus::{Bus, BusLevel};
pub use clip::{ClipCache, DecodedClip, decode_clip};
pub use error::AudioError;
pub use eviction::{Decision, Importance, SoundPriority, VoicePool};
pub use listener::{EarMode, Listener};
pub use mixer::{
    AudioMixer, ClipParams, DeviceSelection, Mixer, MixerConfig, SpatialParams, StreamHandle,
    StreamId, VoiceId,
};
pub use push::{PushOutcome, PushProducer, PushStreamConfig};
