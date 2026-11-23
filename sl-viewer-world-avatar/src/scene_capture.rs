//! The **region timeline** (viewer-perf-region-capture-replay): a recording of
//! what shaped the scene and when it reached the viewer, for comparing rendering
//! performance on identical input.
//!
//! With `SL_VIEWER_SCENE_CAPTURE_DIR` set, a session writes
//! `<dir>/<unix-time>.scenecapture/timeline.jsonl.gz`: a version header, then one
//! [`TimelineRecord`] per line, each stamped with the frame and the time since
//! recording began. A record is either a [`SceneEvent`], the render-relevant part
//! of the session event stream, or an [`AssetArrival`], a texture or mesh finishing
//! its decode. Keeping both in one ordered stream is what lets a replay reproduce
//! a burst, which a snapshot of the final state cannot.
//!
//! **Ctrl+Alt+Shift+B**, or `SL_VIEWER_SCENE_CAPTURE_SECONDS` after the start for
//! an unattended run, ends the recording and bundles what it references into the
//! same directory, on a background thread: `cache/` in the drop-in layout of an
//! avatar bundle, `materials.json` with the legacy materials no reply had carried,
//! and `asset-report.json` listing anything that could not be obtained.
//!
//! This has to happen **before logging out**, because a region's capabilities end
//! with the session. A viewer closed without it still writes a report, from its
//! local caches only, and that report lists every texture as missing: a cached
//! texture may hold only the low detail levels, which would falsify a benchmark.
//! A recording without a report is not complete.
//!
//! A bundle that came out incomplete can be **resumed**: a viewer started with
//! `SL_VIEWER_SCENE_CAPTURE_RESUME=<recording>` records nothing, and as soon as it
//! has capabilities fetches only what that recording's report lists as missing,
//! then rewrites the report. Assets are fetched by id, so this works from a later
//! session; legacy materials are asked of the region the viewer is then in.
//!
//! `--replay <recording>` then redraws the scene offline: [`load_scene`] folds the
//! timeline into its **final state**, which the normal render systems receive as
//! if a session had just delivered it. Arrival timing is not reproduced yet.
//!
//! Serialisation and compression run on a thread of their own, so the viewer's
//! frame pays only for cloning the events. A gzip stream ends in a checksum, so a
//! recording cut short by a crash is reported as such when it is read.
//!
//! **Recordings are strictly local and NEVER committed or shared**: they carry
//! other residents' objects and appearance.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::hash::Hash;
use std::io::{BufRead as _, BufReader, BufWriter, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread::JoinHandle;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use bevy::prelude::*;
use flate2::Compression;
use flate2::read::GzDecoder;
use flate2::write::GzEncoder;
use serde::{Deserialize, Serialize};
use sl_client_bevy::{
    AgentKey, AvatarAppearance, CircuitId, EnvironmentSettings, MAX_FACES, Object, ObjectKey,
    ObjectPlayingAnimation, PlayingAnimation, RegionHandle, RegionIdentity, RegionLimits,
    RenderMaterialEntry, ScopedObjectId, SkySettings, SlCapabilities, SlEvent, SlIdentity,
    SlSessionEvent, TerrainPatch, TextureOverride, Uuid, WaterSettings, decode_texture_entry,
    parse_material_override,
};
use sl_viewer_intents::LocalChatNotice;
use sl_viewer_platform::paths::ViewerPaths;
use sl_viewer_ui_core::i18n::{TransArgs, Translator};
use sl_viewer_world_objects::meshes::MeshDecoded;
use sl_viewer_world_objects::objects::plant_texture;
use sl_viewer_world_objects::textures::TextureDecoded;

use crate::replay_bundle::{
    BundleGaps, CaptureCaps, FetchPlan, MANIFEST_VERSION, ReferencedAssets, ReplayManifest,
    RetryPolicy, bake_fetch_urls, collect_object_assets, copy_referenced, insert_non_nil,
    referenced_assets, run_bundle_fetch, run_render_material_fetch, textures_not_bundled,
};

/// The environment variable that arms the recorder: the directory a recording is
/// created in. Unset, [`SceneCapturePlugin`] adds nothing at all.
pub const SCENE_CAPTURE_DIR_ENV: &str = "SL_VIEWER_SCENE_CAPTURE_DIR";

/// The environment variable that ends the recording and starts the bundle this
/// many seconds after the viewer starts, for a run nobody is at the keyboard for.
pub const SCENE_CAPTURE_SECONDS_ENV: &str = "SL_VIEWER_SCENE_CAPTURE_SECONDS";

/// The environment variable naming a recording whose missing assets to fetch,
/// instead of making a new recording.
pub const SCENE_CAPTURE_RESUME_ENV: &str = "SL_VIEWER_SCENE_CAPTURE_RESUME";

/// The timeline schema version, bumped when a record's shape changes
/// incompatibly so a reader can refuse a recording it cannot interpret.
pub const TIMELINE_VERSION: u32 = 1;

/// The timeline's file name inside a recording directory.
pub const TIMELINE_FILE: &str = "timeline.jsonl.gz";

/// The legacy materials a bundle fetched, inside a recording directory.
pub const MATERIALS_FILE: &str = "materials.json";

/// The report of what a bundle holds, inside a recording directory.
pub const ASSET_REPORT_FILE: &str = "asset-report.json";

/// How long closing the viewer waits for a cancelled bundle to write its report.
const EXIT_WAIT: Duration = Duration::from_secs(5);

/// The extension of a recording directory, which `.gitignore` excludes.
const RECORDING_EXTENSION: &str = "scenecapture";

/// The first line of a timeline.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
struct TimelineHeader {
    /// The schema version ([`TIMELINE_VERSION`]).
    version: u32,
}

/// A session event that shapes what the viewer draws, in serialisable form.
///
/// [`SlSessionEvent`] itself is not serialisable and mostly concerns other things
/// (chat, inventory, economy). Media, sound and name events are left out: replay
/// runs with the media engines off, and none of them changes the rendered scene.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum SceneEvent {
    /// An object entered the scene.
    ObjectAdded(Box<Object>),
    /// A known object changed.
    ObjectUpdated(Box<Object>),
    /// An object left the scene.
    ObjectRemoved {
        /// The region the object was in.
        region_handle: RegionHandle,
        /// The object's scoped region-local id.
        local_id: ScopedObjectId,
    },
    /// A terrain, wind or cloud patch arrived.
    TerrainPatch(Box<TerrainPatch>),
    /// The region's identity, including its terrain composition and water height.
    RegionInfoHandshake(Box<RegionIdentity>),
    /// The region handshake finished.
    RegionHandshakeComplete,
    /// The region's limits arrived.
    RegionLimits(RegionLimits),
    /// The region's or parcel's environment arrived.
    Environment(Box<EnvironmentSettings>),
    /// The region's time dilation changed, which scales interpolated motion.
    TimeDilation {
        /// The region whose time dilation this is.
        region_handle: RegionHandle,
        /// The time dilation, `0.0..=1.0`.
        dilation: f32,
    },
    /// An avatar's appearance arrived.
    AvatarAppearance(Box<AvatarAppearance>),
    /// An avatar's set of playing animations changed.
    AvatarAnimation {
        /// The avatar the animations belong to.
        avatar_id: AgentKey,
        /// The animations it is playing.
        animations: Vec<PlayingAnimation>,
    },
    /// An animated object's set of playing animations changed.
    ObjectAnimation {
        /// The animated object.
        object_id: ObjectKey,
        /// The animations it is playing.
        animations: Vec<ObjectPlayingAnimation>,
    },
    /// Legacy materials arrived.
    RenderMaterials(Vec<RenderMaterialEntry>),
    /// A PBR material override arrived for an object's faces.
    GltfMaterialOverride {
        /// The region the override applies in.
        region_handle: RegionHandle,
        /// The overridden object's scoped region-local id.
        local_id: ScopedObjectId,
        /// The face indices carrying an override, in order.
        faces: Vec<u8>,
        /// The raw per-face override LLSD, one per entry of `faces`.
        overrides: Vec<Vec<u8>>,
    },
}

impl SceneEvent {
    /// The recordable form of `event`, or `None` if it does not shape the scene.
    #[must_use]
    pub fn capture(event: &SlSessionEvent) -> Option<Self> {
        Some(match event {
            SlSessionEvent::ObjectAdded(object) => Self::ObjectAdded(object.clone()),
            SlSessionEvent::ObjectUpdated(object) => Self::ObjectUpdated(object.clone()),
            SlSessionEvent::ObjectRemoved {
                region_handle,
                local_id,
            } => Self::ObjectRemoved {
                region_handle: *region_handle,
                local_id: *local_id,
            },
            SlSessionEvent::TerrainPatch(patch) => Self::TerrainPatch(patch.clone()),
            SlSessionEvent::RegionInfoHandshake(identity) => {
                Self::RegionInfoHandshake(identity.clone())
            }
            SlSessionEvent::RegionHandshakeComplete => Self::RegionHandshakeComplete,
            SlSessionEvent::RegionLimits(limits) => Self::RegionLimits(limits.clone()),
            SlSessionEvent::Environment(settings) => Self::Environment(settings.clone()),
            SlSessionEvent::TimeDilation {
                region_handle,
                dilation,
            } => Self::TimeDilation {
                region_handle: *region_handle,
                dilation: *dilation,
            },
            SlSessionEvent::AvatarAppearance(appearance) => {
                Self::AvatarAppearance(appearance.clone())
            }
            SlSessionEvent::AvatarAnimation {
                avatar_id,
                animations,
                ..
            } => Self::AvatarAnimation {
                avatar_id: *avatar_id,
                animations: animations.clone(),
            },
            SlSessionEvent::ObjectAnimation {
                object_id,
                animations,
            } => Self::ObjectAnimation {
                object_id: *object_id,
                animations: animations.clone(),
            },
            SlSessionEvent::RenderMaterials(entries) => Self::RenderMaterials(entries.clone()),
            SlSessionEvent::GltfMaterialOverride {
                region_handle,
                local_id,
                faces,
                overrides,
            } => Self::GltfMaterialOverride {
                region_handle: *region_handle,
                local_id: *local_id,
                faces: faces.clone(),
                overrides: overrides.clone(),
            },
            _other => return None,
        })
    }

