//! The GStreamer [`MediaSurface`]: one `playbin3` pipeline per direct
//! video / audio URL, its frames converted to tightly packed BGRA by a
//! `videoconvert ! appsink` video sink and handed to the viewer through
//! [`MediaSurface::with_new_frame`] — the playback counterpart of the CEF
//! browser surface.
//!
//! Threading: GStreamer runs its own streaming threads; the `appsink`
//! new-sample callback copies each decoded frame into the [`Shared`] state
//! under a mutex, and everything else (bus messages, position polling)
//! happens inside [`SurfaceInner::pump`] on the caller's thread. Pipeline
//! operations (state changes, seeks) are **never** performed while holding
//! the lock — a flushing seek can block on the very streaming thread that is
//! waiting for the lock in the frame callback.
//!
//! Audio goes straight to the system device for now (`playbin3`'s default
//! `autoaudiosink`) — see the crate docs for the planned mixer hand-off. The
//! browser-only trait methods (history, mouse, keyboard, resize) are
//! deliberate no-ops.

use std::sync::{Arc, Mutex};

use gstreamer::prelude::*;
use gstreamer_video::VideoFrameExt as _;
use sl_media::{
    FrameView, KeyInput, MediaError, MediaSurface, Modifiers, MouseButton, PlaybackState,
    PlaybackStatus, SurfaceConfig, SurfaceStatus,
};
use tracing::{debug, warn};

use crate::lock_shared;
use crate::messages::{friendly_error, missing_plugin_description, title_from_tags};

/// The newest decoded frame, tightly packed BGRA rows.
#[derive(Default)]
struct FrameStore {
    /// Tightly packed BGRA pixel data (`width * height * 4` bytes).
    bgra: Vec<u8>,
    /// Frame width in pixels.
    width: u32,
    /// Frame height in pixels.
    height: u32,
    /// Monotonic frame counter (0 = no frame yet).
    generation: u64,
}

/// State shared between the trait methods, the bus pump and the appsink
/// streaming thread.
#[expect(
    clippy::struct_excessive_bools,
    reason = "playback intent, loop, mute and the buffering hold vary independently — a \
              combined state enum would multiply, not simplify"
)]
struct Shared {
    /// The status snapshot handed to [`MediaSurface::status`].
    status: SurfaceStatus,
    /// The newest decoded frame.
    frame: FrameStore,
    /// Whether the user wants the media playing (survives buffering holds).
    desired_playing: bool,
    /// Whether playback restarts from the start on end-of-stream.
    loop_media: bool,
    /// Whether audio is muted (mirrors the pipeline property).
    muted: bool,
    /// Whether playback is paused waiting for the network buffer.
    buffering_hold: bool,
    /// Human-readable `missing-plugin` descriptions collected so far, folded
    /// into the error text when the pipeline gives up.
    missing_plugins: Vec<String>,
}

impl Shared {
    /// Advances the status generation (call after any status field change).
    const fn touch(&mut self) {
        self.status.generation = self.status.generation.wrapping_add(1);
    }

    /// The mutable playback half of the status (always present on this
    /// surface).
    fn playback(&mut self) -> &mut PlaybackStatus {
        self.status.playback.get_or_insert_default()
    }
}

/// A deferred pipeline operation decided under the lock and executed after
/// releasing it.
enum PipelineAction {
    /// Change the pipeline state.
    SetState(gstreamer::State),
    /// Flush-seek back to the start.
    SeekToStart,
}

/// The engine-side state of one playback surface, shared between the backend
/// (which pumps it) and the [`GstMediaSurface`] trait object.
pub(crate) struct SurfaceInner {
    /// The `playbin3` pipeline.
    playbin: gstreamer::Element,
    /// The pipeline's message bus, drained in [`pump`](Self::pump).
    bus: gstreamer::Bus,
    /// The state shared with the appsink streaming thread.
    shared: Arc<Mutex<Shared>>,
}

