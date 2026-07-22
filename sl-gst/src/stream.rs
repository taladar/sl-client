//! The parcel streaming-audio player (`viewer-streaming-audio`): plays one
//! Shoutcast / Icecast / HLS radio-stream URL at a time, audio only — the
//! counterpart of the reference viewer's `LLStreamingAudio_*` interface,
//! with GStreamer owning the network stack the reference delegates to FMOD.
//!
//! A `playbin3` restricted to its audio path (video / subtitle streams
//! deliberately unselected — a parcel "music" URL pointing at a video should
//! play its soundtrack, not open a window), with ICY metadata arriving as
//! title tags (`icydemux` re-emits the in-band `StreamTitle` updates), so
//! [`AudioStreamStatus::title`] is the live "now playing" line.
//!
//! No trait, no surface: the parcel stream has no frames and no notion of a
//! page. The owner calls [`AudioStreamPlayer::poll`] once per frame to drain
//! the pipeline bus. Audio goes straight to the system device for now — the
//! shared-mixer hand-off is the same interim noted in the crate docs.

use gstreamer::prelude::*;
use tracing::{debug, warn};

use crate::messages::{friendly_error, missing_plugin_description, title_from_tags};

/// Where the stream player currently is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AudioStreamState {
    /// No stream is loaded (or the last one was stopped).
    #[default]
    Stopped,
    /// A stream URL is set and the pipeline is connecting / prerolling.
    Connecting,
    /// Playback is stalled while the network buffer refills.
    Buffering,
    /// The stream is playing.
    Playing,
    /// The stream failed; [`AudioStreamStatus::error`] carries the reason.
    Error,
}

/// A snapshot of the stream player's state for the UI.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct AudioStreamStatus {
    /// The lifecycle state.
    pub state: AudioStreamState,
    /// The URL of the current / last stream, if any was set.
    pub url: Option<String>,
    /// The stream's "now playing" title (ICY metadata), once one arrived.
    pub title: Option<String>,
    /// The failure description while [`state`](Self::state) is
    /// [`Error`](AudioStreamState::Error).
    pub error: Option<String>,
}

/// The one-stream audio player. Construct once, [`play`](Self::play) /
/// [`stop`](Self::stop) as the parcel (or the user) demands, and
/// [`poll`](Self::poll) once per frame.
#[derive(Debug)]
pub struct AudioStreamPlayer {
    /// The audio-only `playbin3`, present while a stream is loaded.
    playbin: Option<gstreamer::Element>,
    /// The pipeline's bus, drained in [`poll`](Self::poll).
    bus: Option<gstreamer::Bus>,
    /// The status snapshot.
    status: AudioStreamStatus,
    /// The volume in `[0, 1]`, applied to the pipeline and re-applied to the
    /// next one.
    volume: f64,
    /// Whether audio is muted (kept across streams, distinct from volume).
    muted: bool,
    /// `missing-plugin` descriptions collected for the current stream.
    missing_plugins: Vec<String>,
}

impl AudioStreamPlayer {
    /// Creates an idle player (initialises GStreamer on first use instead —
    /// construction never fails).
    #[must_use]
    pub const fn new() -> Self {
        Self {
            playbin: None,
            bus: None,
            status: AudioStreamStatus {
                state: AudioStreamState::Stopped,
                url: None,
                title: None,
                error: None,
            },
            volume: 1.0,
            muted: false,
            missing_plugins: Vec::new(),
        }
    }

    /// Starts playing `url`, replacing any current stream.
    pub fn play(&mut self, url: &str) {
        self.stop();
        if let Err(error) = crate::ensure_initialized() {
            self.status.state = AudioStreamState::Error;
            self.status.error = Some(error.to_string());
            return;
        }
        self.status.url = Some(String::from(url));
        self.status.title = None;
        self.status.error = None;
        self.missing_plugins.clear();
        let playbin = match gstreamer::ElementFactory::make("playbin3")
            .property("uri", url)
            .property("mute", self.muted)
            .property("volume", self.volume)
            .build()
        {
            Ok(playbin) => playbin,
            Err(error) => {
                self.status.state = AudioStreamState::Error;
                self.status.error = Some(format!("playbin3: {error}"));
                return;
            }
        };
        // Audio only: deselect the video / subtitle streams so a video URL in
        // the parcel's music field costs no decode and opens no surface.
        playbin.set_property_from_str("flags", "audio+soft-volume+buffering");
        self.bus = playbin.bus();
        if let Err(error) = playbin.set_state(gstreamer::State::Playing) {
            // The detailed reason follows on the bus; poll() surfaces it.
            debug!("audio stream refused to start: {error}");
        }
        self.playbin = Some(playbin);
        self.status.state = AudioStreamState::Connecting;
    }

