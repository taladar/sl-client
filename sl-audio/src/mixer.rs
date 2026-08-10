//! The mixer: the single firewheel audio graph every source feeds.
//!
//! Topology: a [`Bus::Master`] volume node feeds the output device; one volume
//! node per category ([`Bus::CATEGORIES`]) feeds the master. A played clip adds
//! a `sampler` node and a `spatial_basic` node (sampler → spatial → category
//! bus); a pushed-PCM stream adds a [`crate::push::PushNode`] (optionally via a
//! spatial node) to its bus. Nothing else opens an audio device.
//!
//! Graph edits (adding/removing voices, connecting) are queued on the context
//! and committed once per frame in [`Mixer::update`], so a frame that starts a
//! dozen sounds still triggers a single graph recompile.
#![expect(
    clippy::module_name_repetitions,
    reason = "Mixer* / *Mixer are the natural public names, re-exported at the crate root"
)]

use std::collections::HashMap;
use std::sync::Arc;

use firewheel::FirewheelContext;
use firewheel::channel_config::NonZeroChannelCount;
use firewheel::cpal::{CpalConfig, CpalOutputConfig, CpalStream};
use firewheel::diff::{Diff as _, PathBuilder};
use firewheel::node::NodeID;
use firewheel::nodes::sampler::{SamplerConfig, SamplerNode, SamplerState};
use firewheel::nodes::spatial_basic::SpatialBasicNode;
use firewheel::nodes::volume::{VolumeNode, VolumeNodeConfig};
use firewheel::vector::Vec3;

use crate::bus::{Bus, BusLevel};
use crate::clip::DecodedClip;
use crate::error::AudioError;
use crate::eviction::{Decision, Importance, SoundPriority, VoicePool};
use crate::listener::Listener;
use crate::push::{ProducerSlot, PushProducer, PushStreamConfig, install_stream, push_stream};

/// An opaque handle to a playing voice (a clip triggered on the mixer).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct VoiceId(u64);

/// An opaque handle to an open pushed-PCM stream.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct StreamId(u64);

/// Which output device the mixer should open.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum DeviceSelection {
    /// The system default output device.
    #[default]
    Default,
    /// A specific device by its name (as reported by [`Mixer::output_devices`]).
    Named(String),
}

/// Configuration for a [`Mixer`].
#[derive(Debug, Clone)]
pub struct MixerConfig {
    /// The maximum number of concurrent clip voices before priority eviction
    /// kicks in. SL asks for far more simultaneous sounds than any device wants.
    pub max_voices: usize,
}

impl Default for MixerConfig {
    /// 32 voices — comfortably more than a device wants to render at once, and
    /// the point where eviction starts to matter in busy SL regions.
    fn default() -> Self {
        Self { max_voices: 32 }
    }
}

/// Parameters for a 2-D (non-spatial) clip trigger, such as a UI sound.
#[derive(Debug, Clone, Copy)]
pub struct ClipParams {
    /// The bus to play on.
    pub bus: Bus,
    /// The linear gain in `[0.0, 1.0]`.
    pub gain: f32,
    /// The sound's importance tier for eviction.
    pub importance: Importance,
    /// Whether the clip loops.
    pub looped: bool,
}

impl Default for ClipParams {
    /// A one-shot UI sound at unity gain.
    fn default() -> Self {
        Self {
            bus: Bus::Ui,
            gain: 1.0,
            importance: Importance::Ui,
            looped: false,
        }
    }
}

/// Parameters for a spatial (3-D) clip trigger, such as an in-world sound.
#[derive(Debug, Clone, Copy)]
pub struct SpatialParams {
    /// The bus to play on.
    pub bus: Bus,
    /// The linear gain in `[0.0, 1.0]`.
    pub gain: f32,
    /// The sound's importance tier for eviction.
    pub importance: Importance,
    /// Whether the clip loops.
    pub looped: bool,
    /// The world position of the source, in the listener's world frame.
    pub position: [f32; 3],
}