impl SurfaceInner {
    /// Builds the pipeline for `config` and starts it playing.
    ///
    /// # Errors
    /// Returns [`MediaError::SurfaceCreation`] when an element cannot be
    /// created (missing base plugins) or the pipeline cannot be assembled.
    pub(crate) fn create(config: &SurfaceConfig) -> Result<Arc<Self>, MediaError> {
        /// Wraps an assembly failure as a surface-creation error.
        const fn creation(error: String) -> MediaError {
            MediaError::SurfaceCreation(error)
        }

        let shared = Arc::new(Mutex::new(Shared {
            status: SurfaceStatus {
                url: config.initial_url.clone(),
                loading: true,
                playback: Some(PlaybackStatus::default()),
                ..SurfaceStatus::default()
            },
            frame: FrameStore::default(),
            desired_playing: true,
            loop_media: config.loop_media,
            muted: config.muted,
            buffering_hold: false,
            missing_plugins: Vec::new(),
        }));

        // The video sink: videoconvert normalises whatever the decoder emits
        // into the tightly specified BGRA the viewer mirrors into textures.
        let caps = gstreamer_video::VideoCapsBuilder::new()
            .format(gstreamer_video::VideoFormat::Bgra)
            .build();
        let appsink = gstreamer_app::AppSink::builder()
            .caps(&caps)
            .max_buffers(1)
            .drop(true)
            .sync(true)
            .build();
        let frame_shared = Arc::clone(&shared);
        appsink.set_callbacks(
            gstreamer_app::AppSinkCallbacks::builder()
                .new_sample(move |sink| {
                    let sample = sink
                        .pull_sample()
                        .map_err(|_gone| gstreamer::FlowError::Eos)?;
                    store_sample(&sample, &frame_shared);
                    Ok(gstreamer::FlowSuccess::Ok)
                })
                .build(),
        );
        let convert = gstreamer::ElementFactory::make("videoconvert")
            .build()
            .map_err(|error| creation(format!("videoconvert: {error}")))?;
        let sink_bin = gstreamer::Bin::builder().name("sl-video-sink").build();
        sink_bin
            .add_many([&convert, appsink.upcast_ref()])
            .map_err(|error| creation(format!("assembling video sink: {error}")))?;
        convert
            .link(&appsink)
            .map_err(|error| creation(format!("linking video sink: {error}")))?;
        let convert_sink = convert
            .static_pad("sink")
            .ok_or_else(|| creation(String::from("videoconvert has no sink pad")))?;
        let ghost = gstreamer::GhostPad::with_target(&convert_sink)
            .map_err(|error| creation(format!("video sink ghost pad: {error}")))?;
        sink_bin
            .add_pad(&ghost)
            .map_err(|error| creation(format!("video sink ghost pad: {error}")))?;

        let playbin = gstreamer::ElementFactory::make("playbin3")
            .property("uri", &config.initial_url)
            .property("video-sink", sink_bin.upcast_ref::<gstreamer::Element>())
            .property("mute", config.muted)
            .build()
            .map_err(|error| creation(format!("playbin3: {error}")))?;
        let bus = playbin
            .bus()
            .ok_or_else(|| creation(String::from("playbin3 has no bus")))?;

        if let Err(error) = playbin.set_state(gstreamer::State::Playing) {
            // A synchronous refusal (e.g. no URI handler) still posts the
            // detailed error on the bus; the pump surfaces it.
            debug!("media pipeline refused to start: {error}");
        }
        Ok(Arc::new(Self {
            playbin,
            bus,
            shared,
        }))
    }

    /// Drains the pipeline bus and refreshes position / duration — one
    /// iteration of engine work, on the caller's thread.
    pub(crate) fn pump(&self) {
        // 1. Collect messages (no lock needed).
        let mut pending = Vec::new();
        while let Some(message) = self.bus.pop() {
            pending.push(message);
        }
        // 2. Queries (no lock, so a blocked streaming thread cannot deadlock
        //    against them).
        let position = self
            .playbin
            .query_position::<gstreamer::ClockTime>()
            .map(gstreamer::ClockTime::seconds_f64);
        let duration = self
            .playbin
            .query_duration::<gstreamer::ClockTime>()
            .map(gstreamer::ClockTime::seconds_f64);

        // 3. Fold into the status under the lock, deciding pipeline actions.
        let mut actions = Vec::new();
        {
            let mut shared = lock_shared(&self.shared);
            for message in &pending {
                self.apply_message(message, &mut shared, &mut actions);
            }
            // Bit-exact compares: these are change detectors for the same
            // computed value, not numeric comparisons.
            if let Some(position) = position
                && shared.playback().position_seconds.to_bits() != position.to_bits()
            {
                shared.playback().position_seconds = position;
                shared.touch();
            }
            if let Some(duration) = duration
                && shared
                    .playback()
                    .duration_seconds
                    .map(f64::to_bits)
                    .is_none_or(|known| known != duration.to_bits())
            {
                shared.playback().duration_seconds = Some(duration);
                shared.touch();
            }
        }
        // 4. Execute the deferred pipeline operations lock-free.
        for action in actions {
            match action {
                PipelineAction::SetState(state) => {
                    let _result = self.playbin.set_state(state);
                }
                PipelineAction::SeekToStart => self.seek_seconds(0.0),
            }
        }
    }