    /// The session event a replay re-emits for this record.
    #[must_use]
    pub fn into_session_event(self) -> SlSessionEvent {
        match self {
            Self::ObjectAdded(object) => SlSessionEvent::ObjectAdded(object),
            Self::ObjectUpdated(object) => SlSessionEvent::ObjectUpdated(object),
            Self::ObjectRemoved {
                region_handle,
                local_id,
            } => SlSessionEvent::ObjectRemoved {
                region_handle,
                local_id,
            },
            Self::TerrainPatch(patch) => SlSessionEvent::TerrainPatch(patch),
            Self::RegionInfoHandshake(identity) => SlSessionEvent::RegionInfoHandshake(identity),
            Self::RegionHandshakeComplete => SlSessionEvent::RegionHandshakeComplete,
            Self::RegionLimits(limits) => SlSessionEvent::RegionLimits(limits),
            Self::Environment(settings) => SlSessionEvent::Environment(settings),
            Self::TimeDilation {
                region_handle,
                dilation,
            } => SlSessionEvent::TimeDilation {
                region_handle,
                dilation,
            },
            Self::AvatarAppearance(appearance) => SlSessionEvent::AvatarAppearance(appearance),
            Self::AvatarAnimation {
                avatar_id,
                animations,
            } => SlSessionEvent::AvatarAnimation {
                avatar_id,
                animations,
                physical_events: Vec::new(),
            },
            Self::ObjectAnimation {
                object_id,
                animations,
            } => SlSessionEvent::ObjectAnimation {
                object_id,
                animations,
            },
            Self::RenderMaterials(entries) => SlSessionEvent::RenderMaterials(entries),
            Self::GltfMaterialOverride {
                region_handle,
                local_id,
                faces,
                overrides,
            } => SlSessionEvent::GltfMaterialOverride {
                region_handle,
                local_id,
                faces,
                overrides,
            },
        }
    }
}

/// An asset that finished decoding, which is the moment it can affect a frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AssetArrival {
    /// A texture finished decoding.
    Texture(Uuid),
    /// A mesh finished decoding.
    Mesh(Uuid),
}

/// What one timeline record holds.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum TimelineEntry {
    /// A session event.
    Scene(Box<SceneEvent>),
    /// An asset arrival.
    Asset(AssetArrival),
}

/// One entry of a timeline, with when it reached the viewer.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TimelineRecord {
    /// The frame it was seen on, counted from the start of the recording.
    pub frame: u64,
    /// Microseconds since the start of the recording.
    pub micros: u64,
    /// What arrived.
    pub entry: TimelineEntry,
}

/// Serialise `value` as one line of `writer`.
fn write_line<T: Serialize>(writer: &mut impl Write, value: &T) -> Result<(), String> {
    serde_json::to_writer(&mut *writer, value)
        .map_err(|error| format!("serialize a timeline line: {error}"))?;
    writer
        .write_all(b"\n")
        .map_err(|error| format!("write a timeline line: {error}"))
}

/// The writer thread: everything from `records` into `file`, until the sender is
/// dropped. Returns the number of records written.
fn write_timeline(file: fs_err::File, records: &Receiver<TimelineRecord>) -> Result<u64, String> {
    let mut encoder = GzEncoder::new(BufWriter::new(file), Compression::fast());
    write_line(
        &mut encoder,
        &TimelineHeader {
            version: TIMELINE_VERSION,
        },
    )?;
    let mut written = 0_u64;
    for record in records {
        write_line(&mut encoder, &record)?;
        written = written.saturating_add(1);
    }
    let mut buffered = encoder
        .finish()
        .map_err(|error| format!("finish the timeline: {error}"))?;
    buffered
        .flush()
        .map_err(|error| format!("flush the timeline: {error}"))?;
    Ok(written)
}

/// Read a whole timeline.
///
/// # Errors
///
/// Returns a description if the file cannot be read, is of another schema
/// version, holds a malformed line, or was cut short.
pub fn read_timeline(path: &Path) -> Result<Vec<TimelineRecord>, String> {
    let file = fs_err::File::open(path).map_err(|error| error.to_string())?;
    let mut lines = BufReader::new(GzDecoder::new(file)).lines();
    let first = lines
        .next()
        .ok_or_else(|| format!("{} is empty", path.display()))?
        .map_err(|error| format!("{}: cut short or corrupt: {error}", path.display()))?;
    let header: TimelineHeader = serde_json::from_str(&first)
        .map_err(|error| format!("{}: bad header: {error}", path.display()))?;
    if header.version != TIMELINE_VERSION {
        return Err(format!(
            "{}: timeline version {} (this viewer reads {TIMELINE_VERSION})",
            path.display(),
            header.version,
        ));
    }
    let mut records = Vec::new();
    for line in lines {
        let line = line.map_err(|error| {
            format!(
                "{}: cut short or corrupt after {} record(s): {error}",
                path.display(),
                records.len(),
            )
        })?;
        let record = serde_json::from_str(&line).map_err(|error| {
            format!(
                "{}: bad record after {} record(s): {error}",
                path.display(),
                records.len(),
            )
        })?;
        records.push(record);
    }
    Ok(records)
}

/// A timeline being written.
#[derive(Debug)]
struct TimelineWriter {
    /// The queue to the writer thread; `None` once finished.
    sender: Option<Sender<TimelineRecord>>,
    /// The writer thread; `None` once finished.
    thread: Option<JoinHandle<Result<u64, String>>>,
    /// Where the timeline is written, for the closing log line.
    path: PathBuf,
}

impl TimelineWriter {
    /// Start a timeline at `path`, which must not exist yet.
    fn create(path: &Path) -> Result<Self, String> {
        let file = fs_err::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)
            .map_err(|error| error.to_string())?;
        let (sender, records) = mpsc::channel();
        let thread =
            sl_client_bevy::log_context::spawn_thread(move || write_timeline(file, &records));
        Ok(Self {
            sender: Some(sender),
            thread: Some(thread),
            path: path.to_path_buf(),
        })
    }

    /// Queue `record`. A failed writer thread has dropped the queue, so this
    /// finishes the timeline, which reports why.
    fn push(&mut self, record: TimelineRecord) {
        let Some(sender) = &self.sender else {
            return;
        };
        if sender.send(record).is_err() {
            self.finish();
        }
    }

    /// Close the queue, wait for the file to be complete and report the outcome.
    /// Does nothing the second time.
    fn finish(&mut self) {
        drop(self.sender.take());
        let Some(thread) = self.thread.take() else {
            return;
        };
        match thread.join() {
            Ok(Ok(written)) => {
                info!(
                    "scene capture: wrote {written} record(s) to {}",
                    self.path.display()
                );
            }
            Ok(Err(error)) => {
                error!(
                    "scene capture: {} is incomplete: {error}",
                    self.path.display()
                );
            }
            Err(_panic) => {
                error!(
                    "scene capture: the writer of {} panicked",
                    self.path.display()
                );
            }
        }
    }
}

impl Drop for TimelineWriter {
    fn drop(&mut self) {
        self.finish();
    }
}

/// The recording in progress.
#[derive(Debug, Resource)]
struct SceneRecorder {
    /// The timeline being written.
    writer: TimelineWriter,
    /// When recording began.
    started: Instant,
    /// The current frame, counted from the start of the recording.
    frame: u64,
    /// The recording directory.
    recording: PathBuf,
    /// The region's capability URLs, which the bundle fetches from.
    caps: CaptureCaps,
    /// The bundling thread, once it has been started.
    bundle: Option<JoinHandle<BundleOutcome>>,
    /// Whether the bundle has been started, which stays true after it ended.
    bundle_started: bool,
    /// Set to make a running bundle give up on what it has not fetched yet.
    cancel: Arc<AtomicBool>,
    /// How long after the start to bundle unprompted, if at all.
    bundle_after: Option<Duration>,
}

impl SceneRecorder {
    /// Stamp `entry` with the current frame and time, and queue it.
    fn record(&mut self, entry: TimelineEntry) {
        let micros = u64::try_from(self.started.elapsed().as_micros()).unwrap_or(u64::MAX);
        self.writer.push(TimelineRecord {
            frame: self.frame,
            micros,
            entry,
        });
    }
}

/// Records the region timeline while `SL_VIEWER_SCENE_CAPTURE_DIR` is set, and
/// adds nothing otherwise, so a normal session pays nothing.
#[derive(Debug, Default, Clone, Copy)]
pub struct SceneCapturePlugin;