    /// Stops and tears down the current stream (keeps the URL in the status
    /// so the UI can offer a restart).
    pub fn stop(&mut self) {
        if let Some(playbin) = self.playbin.take() {
            let _result = playbin.set_state(gstreamer::State::Null);
        }
        self.bus = None;
        self.status.state = AudioStreamState::Stopped;
        self.status.title = None;
    }

    /// Sets the stream volume in `[0, 1]`.
    pub fn set_volume(&mut self, volume: f64) {
        self.volume = volume.clamp(0.0, 1.0);
        if let Some(playbin) = &self.playbin {
            playbin.set_property("volume", self.volume);
        }
    }

    /// Mutes or unmutes the stream (retaining the volume level).
    pub fn set_muted(&mut self, muted: bool) {
        self.muted = muted;
        if let Some(playbin) = &self.playbin {
            playbin.set_property("mute", muted);
        }
    }

    /// Whether the stream is muted.
    #[must_use]
    pub const fn muted(&self) -> bool {
        self.muted
    }

    /// The current status snapshot.
    #[must_use]
    pub const fn status(&self) -> &AudioStreamStatus {
        &self.status
    }

    /// Drains the pipeline bus — call once per frame while a stream is
    /// loaded.
    pub fn poll(&mut self) {
        let Some(bus) = self.bus.clone() else { return };
        while let Some(message) = bus.pop() {
            self.apply_message(&message);
        }
    }

    /// Folds one bus message into the status, reacting on the pipeline where
    /// needed (buffering holds, end-of-stream teardown).
    fn apply_message(&mut self, message: &gstreamer::Message) {
        if let Some(description) = missing_plugin_description(message) {
            warn!("audio stream missing plugin: {description}");
            self.missing_plugins.push(description);
            return;
        }
        match message.view() {
            gstreamer::MessageView::Error(error) => {
                let text = friendly_error(error, &self.missing_plugins);
                warn!("audio stream error: {text}");
                if let Some(playbin) = self.playbin.take() {
                    let _result = playbin.set_state(gstreamer::State::Null);
                }
                self.bus = None;
                self.status.state = AudioStreamState::Error;
                self.status.error = Some(text);
            }
            gstreamer::MessageView::Eos(_eos) => {
                // A radio stream ending means the server dropped us; report
                // it as stopped so the UI offers a restart.
                debug!("audio stream ended");
                self.stop();
            }
            gstreamer::MessageView::Buffering(buffering) => {
                let percent = buffering.percent();
                if let Some(playbin) = &self.playbin {
                    if percent < 100 {
                        if self.status.state != AudioStreamState::Buffering {
                            let _result = playbin.set_state(gstreamer::State::Paused);
                            self.status.state = AudioStreamState::Buffering;
                        }
                    } else if self.status.state == AudioStreamState::Buffering {
                        let _result = playbin.set_state(gstreamer::State::Playing);
                    }
                }
            }
            gstreamer::MessageView::Tag(tag) => {
                if let Some(title) = title_from_tags(&tag.tags()) {
                    self.status.title = Some(title);
                }
            }
            gstreamer::MessageView::StateChanged(changed) => {
                let is_playbin = self
                    .playbin
                    .as_ref()
                    .is_some_and(|playbin| message.src() == Some(playbin.upcast_ref()));
                if is_playbin && changed.current() == gstreamer::State::Playing {
                    self.status.state = AudioStreamState::Playing;
                }
            }
            _other => {}
        }
    }
}

impl Default for AudioStreamPlayer {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for AudioStreamPlayer {
    fn drop(&mut self) {
        self.stop();
    }
}