    /// Folds one bus message into the shared status, queueing any pipeline
    /// reaction onto `actions`.
    fn apply_message(
        &self,
        message: &gstreamer::Message,
        shared: &mut Shared,
        actions: &mut Vec<PipelineAction>,
    ) {
        if let Some(description) = missing_plugin_description(message) {
            warn!("media pipeline missing plugin: {description}");
            shared.missing_plugins.push(description);
            return;
        }
        match message.view() {
            gstreamer::MessageView::Error(error) => {
                let text = friendly_error(error, &shared.missing_plugins);
                warn!("media pipeline error: {text}");
                shared.status.load_error = Some(text);
                shared.status.loading = false;
                shared.playback().state = PlaybackState::Error;
                shared.touch();
                actions.push(PipelineAction::SetState(gstreamer::State::Null));
            }
            gstreamer::MessageView::Eos(_eos) => {
                if shared.loop_media {
                    actions.push(PipelineAction::SeekToStart);
                } else {
                    shared.desired_playing = false;
                    shared.playback().state = PlaybackState::Ended;
                    shared.touch();
                }
            }
            gstreamer::MessageView::Buffering(buffering) => {
                let percent = buffering.percent();
                if percent < 100 {
                    if !shared.buffering_hold {
                        shared.buffering_hold = true;
                        if shared.desired_playing {
                            actions.push(PipelineAction::SetState(gstreamer::State::Paused));
                        }
                    }
                    shared.playback().state = PlaybackState::Buffering;
                    shared.playback().buffering_percent =
                        Some(u8::try_from(percent.clamp(0, 100)).unwrap_or(100));
                    shared.status.progress = f64::from(percent.clamp(0, 100)) / 100.0;
                    shared.touch();
                } else if shared.buffering_hold {
                    shared.buffering_hold = false;
                    shared.playback().buffering_percent = None;
                    if shared.desired_playing {
                        actions.push(PipelineAction::SetState(gstreamer::State::Playing));
                    }
                    shared.touch();
                }
            }
            gstreamer::MessageView::Tag(tag) => {
                if let Some(title) = title_from_tags(&tag.tags())
                    && shared.status.title != title
                {
                    shared.status.title = title;
                    shared.touch();
                }
            }
            gstreamer::MessageView::StateChanged(changed) => {
                if message.src() != Some(self.playbin.upcast_ref()) {
                    return;
                }
                match changed.current() {
                    gstreamer::State::Playing => {
                        shared.status.loading = false;
                        shared.status.progress = 1.0;
                        shared.playback().state = PlaybackState::Playing;
                        shared.touch();
                    }
                    gstreamer::State::Paused
                        if !shared.buffering_hold
                            && !shared.desired_playing
                            && shared.playback().state == PlaybackState::Playing =>
                    {
                        shared.playback().state = PlaybackState::Paused;
                        shared.touch();
                    }
                    _other => {}
                }
            }
            gstreamer::MessageView::AsyncDone(_done) => {
                // Preroll finished: the stream's seekability is now known.
                let mut query = gstreamer::query::Seeking::new(gstreamer::Format::Time);
                if self.playbin.query(&mut query) {
                    let (seekable, _start, _end) = query.result();
                    if shared.playback().seekable != seekable {
                        shared.playback().seekable = seekable;
                        shared.touch();
                    }
                }
                if shared.status.loading {
                    shared.status.loading = false;
                    if !shared.desired_playing {
                        shared.playback().state = PlaybackState::Paused;
                    }
                    shared.touch();
                }
            }
            _other => {}
        }
    }