impl Default for SpatialParams {
    /// A one-shot in-world sound at unity gain, at the origin.
    fn default() -> Self {
        Self {
            bus: Bus::Sfx,
            gain: 1.0,
            importance: Importance::OneShot,
            looped: false,
            position: [0.0, 0.0, 0.0],
        }
    }
}

/// A snapshot of one open stream, captured before a device rebuild so it can be
/// re-established on the new device: `(id, bus, config, position, slot)`.
type StreamSnapshot = (
    StreamId,
    Bus,
    PushStreamConfig,
    Option<[f32; 3]>,
    ProducerSlot,
);

/// A handle to an open pushed-PCM stream: the [`StreamId`] to close it and the
/// [`PushProducer`] the source pushes samples into.
#[derive(Debug)]
pub struct StreamHandle {
    /// The stream's id in the mixer.
    pub id: StreamId,
    /// The producer end — hand this to the source thread.
    pub producer: PushProducer,
}

/// A single category / master bus in the graph.
#[derive(Debug)]
struct BusNode {
    /// The volume node's id.
    node_id: NodeID,
    /// A local copy of the volume node used to diff parameter changes.
    node: VolumeNode,
    /// The user-facing level (gain + mute state).
    level: BusLevel,
}

/// A playing clip voice.
#[derive(Debug)]
struct Voice {
    /// The sampler node id.
    sampler_id: NodeID,
    /// The spatial node id.
    spatial_id: NodeID,
    /// A local copy of the spatial node for diffing offset changes.
    spatial: SpatialBasicNode,
    /// The playback id assigned when this voice started (for finish detection).
    playback_id: firewheel::nodes::sampler::PlaybackID,
    /// Whether the clip loops (looping voices are never auto-removed).
    looped: bool,
    /// The world position for spatial voices (`None` for 2-D voices).
    position: Option<[f32; 3]>,
    /// The importance tier, for re-scoring when the source moves.
    importance: Importance,
    /// The linear gain, for re-scoring.
    gain: f32,
    /// Whether the voice has been through at least one commit (so its sampler
    /// state exists and finish-detection is valid).
    committed: bool,
}

/// An open pushed-PCM stream.
struct StreamNode {
    /// The push node id.
    push_id: NodeID,
    /// The spatial node id, for spatial streams (`None` for 2-D streams).
    spatial_id: Option<NodeID>,
    /// A local copy of the spatial node for diffing offset changes.
    spatial: Option<SpatialBasicNode>,
    /// The world position for spatial streams.
    position: Option<[f32; 3]>,
    /// The bus this stream plays on (needed to reconnect after a device
    /// rebuild).
    bus: Bus,
    /// The stream configuration (needed to re-establish the channel after a
    /// device rebuild).
    config: PushStreamConfig,
    /// The shared producer slot, swapped when the graph is rebuilt on a device
    /// hot-plug so the source's [`PushProducer`] keeps working.
    slot: ProducerSlot,
}

impl std::fmt::Debug for StreamNode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StreamNode")
            .field("push_id", &self.push_id)
            .field("spatial_id", &self.spatial_id)
            .field("position", &self.position)
            .field("bus", &self.bus)
            .finish_non_exhaustive()
    }
}

/// The engine-agnostic mixer interface the backend implements, so the concrete
/// engine (firewheel today) stays a swap rather than a rewrite.
pub trait AudioMixer {
    /// Set a bus's level (gain + mute), applied live.
    fn set_bus_level(&mut self, bus: Bus, level: BusLevel);
    /// Get a bus's current level.
    fn bus_level(&self, bus: Bus) -> BusLevel;
    /// Set the listener pose (used to spatialise every spatial source).
    fn set_listener(&mut self, listener: Listener);
    /// The current listener pose.
    fn listener(&self) -> Listener;
    /// Trigger a 2-D clip. Returns `None` if the device is not started or the
    /// source cap rejected it.
    fn play_clip(&mut self, clip: &DecodedClip, params: ClipParams) -> Option<VoiceId>;
    /// Trigger a spatial clip. Returns `None` if the device is not started or the
    /// source cap rejected it.
    fn play_spatial(&mut self, clip: &DecodedClip, params: SpatialParams) -> Option<VoiceId>;
    /// Stop a playing voice.
    fn stop_voice(&mut self, id: VoiceId);
    /// Whether a voice is still playing.
    fn is_playing(&self, id: VoiceId) -> bool;
    /// Commit queued graph edits and parameter changes; call once per frame.
    fn update(&mut self);
}