impl Plugin for SceneCapturePlugin {
    fn build(&self, app: &mut App) {
        if let Some(recording) = std::env::var_os(SCENE_CAPTURE_RESUME_ENV) {
            if std::env::var_os(SCENE_CAPTURE_DIR_ENV).is_some() {
                warn!(
                    "scene capture: {SCENE_CAPTURE_RESUME_ENV} is set, so \
                     {SCENE_CAPTURE_DIR_ENV} is ignored and nothing new is recorded"
                );
            }
            info!("scene capture: will resume the bundle of {recording:?}");
            app.insert_resource(SceneResume {
                recording: PathBuf::from(recording),
                caps: CaptureCaps::default(),
                thread: None,
                started: false,
                cancel: Arc::default(),
            })
            .add_systems(
                Update,
                (resume_when_capabilities_arrive, announce_finished_bundle),
            )
            .add_systems(Last, finish_resume_on_exit);
            return;
        }
        let Some(dir) = std::env::var_os(SCENE_CAPTURE_DIR_ENV) else {
            return;
        };
        match start_recording(Path::new(&dir)) {
            Ok(recorder) => {
                info!(
                    "scene capture: recording to {}",
                    recorder.writer.path.display()
                );
                app.insert_resource(recorder)
                    .add_systems(
                        Update,
                        (bundle_recording_when_asked, announce_finished_bundle),
                    )
                    .add_systems(PostUpdate, record_scene_timeline)
                    .add_systems(Last, finish_recording_on_exit);
            }
            Err(error) => {
                error!("scene capture: cannot record into {dir:?}: {error}");
            }
        }
    }
}

/// Create a new recording directory under `dir` and start its timeline.
fn start_recording(dir: &Path) -> Result<SceneRecorder, String> {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_secs());
    let recording = dir.join(format!("{stamp}.{RECORDING_EXTENSION}"));
    fs_err::create_dir_all(&recording).map_err(|error| error.to_string())?;
    let writer = TimelineWriter::create(&recording.join(TIMELINE_FILE))?;
    Ok(SceneRecorder {
        writer,
        started: Instant::now(),
        frame: 0,
        recording,
        caps: CaptureCaps::default(),
        bundle: None,
        bundle_started: false,
        cancel: Arc::default(),
        bundle_after: bundle_delay(),
    })
}

/// The unattended bundle delay from [`SCENE_CAPTURE_SECONDS_ENV`]. A value that is
/// not a whole number of seconds is reported and ignored.
fn bundle_delay() -> Option<Duration> {
    let value = std::env::var(SCENE_CAPTURE_SECONDS_ENV).ok()?;
    match value.parse() {
        Ok(seconds) => Some(Duration::from_secs(seconds)),
        Err(error) => {
            error!("scene capture: {SCENE_CAPTURE_SECONDS_ENV}={value:?}: {error}");
            None
        }
    }
}

/// Append this frame's scene events and asset arrivals to the timeline. In
/// `PostUpdate`, so everything `Update` produced carries this frame's number.
fn record_scene_timeline(
    mut recorder: ResMut<SceneRecorder>,
    mut events: MessageReader<SlEvent>,
    mut textures: MessageReader<TextureDecoded>,
    mut meshes: MessageReader<MeshDecoded>,
    mut capabilities: MessageReader<SlCapabilities>,
) {
    for SlCapabilities(map) in capabilities.read() {
        recorder.caps.update(map);
    }
    for event in events.read() {
        if let Some(scene) = SceneEvent::capture(&event.0) {
            recorder.record(TimelineEntry::Scene(Box::new(scene)));
        }
    }
    for TextureDecoded(key) in textures.read() {
        recorder.record(TimelineEntry::Asset(AssetArrival::Texture(key.uuid())));
    }
    for MeshDecoded(key) in meshes.read() {
        recorder.record(TimelineEntry::Asset(AssetArrival::Mesh(key.uuid())));
    }
    recorder.frame = recorder.frame.saturating_add(1);
}

/// Bundle the recording on **Ctrl+Alt+Shift+B**, or when the unattended delay
/// has passed.
fn bundle_recording_when_asked(
    keyboard: Option<Res<ButtonInput<KeyCode>>>,
    mut recorder: ResMut<SceneRecorder>,
    identity: Res<SlIdentity>,
    paths: Option<Res<ViewerPaths>>,
) {
    let pressed = keyboard.is_some_and(|keyboard| {
        keyboard.any_pressed([KeyCode::ControlLeft, KeyCode::ControlRight])
            && keyboard.any_pressed([KeyCode::AltLeft, KeyCode::AltRight])
            && keyboard.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight])
            && keyboard.just_pressed(KeyCode::KeyB)
    });
    let due = recorder
        .bundle_after
        .is_some_and(|delay| recorder.started.elapsed() >= delay);
    if pressed || due {
        start_bundle(
            &mut recorder,
            &identity,
            paths.as_deref(),
            SessionState::Live,
        );
    }
}

/// Whether the session's capabilities can still be used.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SessionState {
    /// Logged in, so assets can be fetched.
    Live,
    /// Logged out, so only the local caches are left.
    LoggedOut,
}

/// End the recording and bundle its assets on a background thread. Does nothing
/// the second time.
fn start_bundle(
    recorder: &mut SceneRecorder,
    identity: &SlIdentity,
    paths: Option<&ViewerPaths>,
    session: SessionState,
) {
    if recorder.bundle_started {
        return;
    }
    recorder.writer.finish();
    let live = session == SessionState::Live;
    let job = BundleJob {
        recording: recorder.recording.clone(),
        cancel: Arc::clone(&recorder.cancel),
        live: paths.cloned(),
        caps: if live {
            recorder.caps.clone()
        } else {
            CaptureCaps::default()
        },
        appearance_service: identity
            .agent_appearance_service
            .as_ref()
            .filter(|_service| live)
            .map(ToString::to_string),
        now_unix: unix_now(),
    };
    if live {
        info!(
            "scene capture: recording ended, bundling its assets into {}",
            job.recording.display()
        );
    } else {
        warn!(
            "scene capture: the session has ended, so {} can hold only cached assets; press \
             Ctrl+Alt+Shift+B before logging out for a complete recording",
            job.recording.display()
        );
    }
    recorder.bundle_started = true;
    recorder.bundle = Some(sl_client_bevy::log_context::spawn_thread(move || {
        conclude(bundle_recording(&job), &job.recording)
    }));
}

/// Complete the recording when the viewer exits, so that closing the window
/// always leaves a readable timeline and a report, and wait for a bundle still
/// running.
fn finish_recording_on_exit(
    mut exits: MessageReader<AppExit>,
    mut recorder: ResMut<SceneRecorder>,
    identity: Res<SlIdentity>,
    paths: Option<Res<ViewerPaths>>,
) {
    if exits.read().next().is_none() {
        return;
    }
    // A bundle already running is fetching over capabilities that ended with the
    // logout, so it is told to stop. One started here has no network to wait for.
    if let Some(bundle) = recorder.bundle.take() {
        stop_bundle(bundle, &recorder.cancel, &recorder.recording);
        return;
    }
    start_bundle(
        &mut recorder,
        &identity,
        paths.as_deref(),
        SessionState::LoggedOut,
    );
    if let Some(bundle) = recorder.bundle.take()
        && bundle.join().is_err()
    {
        error!("scene capture: the bundling thread panicked");
    }
}

/// Cancel a running bundle and give it [`EXIT_WAIT`] to write its report, so that
/// closing the viewer is never held up by a slow service. One that takes longer
/// is left behind: its cache files and report are each replaced atomically, so a
/// later resume finds a valid recording either way.
fn stop_bundle(thread: JoinHandle<BundleOutcome>, cancel: &AtomicBool, recording: &Path) {
    cancel.store(true, Ordering::Relaxed);
    let started = Instant::now();
    while !thread.is_finished() && started.elapsed() < EXIT_WAIT {
        std::thread::sleep(Duration::from_millis(20));
    }
    if !thread.is_finished() {
        warn!(
            "scene capture: exiting before the bundle of {} stopped; resume it to complete it",
            recording.display()
        );
    } else if thread.join().is_err() {
        error!("scene capture: the bundling thread panicked");
    }
}

/// A resume of an earlier recording's bundle.
#[derive(Debug, Resource)]
struct SceneResume {
    /// The recording directory.
    recording: PathBuf,
    /// The capability URLs received so far.
    caps: CaptureCaps,
    /// The fetching thread, once it has been started.
    thread: Option<JoinHandle<BundleOutcome>>,
    /// Whether the resume has been started, which stays true after it ended.
    started: bool,
    /// Set to make a running resume give up on what it has not fetched yet.
    cancel: Arc<AtomicBool>,
}