    /// Flush-seeks to `seconds` from the start (clamped non-negative).
    fn seek_seconds(&self, seconds: f64) {
        let Ok(position) = gstreamer::ClockTime::try_from_seconds_f64(seconds.max(0.0)) else {
            return;
        };
        if let Err(error) = self.playbin.seek_simple(
            gstreamer::SeekFlags::FLUSH | gstreamer::SeekFlags::KEY_UNIT,
            position,
        ) {
            debug!("media seek failed: {error}");
        }
    }

    /// Whether the surface finished closing.
    pub(crate) fn is_closed(&self) -> bool {
        lock_shared(&self.shared).status.closed
    }

    /// Tears the pipeline down and marks the surface closed. Idempotent.
    pub(crate) fn close(&self) {
        {
            let shared = lock_shared(&self.shared);
            if shared.status.closed {
                return;
            }
        }
        let _result = self.playbin.set_state(gstreamer::State::Null);
        let mut shared = lock_shared(&self.shared);
        shared.status.closed = true;
        shared.touch();
    }
}

impl Drop for SurfaceInner {
    fn drop(&mut self) {
        self.close();
    }
}

/// Copies one appsink sample into the shared frame store (tight BGRA rows,
/// stride removed). Runs on a GStreamer streaming thread.
#[expect(
    clippy::significant_drop_tightening,
    reason = "the guard is taken as late as possible already; every statement after it fills \
              the guarded frame store"
)]
fn store_sample(sample: &gstreamer::Sample, shared: &Mutex<Shared>) {
    let Some(caps) = sample.caps() else { return };
    let Ok(info) = gstreamer_video::VideoInfo::from_caps(caps) else {
        return;
    };
    let Some(buffer) = sample.buffer() else {
        return;
    };
    let Ok(frame) = gstreamer_video::VideoFrameRef::from_buffer_ref_readable(buffer, &info) else {
        return;
    };
    let width = frame.width();
    let height = frame.height();
    let Some(stride) = frame
        .plane_stride()
        .first()
        .copied()
        .and_then(|stride| usize::try_from(stride).ok())
        .filter(|stride| *stride > 0)
    else {
        return;
    };
    let Ok(data) = frame.plane_data(0) else {
        return;
    };
    let Some(row_bytes) = usize::try_from(width)
        .ok()
        .and_then(|pixels| pixels.checked_mul(4))
    else {
        return;
    };
    let Ok(rows) = usize::try_from(height) else {
        return;
    };
    let Some(total) = row_bytes.checked_mul(rows) else {
        return;
    };
    let mut shared = lock_shared(shared);
    let store = &mut shared.frame;
    store.bgra.clear();
    store.bgra.reserve(total);
    for row in data.chunks(stride).take(rows) {
        let Some(pixels) = row.get(..row_bytes) else {
            return;
        };
        store.bgra.extend_from_slice(pixels);
    }
    if store.bgra.len() != total {
        return;
    }
    store.width = width;
    store.height = height;
    store.generation = store.generation.wrapping_add(1);
}

/// The [`MediaSurface`] trait object handed to the viewer: a thin shell over
/// the shared [`SurfaceInner`].
pub(crate) struct GstMediaSurface {
    /// The engine-side state (also held weakly by the backend for pumping).
    inner: Arc<SurfaceInner>,
}

impl GstMediaSurface {
    /// Wraps the shared inner state.
    pub(crate) const fn new(inner: Arc<SurfaceInner>) -> Self {
        Self { inner }
    }
}

impl MediaSurface for GstMediaSurface {
    fn navigate(&self, url: &str) {
        // A new URL: rebuild playback state and reconnect the pipeline.
        {
            let mut shared = lock_shared(&self.inner.shared);
            shared.status.url = String::from(url);
            shared.status.title.clear();
            shared.status.load_error = None;
            shared.status.loading = true;
            shared.status.progress = 0.0;
            shared.missing_plugins.clear();
            shared.desired_playing = true;
            shared.buffering_hold = false;
            *shared.playback() = PlaybackStatus::default();
            shared.touch();
        }
        let _stopped = self.inner.playbin.set_state(gstreamer::State::Null);
        self.inner.playbin.set_property("uri", url);
        let _started = self.inner.playbin.set_state(gstreamer::State::Playing);
    }