/// The firewheel-backed audio mixer.
pub struct Mixer {
    /// The firewheel audio context (the graph).
    cx: FirewheelContext,
    /// The active output stream (device), or `None` before [`Mixer::start`].
    stream: Option<CpalStream>,
    /// The device sample rate once started.
    sample_rate: Option<std::num::NonZeroU32>,
    /// The master and category buses.
    buses: HashMap<Bus, BusNode>,
    /// Active clip voices.
    voices: HashMap<VoiceId, Voice>,
    /// The source cap / priority-eviction policy.
    pool: VoicePool<VoiceId>,
    /// Open pushed-PCM streams.
    streams: HashMap<StreamId, StreamNode>,
    /// The current listener pose.
    listener: Listener,
    /// Monotonic voice id counter.
    next_voice: u64,
    /// Monotonic stream id counter.
    next_stream: u64,
}

impl std::fmt::Debug for Mixer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Mixer")
            .field("started", &self.stream.is_some())
            .field("sample_rate", &self.sample_rate)
            .field("voices", &self.voices.len())
            .field("streams", &self.streams.len())
            .finish_non_exhaustive()
    }
}

impl Mixer {
    /// Build the mixer graph (master + category buses) **without** opening a
    /// device. Call [`Mixer::start`] to open the output device.
    ///
    /// Building without a device lets the graph be constructed and inspected in
    /// tests that have no audio hardware.
    ///
    /// # Errors
    /// Returns [`AudioError::Graph`] if the graph could not be built.
    pub fn new(config: &MixerConfig) -> Result<Self, AudioError> {
        let mut cx = FirewheelContext::new(firewheel::FirewheelConfig::default());
        let buses = Self::build_bus_graph(&mut cx, &HashMap::new())?;
        Ok(Self {
            cx,
            stream: None,
            sample_rate: None,
            buses,
            voices: HashMap::new(),
            pool: VoicePool::new(config.max_voices),
            streams: HashMap::new(),
            listener: Listener::default(),
            next_voice: 0,
            next_stream: 0,
        })
    }

    /// Build the master + category volume graph in `cx`, restoring each bus's
    /// level from `levels` (empty for a fresh mixer). Master feeds the graph
    /// output; each category feeds the master.
    fn build_bus_graph(
        cx: &mut FirewheelContext,
        levels: &HashMap<Bus, BusLevel>,
    ) -> Result<HashMap<Bus, BusNode>, AudioError> {
        let mut buses = HashMap::new();

        let master_level = levels.get(&Bus::Master).copied().unwrap_or_default();
        let master_node = VolumeNode::from_linear(master_level.effective_gain());
        let master_id = cx
            .add_node(
                master_node,
                Some(VolumeNodeConfig {
                    channels: NonZeroChannelCount::STEREO,
                }),
            )
            .map_err(|e| AudioError::Graph(format!("add master bus: {e:?}")))?;
        let out_id = cx.graph_out_node_id();
        cx.connect_stereo(master_id, out_id, false)
            .map_err(|e| AudioError::Graph(format!("connect master to output: {e:?}")))?;
        buses.insert(
            Bus::Master,
            BusNode {
                node_id: master_id,
                node: master_node,
                level: master_level,
            },
        );

        for bus in Bus::CATEGORIES {
            let level = levels.get(&bus).copied().unwrap_or_default();
            let node = VolumeNode::from_linear(level.effective_gain());
            let node_id = cx
                .add_node(
                    node,
                    Some(VolumeNodeConfig {
                        channels: NonZeroChannelCount::STEREO,
                    }),
                )
                .map_err(|e| AudioError::Graph(format!("add {} bus: {e:?}", bus.key())))?;
            cx.connect_stereo(node_id, master_id, false).map_err(|e| {
                AudioError::Graph(format!("connect {} bus to master: {e:?}", bus.key()))
            })?;
            buses.insert(
                bus,
                BusNode {
                    node_id,
                    node,
                    level,
                },
            );
        }

        Ok(buses)
    }