/// Start the resume once the session has a texture capability to fetch from.
fn resume_when_capabilities_arrive(
    mut capabilities: MessageReader<SlCapabilities>,
    mut resume: ResMut<SceneResume>,
    identity: Res<SlIdentity>,
    paths: Option<Res<ViewerPaths>>,
) {
    for SlCapabilities(map) in capabilities.read() {
        resume.caps.update(map);
    }
    if resume.started || resume.caps.texture.is_none() {
        return;
    }
    let job = BundleJob {
        recording: resume.recording.clone(),
        cancel: Arc::clone(&resume.cancel),
        live: paths.as_deref().cloned(),
        caps: resume.caps.clone(),
        appearance_service: identity
            .agent_appearance_service
            .as_ref()
            .map(ToString::to_string),
        now_unix: unix_now(),
    };
    info!(
        "scene capture: resuming the bundle of {}",
        job.recording.display()
    );
    resume.started = true;
    resume.thread = Some(sl_client_bevy::log_context::spawn_thread(move || {
        conclude(resume_recording(&job), &job.recording)
    }));
}

/// Wait for a running resume when the viewer exits, and say so if none started.
fn finish_resume_on_exit(mut exits: MessageReader<AppExit>, mut resume: ResMut<SceneResume>) {
    if exits.read().next().is_none() {
        return;
    }
    if let Some(thread) = resume.thread.take() {
        stop_bundle(thread, &resume.cancel, &resume.recording);
    } else if !resume.started {
        error!(
            "scene capture: {} was not resumed, the session never had a texture capability",
            resume.recording.display()
        );
    }
}

/// How a bundle ended, for the line in nearby chat.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BundleOutcome {
    /// Every referenced asset is in the bundle.
    Complete,
    /// This many referenced assets could not be obtained.
    Incomplete {
        /// How many.
        missing: usize,
    },
    /// The bundle could not be made at all; the log says why.
    Failed,
}

impl BundleOutcome {
    /// The outcome a bundling run's result amounts to.
    fn of(result: &Result<AssetReport, String>) -> Self {
        match result {
            Ok(report) => match report.missing_count() {
                0 => Self::Complete,
                missing => Self::Incomplete { missing },
            },
            Err(_error) => Self::Failed,
        }
    }

    /// The chat line announcing this outcome for `recording`.
    fn message(self, recording: &Path, translator: &Translator) -> String {
        let path = recording.display().to_string();
        let args = TransArgs::new().text("path", &path);
        match self {
            Self::Complete => translator.format("scene-capture-complete", &args),
            Self::Incomplete { missing } => translator.format(
                "scene-capture-incomplete",
                &args.int("missing", i64::try_from(missing).unwrap_or(i64::MAX)),
            ),
            Self::Failed => translator.format("scene-capture-failed", &args),
        }
    }
}

/// Log how a bundling run ended and return its outcome for the chat line.
fn conclude(result: Result<AssetReport, String>, recording: &Path) -> BundleOutcome {
    let outcome = BundleOutcome::of(&result);
    match &result {
        Ok(report) => log_report(report, recording),
        Err(error) => error!(
            "scene capture: bundling {} failed: {error}",
            recording.display()
        ),
    }
    outcome
}

/// The outcome of the thread in `slot`, once it has ended. A panic counts as a
/// failure.
fn take_finished(slot: &mut Option<JoinHandle<BundleOutcome>>) -> Option<BundleOutcome> {
    if !slot.as_ref().is_some_and(JoinHandle::is_finished) {
        return None;
    }
    Some(slot.take()?.join().unwrap_or(BundleOutcome::Failed))
}

/// Say in nearby chat when a bundle has ended, so nobody has to read the log to
/// know when it is safe to log out.
fn announce_finished_bundle(
    recorder: Option<ResMut<SceneRecorder>>,
    resume: Option<ResMut<SceneResume>>,
    mut notices: MessageWriter<LocalChatNotice>,
    translator: Translator,
) {
    if let Some(mut recorder) = recorder
        && let Some(outcome) = take_finished(&mut recorder.bundle)
    {
        notices.write(LocalChatNotice::new(
            outcome.message(&recorder.recording, &translator),
        ));
    }
    if let Some(mut resume) = resume
        && let Some(outcome) = take_finished(&mut resume.thread)
    {
        notices.write(LocalChatNotice::new(
            outcome.message(&resume.recording, &translator),
        ));
    }
}

/// The current Unix time in whole seconds, for stamping bundle cache entries.
fn unix_now() -> u32 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| {
            u32::try_from(elapsed.as_secs()).unwrap_or(u32::MAX)
        })
}

/// What the bundling thread needs from the viewer.
#[derive(Debug)]
struct BundleJob {
    /// The recording directory.
    recording: PathBuf,
    /// The viewer's cache locations, to copy from.
    live: Option<ViewerPaths>,
    /// The capability URLs to fetch from.
    caps: CaptureCaps,
    /// The appearance service, which serves baked body textures.
    appearance_service: Option<String>,
    /// The time to stamp cache entries with.
    now_unix: u32,
    /// Set to give up on whatever has not been fetched yet.
    cancel: Arc<AtomicBool>,
}

/// Everything a timeline references.
#[derive(Default)]
struct SceneInputs {
    /// The referenced assets, by cache.
    assets: ReferencedAssets,
    /// The legacy material ids the faces reference.
    legacy_referenced: BTreeSet<Uuid>,
    /// The legacy materials the session received.
    legacy_received: BTreeSet<Uuid>,
    /// One manifest per appearance seen, which name the baked textures' sources.
    appearances: Vec<ReplayManifest>,
}

impl SceneInputs {
    /// Fold one recorded event in.
    fn add(&mut self, event: SceneEvent) {
        match event {
            SceneEvent::ObjectAdded(object) | SceneEvent::ObjectUpdated(object) => {
                self.add_object(&object);
            }
            SceneEvent::RegionInfoHandshake(identity) => {
                for id in identity.terrain.detail_textures_or_default() {
                    insert_non_nil(&mut self.assets.textures, id);
                }
            }
            SceneEvent::Environment(settings) => {
                for sky in settings.day_cycle.sky_frames.values() {
                    self.add_sky(sky);
                }
                for water in settings.day_cycle.water_frames.values() {
                    self.add_water(water);
                }
            }
            SceneEvent::AvatarAppearance(appearance) => {
                let manifest = ReplayManifest {
                    version: MANIFEST_VERSION,
                    agent: appearance.avatar_id.uuid(),
                    objects: Vec::new(),
                    appearance: Some(*appearance),
                    animations: Vec::new(),
                    render_materials: Vec::new(),
                };
                self.assets
                    .textures
                    .extend(referenced_assets(&manifest).textures);
                self.appearances.push(manifest);
            }
            SceneEvent::AvatarAnimation { animations, .. } => {
                for animation in animations {
                    insert_non_nil(&mut self.assets.anims, animation.anim_id);
                }
            }
            SceneEvent::ObjectAnimation { animations, .. } => {
                for animation in animations {
                    insert_non_nil(&mut self.assets.anims, animation.anim_id.uuid());
                }
            }
            SceneEvent::RenderMaterials(entries) => {
                for entry in &entries {
                    let _inserted = self.legacy_received.insert(entry.material_id);
                    self.add_legacy_maps(entry);
                }
            }
            SceneEvent::GltfMaterialOverride { overrides, .. } => {
                for raw in &overrides {
                    self.add_override(raw);
                }
            }
            SceneEvent::ObjectRemoved { .. }
            | SceneEvent::TerrainPatch(_)
            | SceneEvent::RegionHandshakeComplete
            | SceneEvent::RegionLimits(_)
            | SceneEvent::TimeDilation { .. } => {}
        }
    }

    /// Fold in what an object references, including what the avatar bundle has no
    /// use for: its legacy material ids and its particle texture.
    fn add_object(&mut self, object: &Object) {
        collect_object_assets(object, &mut self.assets);
        for face in &decode_texture_entry(&object.texture_entry, MAX_FACES).faces {
            if let Some(id) = face.material_id {
                insert_non_nil(&mut self.legacy_referenced, id);
            }
        }
        if let Some(texture) = plant_texture(object) {
            insert_non_nil(&mut self.assets.textures, texture);
        }
        if let Some(texture) = object
            .particles
            .as_ref()
            .and_then(|particles| particles.texture_id)
        {
            insert_non_nil(&mut self.assets.textures, texture.uuid());
        }
    }

    /// Fold in the textures a sky frame names.
    fn add_sky(&mut self, sky: &SkySettings) {
        for texture in [
            sky.sun_texture,
            sky.moon_texture,
            sky.cloud_texture,
            sky.bloom_texture,
            sky.halo_texture,
            sky.rainbow_texture,
        ]
        .into_iter()
        .flatten()
        {
            insert_non_nil(&mut self.assets.textures, texture.uuid());
        }
    }

    /// Fold in the textures a water frame names.
    fn add_water(&mut self, water: &WaterSettings) {
        for texture in [water.normal_map, water.transparent_texture]
            .into_iter()
            .flatten()
        {
            insert_non_nil(&mut self.assets.textures, texture.uuid());
        }
    }

    /// Fold in a legacy material's normal and specular maps.
    fn add_legacy_maps(&mut self, entry: &RenderMaterialEntry) {
        insert_non_nil(&mut self.assets.textures, entry.material.normal_map.uuid());
        insert_non_nil(
            &mut self.assets.textures,
            entry.material.specular_map.uuid(),
        );
    }