    fn reload(&self) {
        let (seekable, url) = {
            let mut shared = lock_shared(&self.inner.shared);
            (shared.playback().seekable, shared.status.url.clone())
        };
        if seekable {
            self.inner.seek_seconds(0.0);
            self.play();
        } else {
            // A live stream restarts by reconnecting.
            self.navigate(&url);
        }
    }

    fn stop(&self) {
        self.pause();
    }

    fn go_back(&self) {}

    fn go_forward(&self) {}

    fn resize(&self, _width: u32, _height: u32) {
        // Playback surfaces keep the media's own frame size.
    }

    fn set_focus(&self, _focused: bool) {}

    fn mouse_move(&self, _x: i32, _y: i32, _modifiers: Modifiers) {}

    fn mouse_leave(&self) {}

    fn mouse_button(
        &self,
        _x: i32,
        _y: i32,
        _button: MouseButton,
        _down: bool,
        _click_count: u8,
        _modifiers: Modifiers,
    ) {
    }

    fn mouse_wheel(&self, _x: i32, _y: i32, _delta_x: i32, _delta_y: i32) {}

    fn key(&self, _input: KeyInput) {}

    fn insert_text(&self, _text: &str) {}

    fn set_max_fps(&self, _fps: u8) {
        // The paint rate is the media's frame rate; the interest throttle
        // could drop frames here later, but audio must keep running either
        // way, so there is nothing cheap to save yet.
    }

    fn set_muted(&self, muted: bool) {
        self.inner.playbin.set_property("mute", muted);
        let mut shared = lock_shared(&self.inner.shared);
        shared.muted = muted;
    }

    fn muted(&self) -> bool {
        lock_shared(&self.inner.shared).muted
    }

    fn play(&self) {
        let restart = {
            let mut shared = lock_shared(&self.inner.shared);
            shared.desired_playing = true;
            shared.playback().state == PlaybackState::Ended && shared.playback().seekable
        };
        if restart {
            self.inner.seek_seconds(0.0);
        }
        let _result = self.inner.playbin.set_state(gstreamer::State::Playing);
    }

    fn pause(&self) {
        {
            let mut shared = lock_shared(&self.inner.shared);
            shared.desired_playing = false;
        }
        let _result = self.inner.playbin.set_state(gstreamer::State::Paused);
    }

    fn seek(&self, seconds: f64) {
        let seekable = {
            let mut shared = lock_shared(&self.inner.shared);
            let seekable = shared.playback().seekable;
            if seekable {
                // Reflect the target position immediately so a scrubber does
                // not snap back while the flush completes.
                shared.playback().position_seconds = seconds.max(0.0);
                shared.touch();
            }
            seekable
        };
        if seekable {
            self.inner.seek_seconds(seconds);
        }
    }

    fn set_volume(&self, volume: f64) {
        self.inner
            .playbin
            .set_property("volume", volume.clamp(0.0, 1.0));
    }