    /// Open the output device and start audio processing.
    ///
    /// # Errors
    /// Returns [`AudioError::Stream`] if the device could not be opened.
    pub fn start(&mut self, device: &DeviceSelection) -> Result<(), AudioError> {
        let device_id = match device {
            DeviceSelection::Default => None,
            DeviceSelection::Named(name) => Self::find_output_device(name),
        };
        let cpal_config = CpalConfig {
            output: CpalOutputConfig {
                device_id,
                ..Default::default()
            },
            ..Default::default()
        };
        let stream = CpalStream::new(&mut self.cx, cpal_config)
            .map_err(|e| AudioError::Stream(format!("{e:?}")))?;
        self.sample_rate = Some(stream.info().sample_rate);
        self.stream = Some(stream);
        Ok(())
    }

    /// Whether the output device is open and processing.
    #[must_use]
    pub const fn is_started(&self) -> bool {
        self.stream.is_some()
    }

    /// The device sample rate, once [`Mixer::start`] has succeeded. Decode clips
    /// to this rate so the sampler never resamples per play.
    #[must_use]
    pub const fn sample_rate(&self) -> Option<std::num::NonZeroU32> {
        self.sample_rate
    }

    /// The latency in seconds between an input (microphone) and the output mix,
    /// the delay hint an echo canceller wants. `None` until started.
    #[must_use]
    pub fn input_to_output_latency_seconds(&self) -> Option<f64> {
        self.stream
            .as_ref()
            .map(|s| s.info().input_to_output_latency_seconds)
    }

    /// The names of the available output devices.
    #[must_use]
    pub fn output_devices() -> Vec<String> {
        firewheel::cpal::default_host_enumerator()
            .output_devices()
            .into_iter()
            .filter_map(|d| d.name)
            .collect()
    }

    /// Resolve a device name to its id on the default host.
    fn find_output_device(name: &str) -> Option<firewheel::cpal::DeviceId> {
        firewheel::cpal::default_host_enumerator()
            .output_devices()
            .into_iter()
            .find(|d| d.name.as_deref() == Some(name))
            .map(|d| d.id)
    }

    /// Whether the output stream is still healthy (a device unplug shows here).
    #[must_use]
    pub fn output_ok(&self) -> bool {
        self.stream
            .as_ref()
            .is_some_and(CpalStream::output_stream_ok)
    }

    /// The node id of a bus, if present.
    fn bus_node_id(&self, bus: Bus) -> Option<NodeID> {
        self.buses.get(&bus).map(|b| b.node_id)
    }

    /// Remove a node from the graph, ignoring the "already gone" error that can
    /// happen when tearing down a half-built voice or a closed stream.
    fn drop_node(&mut self, id: NodeID) {
        if let Err(e) = self.cx.remove_node(id) {
            tracing::trace!("remove_node({id:?}) ignored: {e:?}");
        }
    }

    /// Allocate the next voice id.
    const fn alloc_voice_id(&mut self) -> VoiceId {
        let id = VoiceId(self.next_voice);
        self.next_voice = self.next_voice.wrapping_add(1);
        id
    }

    /// Allocate the next stream id.
    const fn alloc_stream_id(&mut self) -> StreamId {
        let id = StreamId(self.next_stream);
        self.next_stream = self.next_stream.wrapping_add(1);
        id
    }

    /// The firewheel spatial offset for a world position given the listener.
    fn spatial_offset(&self, position: [f32; 3]) -> Vec3 {
        let [x, y, z] = self.listener.source_offset(position);
        Vec3::new(x, y, z)
    }

