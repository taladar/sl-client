//! GStreamer playback media engine for Second Life / OpenSim viewers.
//!
//! This crate is the **second media engine** behind the [`sl_media`]
//! boundary — the browser engine (`sl-cef`) renders web pages, this one
//! plays **direct video / audio URLs**: `.mp4` / `.webm` / `.mkv` files, HLS
//! and DASH manifests, RTSP feeds, and Shoutcast / Icecast radio streams.
//! The split mirrors the reference viewer, whose `mime_types.xml` dispatches
//! `video/*` and `audio/*` to a dedicated media plugin (libvlc / GStreamer)
//! while HTML goes to CEF — and it exists for codec reasons: prebuilt CEF is
//! open-codec-only, while GStreamer picks up the **system's** decoders.
//!
//! We deliberately ship no encumbered decoder (see
//! `roadmap/in-progress/viewer-video-playback.md`): H.264 / AAC come from the
//! user's platform plugins (VA-API, libav, Media Foundation, VideoToolbox) or
//! not at all — and when they are absent the failure is loud and useful: the
//! surface's [`sl_media::SurfaceStatus::load_error`] carries GStreamer's
//! `missing-plugin` description rather than a silent black square.
//!
//! Two consumers, two APIs:
//!
//! - [`GstMediaBackend`] / the [`MediaSurface`] it creates — media-on-a-prim
//!   video: a `playbin3` per surface whose frames arrive through a BGRA
//!   `appsink` ([`MediaSurface::with_new_frame`]), with play / pause / seek /
//!   volume via the trait's playback half.
//! - [`AudioStreamPlayer`] — the parcel radio stream (audio only, no
//!   surface): play / stop / volume plus the ICY "now playing" title.
//!
//! **Interim audio path**: GStreamer currently outputs audio straight to the
//! system device (`autoaudiosink`), exactly like the CEF engine does — the
//! shared viewer mixer (`viewer-audio-backend`) does not exist yet. When it
//! lands, both consumers switch their audio sink to an `appsink` feeding the
//! mixer's resampling channel and gain spatialisation; the public API here
//! already keeps volume / mute per surface so that switch stays internal.
//! Until then GStreamer owns its own clock and audio device, which also
//! settles A/V sync the boring way: both sinks sync to the pipeline clock.

use std::sync::{Arc, Mutex, Weak};

use sl_media::{MediaBackend, MediaError, MediaSurface, SurfaceConfig};

mod messages;
pub mod stream;
mod surface;

pub use stream::{AudioStreamPlayer, AudioStreamState, AudioStreamStatus};

/// Initialises the process-global GStreamer runtime (idempotent).
///
/// # Errors
/// Returns [`MediaError::Init`] when GStreamer cannot initialise (broken
/// installation, no registry).
pub fn ensure_initialized() -> Result<(), MediaError> {
    gstreamer::init().map_err(|error| MediaError::Init(error.to_string()))
}

/// Human-readable notes about playback capabilities that are *absent* on this
/// system — logged once at startup so "why is this video black" has an
/// answer in the log before anyone files it as a bug. Empty when everything
/// commonly needed is present.
#[must_use]
pub fn playback_gaps() -> Vec<String> {
    /// The capabilities worth probing: an element name (or `uri:` probe) and
    /// what its absence costs.
    const PROBES: &[(&str, &str)] = &[
        (
            "uri:https://probe.invalid/",
            "no HTTP(S) source element — web-hosted media and radio streams cannot start \
             (install the GStreamer soup plugin, e.g. gst-plugins-soup)",
        ),
        (
            "element:hlsdemux2",
            "no HLS demuxer — .m3u8 streams will not play (install the GStreamer \
             adaptivedemux2 plugin)",
        ),
        (
            "caps:video/x-h264",
            "no H.264 decoder — most .mp4 video will not play (install a VA-API, libav or \
             openh264 GStreamer plugin)",
        ),
        (
            "caps:audio/mpeg,mpegversion=4",
            "no AAC decoder — the audio of most .mp4 video will be silent (install a libav \
             or faad GStreamer plugin)",
        ),
        (
            "caps:audio/mpeg,mpegversion=1,layer=3",
            "no MP3 decoder — most radio streams will not play (install the mpg123 GStreamer \
             plugin)",
        ),
    ];
    if ensure_initialized().is_err() {
        return vec![String::from(
            "GStreamer failed to initialise — no video / stream audio",
        )];
    }
    PROBES
        .iter()
        .filter(|(probe, _cost)| !probe_available(probe))
        .map(|(_probe, cost)| String::from(*cost))
        .collect()
}