    /// Fold in the textures a PBR override sets. One that cannot be parsed is
    /// logged: the viewer cannot apply it either.
    fn add_override(&mut self, raw: &[u8]) {
        match parse_material_override(raw) {
            Ok(parsed) => {
                for slot in parsed.textures.into_iter().flatten() {
                    if let TextureOverride::Set(texture) = slot {
                        insert_non_nil(&mut self.assets.textures, texture.uuid());
                    }
                }
            }
            Err(error) => warn!("scene capture: unreadable material override: {error}"),
        }
    }

    /// The legacy materials referenced and never received.
    fn absent_legacy(&self) -> BTreeSet<Uuid> {
        self.legacy_referenced
            .difference(&self.legacy_received)
            .copied()
            .collect()
    }

    /// Each referenced texture and the URL that serves it, plus those nothing can
    /// serve. A baked texture comes from the appearance service.
    fn texture_sources(
        &self,
        get_texture: Option<&str>,
        appearance_service: Option<&str>,
    ) -> (BTreeMap<Uuid, String>, BTreeSet<Uuid>) {
        let mut bakes: HashMap<Uuid, String> = HashMap::new();
        for manifest in &self.appearances {
            bakes.extend(bake_fetch_urls(manifest, appearance_service));
        }
        let mut sources = BTreeMap::new();
        let mut unserved = BTreeSet::new();
        for &id in &self.assets.textures {
            let url = bakes
                .get(&id)
                .cloned()
                .or_else(|| get_texture.map(|cap| format!("{cap}/?texture_id={id}")));
            match url {
                Some(url) => {
                    let _previous = sources.insert(id, url);
                }
                None => {
                    let _inserted = unserved.insert(id);
                }
            }
        }
        (sources, unserved)
    }
}

/// What a bundle could not obtain.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct MissingAssets {
    /// Legacy materials no reply carried.
    pub legacy_materials: BTreeSet<Uuid>,
    /// Meshes that could not be fetched whole.
    pub meshes: BTreeSet<Uuid>,
    /// Animations neither cached nor fetchable.
    pub animations: BTreeSet<Uuid>,
    /// PBR materials neither cached nor fetchable.
    pub pbr_materials: BTreeSet<Uuid>,
    /// Textures that could not be fetched at full resolution.
    pub textures: BTreeSet<Uuid>,
}

impl From<BundleGaps> for MissingAssets {
    fn from(gaps: BundleGaps) -> Self {
        Self {
            legacy_materials: gaps.render_materials,
            meshes: gaps.meshes,
            animations: gaps.anims,
            pbr_materials: gaps.materials,
            textures: gaps.textures,
        }
    }
}

/// What a finished bundle holds, written as [`ASSET_REPORT_FILE`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AssetReport {
    /// The schema version ([`TIMELINE_VERSION`]).
    pub version: u32,
    /// How many textures the timeline references.
    pub textures: usize,
    /// How many meshes it references.
    pub meshes: usize,
    /// How many animations it references.
    pub animations: usize,
    /// How many PBR materials it references.
    pub pbr_materials: usize,
    /// How many legacy materials it references.
    pub legacy_materials: usize,
    /// What could not be obtained.
    pub missing: MissingAssets,
}

impl AssetReport {
    /// How many referenced assets are missing.
    #[must_use]
    pub fn missing_count(&self) -> usize {
        [
            self.missing.legacy_materials.len(),
            self.missing.meshes.len(),
            self.missing.animations.len(),
            self.missing.pbr_materials.len(),
            self.missing.textures.len(),
        ]
        .into_iter()
        .fold(0, usize::saturating_add)
    }
}

/// Write `value` as pretty JSON.
fn write_json<T: Serialize>(path: &Path, value: &T) -> Result<(), String> {
    let json = serde_json::to_vec_pretty(value)
        .map_err(|error| format!("serialize {}: {error}", path.display()))?;
    // Through a temporary file: a report cut short by a crash would misstate what
    // the bundle holds.
    let temporary = path.with_extension("json.tmp");
    fs_err::write(&temporary, json).map_err(|error| error.to_string())?;
    fs_err::rename(&temporary, path).map_err(|error| error.to_string())
}

/// Read a JSON file written by [`write_json`].
fn read_json<T: serde::de::DeserializeOwned>(path: &Path) -> Result<T, String> {
    let bytes = fs_err::read(path).map_err(|error| error.to_string())?;
    serde_json::from_slice(&bytes).map_err(|error| format!("parse {}: {error}", path.display()))
}

/// Everything the recording's timeline references.
fn timeline_inputs(recording: &Path) -> Result<SceneInputs, String> {
    let mut inputs = SceneInputs::default();
    for record in read_timeline(&recording.join(TIMELINE_FILE))? {
        if let TimelineEntry::Scene(event) = record.entry {
            inputs.add(*event);
        }
    }
    Ok(inputs)
}

/// Bundle everything the recording's timeline references. Blocking file and
/// network work, so off the main thread.
fn bundle_recording(job: &BundleJob) -> Result<AssetReport, String> {
    let mut inputs = timeline_inputs(&job.recording)?;
    // Legacy materials first: they name normal and specular maps to fetch below.
    let absent = inputs.absent_legacy();
    let fetched = match job.caps.render_materials.as_deref() {
        Some(cap) if !absent.is_empty() => run_render_material_fetch(cap, &absent),
        _other => HashMap::new(),
    };
    let mut entries: Vec<&RenderMaterialEntry> = fetched.values().collect();
    entries.sort_by_key(|entry| entry.material_id);
    for entry in &entries {
        inputs.add_legacy_maps(entry);
    }
    write_json(&job.recording.join(MATERIALS_FILE), &entries)?;
    let legacy_missing: BTreeSet<Uuid> = absent
        .into_iter()
        .filter(|id| !fetched.contains_key(id))
        .collect();

    let _copied = copy_referenced(
        job.live.as_ref(),
        &job.recording,
        &mut inputs.assets,
        job.now_unix,
    );
    let (textures, unserved) = inputs.texture_sources(
        job.caps.texture.as_deref(),
        job.appearance_service.as_deref(),
    );
    let mut report = AssetReport {
        version: TIMELINE_VERSION,
        textures: inputs.assets.textures.len(),
        meshes: inputs.assets.meshes.len(),
        animations: inputs.assets.anims.len(),
        pbr_materials: inputs.assets.materials.len(),
        legacy_materials: inputs.legacy_referenced.len(),
        missing: MissingAssets::default(),
    };
    info!(
        "scene capture: fetching {} texture(s), {} mesh(es), {} animation(s) and {} PBR \
         material(s)",
        textures.len(),
        report.meshes,
        report.animations,
        report.pbr_materials,
    );
    let plan = FetchPlan {
        meshes: inputs.assets.meshes,
        anims: inputs.assets.anims,
        materials: inputs.assets.materials,
        textures,
    };
    let mut missing = MissingAssets::from(run_bundle_fetch(
        &job.recording,
        plan,
        &job.caps,
        job.now_unix,
        RetryPolicy::Patient,
        &job.cancel,
    ));
    missing.legacy_materials = legacy_missing;
    missing.textures.extend(unserved);
    report.missing = missing;
    write_json(&job.recording.join(ASSET_REPORT_FILE), &report)?;
    Ok(report)
}

/// Fetch what `job`'s recording lacks according to its report, and rewrite the
/// report. A recording with no report never finished bundling, so it is bundled
/// from the start. Repeating this is safe: nothing already held is lost.
fn resume_recording(job: &BundleJob) -> Result<AssetReport, String> {
    let report_path = job.recording.join(ASSET_REPORT_FILE);
    if !report_path.exists() {
        return bundle_recording(job);
    }
    let mut report: AssetReport = read_json(&report_path)?;
    if report.version != TIMELINE_VERSION {
        return Err(format!(
            "{}: report version {} (this viewer reads {TIMELINE_VERSION})",
            report_path.display(),
            report.version,
        ));
    }
    // An older bundle counted the all-ones sentinel as a texture.
    if report.missing.textures.remove(&Uuid::max()) {
        report.textures = report.textures.saturating_sub(1);
    }
    let mut inputs = timeline_inputs(&job.recording)?;

    // Legacy materials first: they name normal and specular maps to fetch below.
    let wanted = report.missing.legacy_materials.clone();
    let fetched = match job.caps.render_materials.as_deref() {
        Some(cap) if !wanted.is_empty() => run_render_material_fetch(cap, &wanted),
        _other => HashMap::new(),
    };
    let mut maps = SceneInputs::default();
    for entry in fetched.values() {
        maps.add_legacy_maps(entry);
    }
    let new_maps = textures_not_bundled(&job.recording, &maps.assets.textures);
    let materials_path = job.recording.join(MATERIALS_FILE);
    let mut stored: BTreeMap<Uuid, RenderMaterialEntry> =
        read_json::<Vec<RenderMaterialEntry>>(&materials_path)?
            .into_iter()
            .map(|entry| (entry.material_id, entry))
            .collect();
    stored.extend(fetched.iter().map(|(id, entry)| (*id, entry.clone())));
    write_json(&materials_path, &stored.values().collect::<Vec<_>>())?;

    // The report's gaps, the new maps, and anything the timeline references that
    // an older bundle never knew to fetch. No other texture's source is needed.
    let mut added = textures_not_bundled(&job.recording, &inputs.assets.textures);
    added.extend(new_maps);
    let added: BTreeSet<Uuid> = added
        .difference(&report.missing.textures)
        .copied()
        .collect();
    report.textures = report.textures.saturating_add(added.len());
    let mut wanted_textures = report.missing.textures.clone();
    wanted_textures.extend(added);
    inputs.assets.textures = wanted_textures;
    let (textures, unserved) = inputs.texture_sources(
        job.caps.texture.as_deref(),
        job.appearance_service.as_deref(),
    );
    info!(
        "scene capture: fetching the missing {} texture(s), {} mesh(es), {} animation(s), {} PBR \
         and {} legacy material(s)",
        textures.len(),
        report.missing.meshes.len(),
        report.missing.animations.len(),
        report.missing.pbr_materials.len(),
        wanted.len(),
    );
    let plan = FetchPlan {
        meshes: report.missing.meshes.clone(),
        anims: report.missing.animations.clone(),
        materials: report.missing.pbr_materials.clone(),
        textures,
    };
    let mut missing = MissingAssets::from(run_bundle_fetch(
        &job.recording,
        plan,
        &job.caps,
        job.now_unix,
        RetryPolicy::Patient,
        &job.cancel,
    ));
    missing.legacy_materials = wanted
        .into_iter()
        .filter(|id| !fetched.contains_key(id))
        .collect();
    missing.textures.extend(unserved);
    report.missing = missing;
    write_json(&report_path, &report)?;
    Ok(report)
}