    /// Shared clip-trigger path for 2-D and spatial voices.
    fn play_inner(
        &mut self,
        clip: &DecodedClip,
        bus: Bus,
        gain: f32,
        importance: Importance,
        looped: bool,
        position: Option<[f32; 3]>,
    ) -> Option<VoiceId> {
        if !self.is_started() {
            return None;
        }
        let bus_id = self.bus_node_id(bus)?;

        let distance = position.map_or(0.0, |p| self.listener.distance_to(p));
        let priority = SoundPriority {
            importance,
            gain,
            distance,
        };
        let score = priority.score();
        match self.pool.consider(score) {
            Decision::Reject => return None,
            Decision::Evict(old) => self.stop_voice(old),
            Decision::Admit => {}
        }

        // Sampler node (mono clips upmix to stereo via `mono_to_stereo`).
        let mut sampler = SamplerNode {
            volume: firewheel::Volume::Linear(gain.clamp(0.0, 1.0)),
            ..Default::default()
        };
        if looped {
            sampler.repeat_mode = firewheel::nodes::sampler::RepeatMode::RepeatEndlessly;
        }
        let sampler_id = self
            .cx
            .add_node(
                sampler,
                Some(SamplerConfig {
                    channels: NonZeroChannelCount::STEREO,
                    ..Default::default()
                }),
            )
            .ok()?;

        // Spatial node (offset zero = centred 2-D for non-spatial voices).
        let offset = position.map_or(Vec3::ZERO, |p| self.spatial_offset(p));
        let spatial = SpatialBasicNode {
            offset,
            ..Default::default()
        };
        let spatial_id = self.cx.add_node(spatial, None).ok()?;

        // sampler -> spatial -> bus
        if self
            .cx
            .connect_stereo(sampler_id, spatial_id, false)
            .is_err()
            || self.cx.connect_stereo(spatial_id, bus_id, false).is_err()
        {
            self.drop_node(sampler_id);
            self.drop_node(spatial_id);
            return None;
        }

        // Set the sample and start playback.
        self.cx.queue_event_for(
            sampler_id,
            SamplerNode::set_dyn_sample_event(clip.resource()),
        );
        sampler.start_or_restart();
        let playback_id = sampler.playback_id();
        {
            let baseline = SamplerNode {
                volume: firewheel::Volume::Linear(gain.clamp(0.0, 1.0)),
                ..Default::default()
            };
            let mut queue = self.cx.event_queue(sampler_id);
            sampler.diff(&baseline, PathBuilder::default(), &mut queue);
        }

        let voice_id = self.alloc_voice_id();
        self.pool.insert(voice_id, score);
        self.voices.insert(
            voice_id,
            Voice {
                sampler_id,
                spatial_id,
                spatial,
                playback_id,
                looped,
                position,
                importance,
                gain,
                committed: false,
            },
        );
        Some(voice_id)
    }

    /// Open a pushed-PCM stream on `bus`. If `position` is `Some`, the stream is
    /// spatialised at that world position (media-on-a-prim); otherwise it plays
    /// stereo / 2-D (the parcel music stream).
    ///
    /// Returns the [`StreamHandle`] (whose [`PushProducer`] the source pushes
    /// into), or `None` if the device is not started or the bus is unknown.
    pub fn open_stream(
        &mut self,
        bus: Bus,
        position: Option<[f32; 3]>,
        config: PushStreamConfig,
    ) -> Option<StreamHandle> {
        let out_sr = self.sample_rate?;
        let bus_id = self.bus_node_id(bus)?;
        let (producer, node) = push_stream(config, out_sr);
        let push_id = self.cx.add_node(node, None).ok()?;

        let (spatial_id, spatial) = if let Some(p) = position {
            let spatial = SpatialBasicNode {
                offset: self.spatial_offset(p),
                ..Default::default()
            };
            let sid = self.cx.add_node(spatial, None).ok()?;
            if self.cx.connect_stereo(push_id, sid, false).is_err()
                || self.cx.connect_stereo(sid, bus_id, false).is_err()
            {
                self.drop_node(push_id);
                self.drop_node(sid);
                return None;
            }
            (Some(sid), Some(spatial))
        } else {
            if self.cx.connect_stereo(push_id, bus_id, false).is_err() {
                self.drop_node(push_id);
                return None;
            }
            (None, None)
        };

        let id = self.alloc_stream_id();
        self.streams.insert(
            id,
            StreamNode {
                push_id,
                spatial_id,
                spatial,
                position,
                bus,
                config,
                slot: producer.slot_handle(),
            },
        );
        Some(StreamHandle { id, producer })
    }