    #[expect(
        clippy::significant_drop_tightening,
        reason = "the consumer borrows the guarded frame buffer, so the guard must span the \
                  whole call by design"
    )]
    fn with_new_frame(
        &self,
        seen_generation: &mut u64,
        consumer: &mut dyn FnMut(FrameView<'_>),
    ) -> bool {
        let shared = lock_shared(&self.inner.shared);
        let store = &shared.frame;
        if store.generation == 0 || store.generation == *seen_generation {
            return false;
        }
        *seen_generation = store.generation;
        consumer(FrameView {
            bgra: &store.bgra,
            width: store.width,
            height: store.height,
        });
        true
    }

    fn status(&self) -> SurfaceStatus {
        lock_shared(&self.inner.shared).status.clone()
    }

    fn take_popup_request(&self) -> Option<String> {
        None
    }

    fn request_close(&self) {
        self.inner.close();
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use gstreamer::prelude::*;
    use pretty_assertions::assert_eq;
    use sl_media::{MediaSurface as _, PlaybackState, SurfaceConfig};

    use super::{FrameStore, Shared, SurfaceInner, store_sample};
    use crate::lock_shared;

    /// Whether GStreamer is usable on this machine; tests skip (pass
    /// vacuously) where it is not, so `cargo test` works on hosts without a
    /// GStreamer installation.
    fn gstreamer_available() -> bool {
        crate::ensure_initialized().is_ok()
    }

    /// A [`Shared`] with default status, for feeding [`store_sample`].
    fn empty_shared() -> Arc<Mutex<Shared>> {
        Arc::new(Mutex::new(Shared {
            status: sl_media::SurfaceStatus::default(),
            frame: FrameStore::default(),
            desired_playing: true,
            loop_media: false,
            muted: false,
            buffering_hold: false,
            missing_plugins: Vec::new(),
        }))
    }

    /// `videotestsrc` through our BGRA appsink caps: the copied frame is
    /// tight BGRA at the negotiated size — exercising the stride-removal
    /// copy against a real videoconvert output.
    #[test]
    #[expect(
        clippy::print_stderr,
        reason = "a visible skip notice when GStreamer is absent on the host"
    )]
    fn store_sample_copies_tight_bgra() -> Result<(), String> {
        if !gstreamer_available() {
            eprintln!("skipping: no usable GStreamer");
            return Ok(());
        }
        let pipeline = match gstreamer::parse::launch(
            "videotestsrc num-buffers=2 ! video/x-raw,width=97,height=41 ! videoconvert \
             ! appsink name=sink caps=video/x-raw,format=BGRA",
        ) {
            Ok(pipeline) => pipeline,
            Err(error) => {
                eprintln!("skipping: videotestsrc pipeline unavailable ({error})");
                return Ok(());
            }
        };
        let bin = pipeline
            .clone()
            .downcast::<gstreamer::Bin>()
            .map_err(|_element| String::from("parsed pipeline is not a bin"))?;
        let sink = bin
            .by_name("sink")
            .and_then(|sink| sink.downcast::<gstreamer_app::AppSink>().ok())
            .ok_or_else(|| String::from("no appsink in test pipeline"))?;
        pipeline
            .set_state(gstreamer::State::Playing)
            .map_err(|error| format!("test pipeline did not start: {error}"))?;
        let sample = sink
            .pull_sample()
            .map_err(|error| format!("no sample from videotestsrc: {error}"))?;
        let shared = empty_shared();
        store_sample(&sample, &shared);
        let _stopped = pipeline.set_state(gstreamer::State::Null);
        let guard = lock_shared(&shared);
        assert_eq!(guard.frame.width, 97);
        assert_eq!(guard.frame.height, 41);
        // 97 px * 4 bytes is not a multiple of common alignments, so a
        // stride-blind copy would differ in length here.
        assert_eq!(guard.frame.bgra.len(), 97 * 41 * 4);
        assert_eq!(guard.frame.generation, 1);
        Ok(())
    }

    /// A surface pointed at a nonexistent file reports a loud error status
    /// through the pump (the bus error path), not a silent black square.
    #[test]
    #[expect(
        clippy::print_stderr,
        reason = "a visible skip notice when GStreamer is absent on the host"
    )]
    fn missing_file_surfaces_error_status() -> Result<(), String> {
        if !gstreamer_available() {
            eprintln!("skipping: no usable GStreamer");
            return Ok(());
        }
        let inner = match SurfaceInner::create(&SurfaceConfig {
            initial_url: String::from("file:///nonexistent/sl-gst-test.mp4"),
            ..SurfaceConfig::default()
        }) {
            Ok(inner) => inner,
            Err(error) => {
                eprintln!("skipping: surface creation unavailable ({error})");
                return Ok(());
            }
        };
        let surface = super::GstMediaSurface::new(Arc::clone(&inner));
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        loop {
            inner.pump();
            let status = surface.status();
            if let Some(playback) = &status.playback
                && playback.state == PlaybackState::Error
            {
                if status.load_error.is_none() {
                    return Err(String::from("error state must carry a description"));
                }
                break;
            }
            if std::time::Instant::now() >= deadline {
                return Err(String::from(
                    "no error status within 10s for a nonexistent file",
                ));
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        surface.request_close();
        if !inner.is_closed() {
            return Err(String::from("surface did not report closed"));
        }
        Ok(())
    }
}