/// Say whether a bundle is complete, and what it lacks if not.
fn log_report(report: &AssetReport, recording: &Path) {
    let missing = report.missing_count();
    if missing == 0 {
        info!(
            "scene capture: complete, {} texture(s), {} mesh(es), {} animation(s), {} PBR and {} \
             legacy material(s) in {}",
            report.textures,
            report.meshes,
            report.animations,
            report.pbr_materials,
            report.legacy_materials,
            recording.display(),
        );
    } else {
        warn!(
            "scene capture: INCOMPLETE, {missing} asset(s) missing ({} texture(s), {} mesh(es), \
             {} animation(s), {} PBR and {} legacy material(s)); the ids are in {}",
            report.missing.textures.len(),
            report.missing.meshes.len(),
            report.missing.animations.len(),
            report.missing.pbr_materials.len(),
            report.missing.legacy_materials.len(),
            recording.join(ASSET_REPORT_FILE).display(),
        );
    }
}

/// The latest value per key, in the order the keys first appeared.
struct Latest<K, V> {
    /// The keys in order of first appearance.
    order: Vec<K>,
    /// The current value of each key.
    values: HashMap<K, V>,
}

impl<K, V> Default for Latest<K, V> {
    fn default() -> Self {
        Self {
            order: Vec::new(),
            values: HashMap::new(),
        }
    }
}

impl<K: Clone + Eq + Hash, V> Latest<K, V> {
    /// Set `key`'s value.
    fn insert(&mut self, key: K, value: V) {
        if self.values.insert(key.clone(), value).is_none() {
            self.order.push(key);
        }
    }

    /// Forget `key`.
    fn remove(&mut self, key: &K) {
        let _removed = self.values.remove(key);
    }

    /// The current values, in order of first appearance.
    fn into_values(mut self) -> Vec<V> {
        self.order
            .iter()
            .filter_map(|key| self.values.remove(key))
            .collect()
    }
}

/// The key of one terrain patch. The layer is held by name because its type has
/// no hash.
type PatchKey = (RegionHandle, String, u32, u32);

/// A recording's scene as of its last record.
#[derive(Default)]
struct SceneFold {
    /// Each region's identity.
    identities: Latest<RegionHandle, Box<RegionIdentity>>,
    /// The region's limits.
    limits: Option<RegionLimits>,
    /// Each region's and parcel's environment.
    environments: Latest<(Uuid, i32), Box<EnvironmentSettings>>,
    /// The terrain, wind and cloud patches.
    patches: Latest<PatchKey, Box<TerrainPatch>>,
    /// Each region's time dilation.
    dilations: Latest<RegionHandle, f32>,
    /// The objects still in the scene.
    objects: Latest<ScopedObjectId, Box<Object>>,
    /// The PBR overrides, per object and set of faces.
    overrides: Latest<(ScopedObjectId, Vec<u8>), SceneEvent>,
    /// Each avatar's appearance.
    appearances: Latest<AgentKey, Box<AvatarAppearance>>,
    /// Each avatar's animations.
    avatar_animations: Latest<AgentKey, Vec<PlayingAnimation>>,
    /// Each animated object's animations.
    object_animations: Latest<ObjectKey, Vec<ObjectPlayingAnimation>>,
    /// Every legacy material received.
    materials: BTreeMap<Uuid, RenderMaterialEntry>,
}

impl SceneFold {
    /// Apply one recorded event.
    fn apply(&mut self, event: SceneEvent) {
        match event {
            SceneEvent::ObjectAdded(object) | SceneEvent::ObjectUpdated(object) => {
                self.objects.insert(object.scoped_id(), object);
            }
            SceneEvent::ObjectRemoved { local_id, .. } => self.objects.remove(&local_id),
            SceneEvent::TerrainPatch(patch) => {
                let key = (
                    patch.region_handle,
                    format!("{:?}", patch.layer),
                    patch.patch_x,
                    patch.patch_y,
                );
                self.patches.insert(key, patch);
            }
            SceneEvent::RegionInfoHandshake(identity) => {
                self.identities.insert(identity.region_handle, identity);
            }
            SceneEvent::RegionHandshakeComplete => {}
            SceneEvent::RegionLimits(limits) => self.limits = Some(limits),
            SceneEvent::Environment(settings) => {
                self.environments
                    .insert((settings.region_id, settings.parcel_id), settings);
            }
            SceneEvent::TimeDilation {
                region_handle,
                dilation,
            } => self.dilations.insert(region_handle, dilation),
            SceneEvent::AvatarAppearance(appearance) => {
                self.appearances.insert(appearance.avatar_id, appearance);
            }
            SceneEvent::AvatarAnimation {
                avatar_id,
                animations,
            } => self.avatar_animations.insert(avatar_id, animations),
            SceneEvent::ObjectAnimation {
                object_id,
                animations,
            } => self.object_animations.insert(object_id, animations),
            SceneEvent::RenderMaterials(entries) => {
                self.materials
                    .extend(entries.into_iter().map(|entry| (entry.material_id, entry)));
            }
            event @ SceneEvent::GltfMaterialOverride { .. } => {
                if let SceneEvent::GltfMaterialOverride {
                    local_id, faces, ..
                } = &event
                {
                    let key = (*local_id, faces.clone());
                    self.overrides.insert(key, event);
                }
            }
        }
    }

    /// The scene as events in the order its consumers expect: the region and its
    /// ground first, then the objects, then what refers to them. An avatar or
    /// animated object that left is not brought back by its appearance.
    fn into_replay(self) -> SceneReplay {
        let identities = self.identities.into_values();
        let region_handle = identities.first().map(|identity| identity.region_handle);
        let objects = self.objects.into_values();
        let in_region = |object: &Object| Some(object.region_handle) == region_handle;
        let circuit = objects
            .iter()
            .find(|object| in_region(object))
            .map(|object| object.circuit);
        let focus = objects
            .iter()
            .filter(|object| in_region(object))
            .find(|object| object.pcode == sl_client_bevy::pcode::AVATAR)
            .map(|object| sl_viewer_kit::coords::sl_to_bevy_vec(&object.motion.position));
        let present: HashSet<Uuid> = objects.iter().map(|object| object.full_id.uuid()).collect();

        let mut events: Vec<SceneEvent> = identities
            .into_iter()
            .map(SceneEvent::RegionInfoHandshake)
            .collect();
        events.extend(self.limits.map(SceneEvent::RegionLimits));
        events.push(SceneEvent::RegionHandshakeComplete);
        events.extend(
            self.environments
                .into_values()
                .into_iter()
                .map(SceneEvent::Environment),
        );
        events.extend(
            self.patches
                .into_values()
                .into_iter()
                .map(SceneEvent::TerrainPatch),
        );
        let dilations = self.dilations;
        for region in dilations.order.clone() {
            if let Some(dilation) = dilations.values.get(&region) {
                events.push(SceneEvent::TimeDilation {
                    region_handle: region,
                    dilation: *dilation,
                });
            }
        }
        events.extend(objects.into_iter().map(SceneEvent::ObjectAdded));
        if !self.materials.is_empty() {
            events.push(SceneEvent::RenderMaterials(
                self.materials.into_values().collect(),
            ));
        }
        events.extend(self.overrides.into_values());
        events.extend(
            self.appearances
                .into_values()
                .into_iter()
                .filter(|appearance| present.contains(&appearance.avatar_id.uuid()))
                .map(SceneEvent::AvatarAppearance),
        );
        let avatar_animations = self.avatar_animations;
        for avatar_id in avatar_animations.order.clone() {
            if present.contains(&avatar_id.uuid())
                && let Some(animations) = avatar_animations.values.get(&avatar_id)
            {
                events.push(SceneEvent::AvatarAnimation {
                    avatar_id,
                    animations: animations.clone(),
                });
            }
        }
        let object_animations = self.object_animations;
        for object_id in object_animations.order.clone() {
            if present.contains(&object_id.uuid())
                && let Some(animations) = object_animations.values.get(&object_id)
            {
                events.push(SceneEvent::ObjectAnimation {
                    object_id,
                    animations: animations.clone(),
                });
            }
        }
        SceneReplay {
            events,
            region_handle,
            circuit,
            focus,
        }
    }
}