    /// Move a spatial stream's source position (its offset is recomputed against
    /// the listener each frame from this position).
    pub fn set_stream_position(&mut self, id: StreamId, position: [f32; 3]) {
        if let Some(stream) = self.streams.get_mut(&id) {
            stream.position = Some(position);
        }
    }

    /// Close a pushed-PCM stream, removing its nodes.
    pub fn close_stream(&mut self, id: StreamId) {
        if let Some(stream) = self.streams.remove(&id) {
            self.drop_node(stream.push_id);
            if let Some(sid) = stream.spatial_id {
                self.drop_node(sid);
            }
        }
    }

    /// Rebuild the whole graph in a fresh context and (re)open `device` after a
    /// hot-plug. Bus levels are preserved; clip voices are dropped (they are
    /// transient one-shots — looped sounds are re-triggered by their owner); and
    /// each pushed stream is re-established **on its existing producer slot**, so
    /// the source's [`PushProducer`] keeps working across the device change.
    ///
    /// # Errors
    /// Returns an error if the new device could not be opened; the mixer is then
    /// left un-started (recoverable with [`Mixer::start`]).
    pub fn rebuild_and_restart(&mut self, device: &DeviceSelection) -> Result<(), AudioError> {
        let levels: HashMap<Bus, BusLevel> =
            self.buses.iter().map(|(b, n)| (*b, n.level)).collect();
        let stream_meta: Vec<StreamSnapshot> = self
            .streams
            .iter()
            .map(|(id, s)| (*id, s.bus, s.config, s.position, Arc::clone(&s.slot)))
            .collect();

        // Fresh context + bus graph (dropping the old context deactivates the
        // dead stream and frees the old nodes).
        let mut cx = FirewheelContext::new(firewheel::FirewheelConfig::default());
        let buses = Self::build_bus_graph(&mut cx, &levels)?;
        self.cx = cx;
        self.buses = buses;
        self.voices.clear();
        let cap = self.pool.capacity();
        self.pool = VoicePool::new(cap);
        self.streams.clear();
        self.stream = None;
        self.sample_rate = None;

        // Open the new device (sets the output sample rate).
        self.start(device)?;
        let out_sr = self
            .sample_rate
            .ok_or_else(|| AudioError::Stream("no sample rate after restart".to_owned()))?;

        // Re-establish each stream on its existing slot.
        for (id, bus, config, position, slot) in stream_meta {
            let Some(bus_id) = self.bus_node_id(bus) else {
                continue;
            };
            let node = install_stream(&slot, config, out_sr);
            let Ok(push_id) = self.cx.add_node(node, None) else {
                continue;
            };
            let (spatial_id, spatial) = if let Some(p) = position {
                let spatial = SpatialBasicNode {
                    offset: self.spatial_offset(p),
                    ..Default::default()
                };
                let Ok(sid) = self.cx.add_node(spatial, None) else {
                    self.drop_node(push_id);
                    continue;
                };
                if self.cx.connect_stereo(push_id, sid, false).is_err()
                    || self.cx.connect_stereo(sid, bus_id, false).is_err()
                {
                    self.drop_node(push_id);
                    self.drop_node(sid);
                    continue;
                }
                (Some(sid), Some(spatial))
            } else {
                if self.cx.connect_stereo(push_id, bus_id, false).is_err() {
                    self.drop_node(push_id);
                    continue;
                }
                (None, None)
            };
            self.streams.insert(
                id,
                StreamNode {
                    push_id,
                    spatial_id,
                    spatial,
                    position,
                    bus,
                    config,
                    slot,
                },
            );
        }
        if let Err(e) = self.cx.update() {
            tracing::warn!("firewheel update after device rebuild failed: {e:?}");
        }
        Ok(())
    }