/// Whether one [`playback_gaps`] probe is satisfied: `uri:<url>` asks for a
/// source URI handler, `element:<name>` for a named element, `caps:<caps>`
/// for any decoder accepting those sink caps.
fn probe_available(probe: &str) -> bool {
    if let Some(uri) = probe.strip_prefix("uri:") {
        return gstreamer::Element::make_from_uri(gstreamer::URIType::Src, uri, None).is_ok();
    }
    if let Some(element) = probe.strip_prefix("element:") {
        return gstreamer::ElementFactory::find(element).is_some();
    }
    if let Some(caps_text) = probe.strip_prefix("caps:") {
        let Ok(caps) = caps_text.parse::<gstreamer::Caps>() else {
            return false;
        };
        let decoders = gstreamer::ElementFactory::factories_with_type(
            gstreamer::ElementFactoryType::DECODER,
            gstreamer::Rank::MARGINAL,
        );
        return decoders
            .iter()
            .any(|factory| factory.can_sink_any_caps(&caps));
    }
    false
}

/// The GStreamer playback engine: creates one playback [`MediaSurface`]
/// per direct video / audio URL and pumps each pipeline's bus once per
/// frame.
///
/// Unlike CEF there is no heavyweight global runtime — each surface is its
/// own `playbin3` pipeline — so the backend is just the surface registry.
#[derive(Debug)]
pub struct GstMediaBackend {
    /// The live surfaces, pruned as they close.
    surfaces: Vec<Weak<surface::SurfaceInner>>,
}

impl GstMediaBackend {
    /// Initialises GStreamer and verifies the core playback element exists.
    ///
    /// # Errors
    /// Returns [`MediaError::Init`] when GStreamer cannot initialise or has
    /// no `playbin3` (a broken / truncated installation).
    pub fn initialize() -> Result<Self, MediaError> {
        ensure_initialized()?;
        if gstreamer::ElementFactory::find("playbin3").is_none() {
            return Err(MediaError::Init(String::from(
                "GStreamer has no playbin3 element (gst-plugins-base missing?)",
            )));
        }
        Ok(Self {
            surfaces: Vec::new(),
        })
    }
}

impl MediaBackend for GstMediaBackend {
    fn create_surface(
        &mut self,
        config: &SurfaceConfig,
    ) -> Result<Box<dyn MediaSurface>, MediaError> {
        let inner = surface::SurfaceInner::create(config)?;
        self.surfaces.push(Arc::downgrade(&inner));
        Ok(Box::new(surface::GstMediaSurface::new(inner)))
    }

    fn pump(&mut self) {
        self.surfaces.retain(|weak| {
            let Some(inner) = weak.upgrade() else {
                return false;
            };
            inner.pump();
            !inner.is_closed()
        });
    }

    fn live_surfaces(&self) -> usize {
        self.surfaces
            .iter()
            .filter(|weak| weak.upgrade().is_some_and(|inner| !inner.is_closed()))
            .count()
    }

    fn shutdown(&mut self) {
        for weak in self.surfaces.drain(..) {
            if let Some(inner) = weak.upgrade() {
                inner.close();
            }
        }
    }
}

impl Drop for GstMediaBackend {
    fn drop(&mut self) {
        self.shutdown();
    }
}

/// A guarded lock helper: takes the mutex, recovering from poisoning (a
/// panicked appsink callback must not wedge the render thread).
pub(crate) fn lock_shared<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    match mutex.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    }
}