/// A recording's final scene, ready to be delivered to the render systems.
#[derive(Debug, Default)]
pub struct SceneReplay {
    /// The events that rebuild the scene, in delivery order.
    pub(crate) events: Vec<SceneEvent>,
    /// The region the world origin is set to.
    pub(crate) region_handle: Option<RegionHandle>,
    /// That region's circuit.
    pub(crate) circuit: Option<CircuitId>,
    /// Where the first avatar in that region stands, in Bevy coordinates, for
    /// framing the camera.
    pub(crate) focus: Option<Vec3>,
}

/// Fold `records`, plus the legacy materials a bundle fetched, into the scene.
fn fold_scene(
    records: Vec<TimelineRecord>,
    fetched_materials: Vec<RenderMaterialEntry>,
) -> SceneReplay {
    let mut fold = SceneFold::default();
    for record in records {
        if let TimelineEntry::Scene(event) = record.entry {
            fold.apply(*event);
        }
    }
    fold.materials.extend(
        fetched_materials
            .into_iter()
            .map(|entry| (entry.material_id, entry)),
    );
    fold.into_replay()
}

/// Whether `dir` is a region recording rather than an avatar bundle.
#[must_use]
pub fn is_recording(dir: &Path) -> bool {
    dir.join(TIMELINE_FILE).is_file()
}