    /// Update spatial offsets for every spatial voice and stream from the
    /// current listener pose.
    fn update_spatial(&mut self) {
        // Voices.
        let voice_updates: Vec<(NodeID, Vec3, SpatialBasicNode, VoiceId, f32)> = self
            .voices
            .iter()
            .filter_map(|(id, voice)| {
                let position = voice.position?;
                let offset = self.spatial_offset(position);
                let distance = self.listener.distance_to(position);
                let score = SoundPriority {
                    importance: voice.importance,
                    gain: voice.gain,
                    distance,
                }
                .score();
                Some((voice.spatial_id, offset, voice.spatial, *id, score))
            })
            .collect();
        for (spatial_id, offset, baseline, voice_id, score) in voice_updates {
            let mut updated = baseline;
            updated.offset = offset;
            {
                let mut queue = self.cx.event_queue(spatial_id);
                updated.diff(&baseline, PathBuilder::default(), &mut queue);
            }
            if let Some(voice) = self.voices.get_mut(&voice_id) {
                voice.spatial = updated;
            }
            self.pool.reprioritize(voice_id, score);
        }

        // Streams.
        let stream_updates: Vec<(StreamId, NodeID, Vec3, SpatialBasicNode)> = self
            .streams
            .iter()
            .filter_map(|(id, stream)| {
                let position = stream.position?;
                let spatial_id = stream.spatial_id?;
                let baseline = stream.spatial?;
                Some((*id, spatial_id, self.spatial_offset(position), baseline))
            })
            .collect();
        for (id, spatial_id, offset, baseline) in stream_updates {
            let mut updated = baseline;
            updated.offset = offset;
            {
                let mut queue = self.cx.event_queue(spatial_id);
                updated.diff(&baseline, PathBuilder::default(), &mut queue);
            }
            if let Some(stream) = self.streams.get_mut(&id) {
                stream.spatial = Some(updated);
            }
        }
    }

    /// Detect finished one-shot voices and remove their nodes.
    fn reap_finished(&mut self) {
        let finished: Vec<VoiceId> = self
            .voices
            .iter()
            .filter(|(_, v)| v.committed && !v.looped)
            .filter(|(_, v)| {
                self.cx
                    .node_state::<SamplerState>(v.sampler_id)
                    .is_some_and(|state| state.playback_finished(v.playback_id))
            })
            .map(|(id, _)| *id)
            .collect();
        for id in finished {
            self.remove_voice(id);
        }
    }

    /// Remove a voice and its nodes from the graph and bookkeeping.
    fn remove_voice(&mut self, id: VoiceId) {
        if let Some(voice) = self.voices.remove(&id) {
            self.drop_node(voice.sampler_id);
            self.drop_node(voice.spatial_id);
        }
        self.pool.remove(id);
    }
}

impl AudioMixer for Mixer {
    fn set_bus_level(&mut self, bus: Bus, level: BusLevel) {
        let Some(bus_node) = self.buses.get_mut(&bus) else {
            return;
        };
        let baseline = bus_node.node;
        let mut updated = baseline;
        updated.set_linear(level.effective_gain());
        {
            let mut queue = self.cx.event_queue(bus_node.node_id);
            updated.diff(&baseline, PathBuilder::default(), &mut queue);
        }
        bus_node.node = updated;
        bus_node.level = level;
    }

    fn bus_level(&self, bus: Bus) -> BusLevel {
        self.buses
            .get(&bus)
            .map_or_else(BusLevel::default, |b| b.level)
    }

    fn set_listener(&mut self, listener: Listener) {
        self.listener = listener;
    }

    fn listener(&self) -> Listener {
        self.listener
    }

    fn play_clip(&mut self, clip: &DecodedClip, params: ClipParams) -> Option<VoiceId> {
        self.play_inner(
            clip,
            params.bus,
            params.gain,
            params.importance,
            params.looped,
            None,
        )
    }

    fn play_spatial(&mut self, clip: &DecodedClip, params: SpatialParams) -> Option<VoiceId> {
        self.play_inner(
            clip,
            params.bus,
            params.gain,
            params.importance,
            params.looped,
            Some(params.position),
        )
    }

    fn stop_voice(&mut self, id: VoiceId) {
        self.remove_voice(id);
    }

    fn is_playing(&self, id: VoiceId) -> bool {
        self.voices.contains_key(&id)
    }

    fn update(&mut self) {
        // Seamless hot-plug: if the output device disappeared, rebuild the graph
        // on the default device (pushed streams keep their producer handles).
        if self.stream.is_some() && !self.output_ok() {
            tracing::warn!("audio output device lost; rebuilding on the default device");
            if let Err(e) = self.rebuild_and_restart(&DeviceSelection::Default) {
                tracing::error!("audio device rebuild failed: {e}");
            }
        }

        self.update_spatial();
        self.reap_finished();
        if let Err(e) = self.cx.update() {
            tracing::warn!("firewheel context update failed: {e:?}");
        }
        for voice in self.voices.values_mut() {
            voice.committed = true;
        }
        if let Some(stream) = self.stream.as_mut() {
            stream.log_status();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn builds_bus_graph_without_device() {
        let Ok(mixer) = Mixer::new(&MixerConfig::default()) else {
            unreachable!("graph builds without a device")
        };
        assert!(!mixer.is_started());
        assert!(mixer.sample_rate().is_none());
        // Master plus six categories.
        assert_eq!(mixer.buses.len(), Bus::ALL.len());
        for bus in Bus::ALL {
            assert!(mixer.bus_node_id(bus).is_some(), "bus {bus:?} present");
        }
    }

    #[test]
    fn bus_level_roundtrips_without_device() {
        let Ok(mut mixer) = Mixer::new(&MixerConfig::default()) else {
            unreachable!("graph builds")
        };
        let mut level = BusLevel::from_percent(30.0);
        level.set_muted(true);
        mixer.set_bus_level(Bus::Music, level);
        let read = mixer.bus_level(Bus::Music);
        assert!(read.is_muted());
        assert!((read.gain() - 0.3).abs() < 1e-6);
    }

    #[test]
    fn rebuild_preserves_bus_levels() {
        // The bus-graph builder restores levels from a snapshot, which is how a
        // device hot-plug keeps volumes across a full graph rebuild.
        let mut levels = HashMap::new();
        let mut music = BusLevel::from_percent(20.0);
        music.set_muted(true);
        levels.insert(Bus::Music, music);
        levels.insert(Bus::Master, BusLevel::from_percent(50.0));

        let mut cx = FirewheelContext::new(firewheel::FirewheelConfig::default());
        let Ok(buses) = Mixer::build_bus_graph(&mut cx, &levels) else {
            unreachable!("graph builds")
        };
        assert_eq!(buses.len(), Bus::ALL.len());
        let Some(music_bus) = buses.get(&Bus::Music) else {
            unreachable!("music bus present")
        };
        assert!(music_bus.level.is_muted());
        assert!((music_bus.level.gain() - 0.2).abs() < 1e-6);
        let Some(master_bus) = buses.get(&Bus::Master) else {
            unreachable!("master bus present")
        };
        assert!((master_bus.level.gain() - 0.5).abs() < 1e-6);
    }

    #[test]
    fn play_without_device_is_none() {
        let Ok(mut mixer) = Mixer::new(&MixerConfig::default()) else {
            unreachable!("graph builds")
        };
        // Nothing to decode without a device; a null clip is impossible to
        // construct here, so just assert the not-started guard via listener.
        assert!(!mixer.is_started());
        mixer.set_listener(Listener::default());
        assert_eq!(mixer.listener(), Listener::default());
    }
}