/// Load a bundled recording for replay. Assets the bundle lacks are warned about,
/// since they draw differently, but do not stop it.
///
/// # Errors
///
/// Returns a description if the recording was never bundled, or if one of its
/// files cannot be read.
pub fn load_scene(dir: &Path) -> Result<SceneReplay, String> {
    let report_path = dir.join(ASSET_REPORT_FILE);
    if !report_path.is_file() {
        return Err(format!(
            "{} has no bundle yet: resume it with {SCENE_CAPTURE_RESUME_ENV}",
            dir.display()
        ));
    }
    let report: AssetReport = read_json(&report_path)?;
    if report.version != TIMELINE_VERSION {
        return Err(format!(
            "{}: report version {} (this viewer reads {TIMELINE_VERSION})",
            report_path.display(),
            report.version,
        ));
    }
    let missing = report.missing_count();
    if missing > 0 {
        warn!(
            "scene replay: {missing} referenced asset(s) are not in the bundle and will draw \
             differently; the ids are in {}",
            report_path.display()
        );
    }
    let materials = read_json(&dir.join(MATERIALS_FILE))?;
    let records = read_timeline(&dir.join(TIMELINE_FILE))?;
    Ok(fold_scene(records, materials))
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;
    use sl_client_bevy::RegionLocalObjectId;

    use super::*;

    /// A fresh, empty directory for one test.
    fn temp_dir(tag: &str) -> Result<PathBuf, String> {
        let dir =
            std::env::temp_dir().join(format!("sl-scene-capture-{tag}-{}", std::process::id()));
        let _absent = fs_err::remove_dir_all(&dir);
        fs_err::create_dir_all(&dir).map_err(|error| error.to_string())?;
        Ok(dir)
    }

    /// A short timeline covering a scene event and both asset kinds.
    fn sample_records() -> Vec<TimelineRecord> {
        let region_handle = RegionHandle(42);
        vec![
            TimelineRecord {
                frame: 0,
                micros: 10,
                entry: TimelineEntry::Scene(Box::new(SceneEvent::RegionHandshakeComplete)),
            },
            TimelineRecord {
                frame: 3,
                micros: 50_000,
                entry: TimelineEntry::Scene(Box::new(SceneEvent::ObjectRemoved {
                    region_handle,
                    local_id: ScopedObjectId::new(CircuitId(7), RegionLocalObjectId(9)),
                })),
            },
            TimelineRecord {
                frame: 3,
                micros: 50_001,
                entry: TimelineEntry::Scene(Box::new(SceneEvent::TimeDilation {
                    region_handle,
                    dilation: 0.5,
                })),
            },
            TimelineRecord {
                frame: 4,
                micros: 66_000,
                entry: TimelineEntry::Asset(AssetArrival::Texture(Uuid::from_u128(1))),
            },
            TimelineRecord {
                frame: 4,
                micros: 66_001,
                entry: TimelineEntry::Asset(AssetArrival::Mesh(Uuid::from_u128(2))),
            },
        ]
    }

    /// Write `records` through the real writer.
    fn write_records(path: &Path, records: &[TimelineRecord]) -> Result<(), String> {
        let mut writer = TimelineWriter::create(path)?;
        for record in records {
            writer.push(record.clone());
        }
        writer.finish();
        Ok(())
    }

    #[test]
    fn a_timeline_reads_back_in_order() -> Result<(), String> {
        let dir = temp_dir("round-trip")?;
        let path = dir.join(TIMELINE_FILE);
        let records = sample_records();
        write_records(&path, &records)?;
        assert_eq!(read_timeline(&path)?, records);
        fs_err::remove_dir_all(&dir).map_err(|error| error.to_string())
    }

    #[test]
    fn a_recording_is_never_overwritten() -> Result<(), String> {
        let dir = temp_dir("no-overwrite")?;
        let path = dir.join(TIMELINE_FILE);
        write_records(&path, &sample_records())?;
        let _refusal = TimelineWriter::create(&path)
            .err()
            .ok_or("a recording was overwritten")?;
        assert_eq!(read_timeline(&path)?, sample_records());
        fs_err::remove_dir_all(&dir).map_err(|error| error.to_string())
    }

    #[test]
    fn a_truncated_timeline_is_an_error() -> Result<(), String> {
        let dir = temp_dir("truncated")?;
        let path = dir.join(TIMELINE_FILE);
        write_records(&path, &sample_records())?;
        let bytes = fs_err::read(&path).map_err(|error| error.to_string())?;
        let kept = bytes.len().saturating_sub(4);
        let cut = bytes.get(..kept).ok_or("timeline too short")?;
        fs_err::write(&path, cut).map_err(|error| error.to_string())?;
        let refusal = read_timeline(&path)
            .err()
            .ok_or("a truncated timeline was accepted")?;
        assert!(refusal.contains("cut short"), "{refusal}");
        fs_err::remove_dir_all(&dir).map_err(|error| error.to_string())
    }

    #[test]
    fn another_schema_version_is_refused() -> Result<(), String> {
        let dir = temp_dir("version")?;
        let path = dir.join(TIMELINE_FILE);
        let file = fs_err::File::create(&path).map_err(|error| error.to_string())?;
        let mut encoder = GzEncoder::new(file, Compression::fast());
        write_line(
            &mut encoder,
            &TimelineHeader {
                version: TIMELINE_VERSION.saturating_add(1),
            },
        )?;
        let _file = encoder.finish().map_err(|error| error.to_string())?;
        let refusal = read_timeline(&path).err().ok_or("version accepted")?;
        assert!(refusal.contains("timeline version"));
        fs_err::remove_dir_all(&dir).map_err(|error| error.to_string())
    }

    #[test]
    fn only_scene_events_are_recorded() {
        assert_eq!(
            SceneEvent::capture(&SlSessionEvent::RegionHandshakeComplete),
            Some(SceneEvent::RegionHandshakeComplete),
        );
        assert_eq!(
            SceneEvent::capture(&SlSessionEvent::SimulatorVersion("test".to_owned())),
            None,
        );
    }

    #[test]
    fn a_bundle_without_a_source_reports_what_it_lacks() -> Result<(), String> {
        let dir = temp_dir("bundle")?;
        let animation = Uuid::from_u128(77);
        write_records(
            &dir.join(TIMELINE_FILE),
            &[TimelineRecord {
                frame: 0,
                micros: 0,
                entry: TimelineEntry::Scene(Box::new(SceneEvent::AvatarAnimation {
                    avatar_id: AgentKey::from(Uuid::from_u128(5)),
                    animations: vec![PlayingAnimation {
                        anim_id: animation,
                        sequence_id: 1,
                        source_id: None,
                    }],
                })),
            }],
        )?;
        let report = bundle_recording(&BundleJob {
            recording: dir.clone(),
            live: None,
            caps: CaptureCaps::default(),
            appearance_service: None,
            now_unix: 1,
            cancel: Arc::default(),
        })?;
        assert_eq!(report.animations, 1);
        assert_eq!(report.missing_count(), 1);
        assert!(report.missing.animations.contains(&animation));
        let written = fs_err::read(dir.join(ASSET_REPORT_FILE)).map_err(|e| e.to_string())?;
        let reread: AssetReport = serde_json::from_slice(&written).map_err(|e| e.to_string())?;
        assert_eq!(reread, report);
        fs_err::remove_dir_all(&dir).map_err(|error| error.to_string())
    }

    #[test]
    fn a_resume_fetches_the_gap_and_rewrites_the_report() -> Result<(), String> {
        use std::io::{Read as _, Write as _};

        let dir = temp_dir("resume")?;
        let animation = Uuid::from_u128(78);
        write_records(
            &dir.join(TIMELINE_FILE),
            &[TimelineRecord {
                frame: 0,
                micros: 0,
                entry: TimelineEntry::Scene(Box::new(SceneEvent::AvatarAnimation {
                    avatar_id: AgentKey::from(Uuid::from_u128(5)),
                    animations: vec![PlayingAnimation {
                        anim_id: animation,
                        sequence_id: 1,
                        source_id: None,
                    }],
                })),
            }],
        )?;
        let offline = BundleJob {
            recording: dir.clone(),
            live: None,
            caps: CaptureCaps::default(),
            appearance_service: None,
            now_unix: 1,
            cancel: Arc::default(),
        };
        assert_eq!(bundle_recording(&offline)?.missing_count(), 1);
        // With still no source, a resume keeps the gap on record.
        assert_eq!(resume_recording(&offline)?.missing_count(), 1);

        let listener =
            std::net::TcpListener::bind("127.0.0.1:0").map_err(|error| error.to_string())?;
        let port = listener
            .local_addr()
            .map_err(|error| error.to_string())?
            .port();
        let server = std::thread::spawn(move || {
            let Ok((mut stream, _peer)) = listener.accept() else {
                return;
            };
            let mut request = [0_u8; 1024];
            let _read = stream.read(&mut request);
            let _written = stream.write_all(
                b"HTTP/1.1 200 OK\r\nContent-Length: 4\r\nConnection: close\r\n\r\nanim",
            );
        });
        let report = resume_recording(&BundleJob {
            caps: CaptureCaps {
                viewer_asset: Some(format!("http://127.0.0.1:{port}")),
                ..CaptureCaps::default()
            },
            ..offline
        })?;
        server.join().map_err(|_panic| "server panicked")?;
        assert_eq!(report.missing_count(), 0);
        let reread: AssetReport = read_json(&dir.join(ASSET_REPORT_FILE))?;
        assert_eq!(reread, report);
        fs_err::remove_dir_all(&dir).map_err(|error| error.to_string())
    }

    #[test]
    fn the_chat_line_follows_the_report() {
        let mut report = AssetReport {
            version: TIMELINE_VERSION,
            textures: 2,
            meshes: 0,
            animations: 0,
            pbr_materials: 0,
            legacy_materials: 0,
            missing: MissingAssets::default(),
        };
        assert_eq!(
            BundleOutcome::of(&Ok(report.clone())),
            BundleOutcome::Complete
        );
        let _inserted = report.missing.textures.insert(Uuid::from_u128(1));
        assert_eq!(
            BundleOutcome::of(&Ok(report)),
            BundleOutcome::Incomplete { missing: 1 }
        );
        assert_eq!(
            BundleOutcome::of(&Err("no timeline".to_owned())),
            BundleOutcome::Failed
        );
    }

    /// A minimal object with the given id, parent-less, of class `pcode`.
    fn object(id: u32, pcode: u8, x: f32) -> Box<Object> {
        let zero = || sl_client_bevy::Vector {
            x: 0.0,
            y: 0.0,
            z: 0.0,
        };
        Box::new(Object {
            region_handle: RegionHandle(42),
            local_id: RegionLocalObjectId(id),
            circuit: CircuitId(7),
            full_id: Uuid::from_u128(u128::from(id)).into(),
            parent_id: RegionLocalObjectId(0),
            pcode,
            state: 0,
            crc: 0,
            material: 0,
            click_action: 0,
            update_flags: 0,
            scale: sl_client_bevy::Vector {
                x: 1.0,
                y: 1.0,
                z: 1.0,
            },
            motion: sl_client_bevy::ObjectMotion {
                position: sl_client_bevy::Vector { x, y: 0.0, z: 0.0 },
                velocity: zero(),
                acceleration: zero(),
                rotation: sl_client_bevy::Rotation {
                    x: 0.0,
                    y: 0.0,
                    z: 0.0,
                    s: 1.0,
                },
                angular_velocity: zero(),
                collision_plane: None,
            },
            owner_id: Uuid::nil(),
            sound: Uuid::nil(),
            gain: 0.0,
            sound_flags: 0,
            sound_radius: 0.0,
            text: String::new(),
            text_color: [0; 4],
            name_value: String::new(),
            media_url: None,
            texture_entry: Vec::new(),
            texture_anim: Vec::new(),
            texture_animation: None,
            shape: sl_client_bevy::PrimShapeParams::default(),
            particle_system: Vec::new(),
            particles: None,
            data: Vec::new(),
            extra_params: Vec::new(),
            extra: sl_client_bevy::ObjectExtraParams::default(),
            properties: None,
            joint_type: 0,
            joint_pivot: zero(),
            joint_axis_or_anchor: zero(),
        })
    }

    /// `event` as a timeline record.
    fn scene_record(event: SceneEvent) -> TimelineRecord {
        TimelineRecord {
            frame: 0,
            micros: 0,
            entry: TimelineEntry::Scene(Box::new(event)),
        }
    }

    #[test]
    fn replay_holds_the_final_state_of_the_scene() {
        let avatar = sl_client_bevy::pcode::AVATAR;
        let gone = object(2, avatar, 9.0);
        let gone_key = AgentKey::from(gone.full_id.uuid());
        let scene = fold_scene(
            vec![
                scene_record(SceneEvent::ObjectAdded(object(1, avatar, 1.0))),
                scene_record(SceneEvent::ObjectAdded(gone.clone())),
                scene_record(SceneEvent::AvatarAnimation {
                    avatar_id: gone_key,
                    animations: Vec::new(),
                }),
                scene_record(SceneEvent::ObjectUpdated(object(1, avatar, 5.0))),
                scene_record(SceneEvent::ObjectRemoved {
                    region_handle: RegionHandle(42),
                    local_id: gone.scoped_id(),
                }),
            ],
            Vec::new(),
        );
        let objects: Vec<&Object> = scene
            .events
            .iter()
            .filter_map(|event| match event {
                SceneEvent::ObjectAdded(object) => Some(&**object),
                _other => None,
            })
            .collect();
        assert_eq!(objects.len(), 1);
        assert_eq!(
            objects.first().map(|object| object.motion.position.x),
            Some(5.0)
        );
        assert!(
            !scene
                .events
                .iter()
                .any(|event| matches!(event, SceneEvent::AvatarAnimation { .. })),
            "the animations of an avatar that left must not be replayed",
        );
    }

    #[test]
    fn a_plants_species_texture_is_bundled() {
        for plant in [
            sl_client_bevy::pcode::TREE,
            sl_client_bevy::pcode::NEW_TREE,
            sl_client_bevy::pcode::GRASS,
        ] {
            let mut inputs = SceneInputs::default();
            inputs.add(SceneEvent::ObjectAdded(object(1, plant, 0.0)));
            assert_eq!(inputs.assets.textures.len(), 1, "pcode {plant}");
        }
    }

    #[test]
    fn a_resume_finds_what_an_older_bundle_never_fetched() -> Result<(), String> {
        let dir = temp_dir("rediscover")?;
        write_records(
            &dir.join(TIMELINE_FILE),
            &[scene_record(SceneEvent::ObjectAdded(object(
                1,
                sl_client_bevy::pcode::TREE,
                0.0,
            )))],
        )?;
        write_json(
            &dir.join(ASSET_REPORT_FILE),
            &AssetReport {
                version: TIMELINE_VERSION,
                textures: 0,
                meshes: 0,
                animations: 0,
                pbr_materials: 0,
                legacy_materials: 0,
                missing: MissingAssets::default(),
            },
        )?;
        write_json(
            &dir.join(MATERIALS_FILE),
            &Vec::<RenderMaterialEntry>::new(),
        )?;
        let report = resume_recording(&BundleJob {
            recording: dir.clone(),
            live: None,
            caps: CaptureCaps::default(),
            appearance_service: None,
            now_unix: 1,
            cancel: Arc::default(),
        })?;
        assert_eq!(report.textures, 1);
        assert_eq!(report.missing.textures.len(), 1);
        fs_err::remove_dir_all(&dir).map_err(|error| error.to_string())
    }

    #[test]
    fn a_recording_without_a_bundle_is_refused() -> Result<(), String> {
        let dir = temp_dir("unbundled")?;
        write_records(&dir.join(TIMELINE_FILE), &sample_records())?;
        assert!(is_recording(&dir));
        let refusal = load_scene(&dir).err().ok_or("loaded without a bundle")?;
        assert!(refusal.contains("no bundle"), "{refusal}");
        fs_err::remove_dir_all(&dir).map_err(|error| error.to_string())
    }

    #[test]
    fn a_texture_without_a_source_is_not_lost() {
        let mut inputs = SceneInputs::default();
        let texture = Uuid::from_u128(9);
        insert_non_nil(&mut inputs.assets.textures, texture);
        let (served, unserved) = inputs.texture_sources(None, None);
        assert!(served.is_empty());
        assert!(unserved.contains(&texture));
        let (served, unserved) = inputs.texture_sources(Some("http://cap"), None);
        assert_eq!(
            served.get(&texture).map(String::as_str),
            Some("http://cap/?texture_id=00000000-0000-0000-0000-000000000009"),
        );
        assert!(unserved.is_empty());
    }

    #[test]
    fn a_recorded_event_replays_as_the_original() {
        let original = SlSessionEvent::TimeDilation {
            region_handle: RegionHandle(42),
            dilation: 0.25,
        };
        let replayed = SceneEvent::capture(&original).map(SceneEvent::into_session_event);
        assert_eq!(replayed, Some(original));
    }
}
