//! The viewer's media engine runtime (`viewer-media-prim-browser` /
//! `viewer-video-playback`): the Bevy side of the [`sl_media`] boundary and
//! its **two** engines — offscreen Chromium ([`sl_cef`]) for web pages, and
//! GStreamer ([`sl_gst`]) for direct video / audio URLs — one offscreen
//! *surface* per page / stream, pumped once per frame on the main thread,
//! each surface's BGRA frames mirrored into a Bevy [`Image`] that UI widgets
//! ([`crate::browser_widget`]) and in-world media faces
//! ([`crate::media_prim`]) sample. Which engine serves a URL is decided by
//! [`sl_media::classify_url`] (the `mime_types.xml` dispatch); the mirror
//! path is engine-agnostic.
//!
//! Everything CEF is thread-affine and `!Send`, so the engines and the
//! surface table live in **non-send** resources ([`MediaEngine`],
//! [`MediaSurfaces`]) — Bevy then schedules every system touching them onto
//! the main thread, which is exactly the thread that initialised CEF. The
//! GStreamer backend is pumped on the same thread for uniformity (its
//! pipelines run their own streaming threads regardless).
//!
//! The engines are optional at every level: `--disable-web-media` /
//! `--disable-video-media` skip initialisation, a missing `sl-cef-helper`
//! binary or engine runtime fails soft (a warning, no surfaces), and every
//! consumer treats "no engine" as "surface never appears".

use std::collections::HashMap;
use std::path::PathBuf;

use bevy::asset::RenderAssetUsages;
use bevy::image::{ImageAddressMode, ImageSampler, ImageSamplerDescriptor};
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use sl_cef::chromium::CefMediaBackend;
use sl_cef::{BackendConfig, MediaBackend, SurfaceConfig, SurfaceStatus};

/// System sets ordering the media engine's frame work: consumers that create
/// or drive surfaces run **after** [`MediaEngineSystems::Pump`], which is when
/// paints and status changes land.
#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum MediaEngineSystems {
    /// Pump CEF's message loop and mirror new frames / status snapshots.
    Pump,
}

/// A handle to one live media surface in [`MediaSurfaces`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub(crate) struct MediaSurfaceId(u64);

impl MediaSurfaceId {
    /// The sentinel a consumer holds when the engine refused (or has no)
    /// surface, so creation is not retried every frame. Never present in the
    /// surface table.
    pub(crate) const PLACEHOLDER: Self = Self(u64::MAX);
}

/// Which engine a surface runs on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum MediaEngineKind {
    /// The web browser engine (CEF).
    #[default]
    Web,
    /// The video / audio playback engine (GStreamer).
    Video,
}

/// The global engines (non-send resource): each `None` until initialised,
/// and again after shutdown or a failed initialisation.
#[derive(Default)]
pub(crate) struct MediaEngine {
    /// The web (CEF) backend, when live.
    backend: Option<Box<dyn MediaBackend>>,
    /// The video / audio playback (GStreamer) backend, when live.
    video_backend: Option<Box<dyn MediaBackend>>,
    /// Whether initialisation already ran (successfully or not), so it is
    /// attempted once.
    initialized: bool,
    /// Whether the web engine is enabled at all (`--disable-web-media`
    /// clears it).
    pub(crate) enabled: bool,
    /// Whether the video engine is enabled at all (`--disable-video-media`
    /// clears it).
    pub(crate) video_enabled: bool,
}

/// One live surface: the engine-side handle plus the Bevy image its frames
/// are mirrored into.
pub(crate) struct MediaSlot {
    /// Which engine the surface runs on (decides the control set the UI
    /// offers: navigation for web, transport for video).
    pub(crate) kind: MediaEngineKind,
    /// The engine surface.
    pub(crate) surface: Box<dyn sl_cef::MediaSurface>,
    /// The Bevy image the newest frame lives in (BGRA, sRGB).
    pub(crate) image: Handle<Image>,
    /// The newest status snapshot (refreshed each pump).
    pub(crate) status: SurfaceStatus,
    /// The current image size in pixels.
    pub(crate) size: UVec2,
    /// Materials sampling [`image`](Self::image) that must be touched when the
    /// frame changes: a `StandardMaterial`'s bind group caches the texture
    /// view and nothing watches `AssetEvent<Image>` for materials (see
    /// `crate::textures::PrimTextures::materials`), so each new frame marks
    /// these changed.
    pub(crate) touch_materials: Vec<Handle<StandardMaterial>>,
    /// The last frame generation mirrored into [`image`](Self::image).
    seen_frame: u64,
    /// Whether a close was requested; the slot is pruned once the engine
    /// reports the browser closed.
    closing: bool,
}

/// The table of live surfaces (non-send resource).
#[derive(Default)]
pub(crate) struct MediaSurfaces {
    /// The live slots by id.
    slots: HashMap<MediaSurfaceId, MediaSlot>,
    /// The next id to hand out.
    next: u64,
}

impl MediaSurfaces {
    /// Creates a surface through `engine`'s web (CEF) backend, allocating its
    /// mirror [`Image`] (a 1×1 placeholder until the first paint arrives).
    /// Returns `None` when the engine is not live or refuses the surface.
    pub(crate) fn create(
        &mut self,
        engine: &mut MediaEngine,
        images: &mut Assets<Image>,
        config: &SurfaceConfig,
    ) -> Option<MediaSurfaceId> {
        self.create_kind(engine, images, config, MediaEngineKind::Web)
    }

    /// Creates a surface on the backend for `kind` (web pages on CEF, direct
    /// video / audio on GStreamer), allocating its mirror [`Image`]. Returns
    /// `None` when that engine is not live or refuses the surface.
    pub(crate) fn create_kind(
        &mut self,
        engine: &mut MediaEngine,
        images: &mut Assets<Image>,
        config: &SurfaceConfig,
        kind: MediaEngineKind,
    ) -> Option<MediaSurfaceId> {
        let backend = match kind {
            MediaEngineKind::Web => engine.backend.as_mut()?,
            MediaEngineKind::Video => engine.video_backend.as_mut()?,
        };
        match backend.create_surface(config) {
            Ok(surface) => {
                let id = MediaSurfaceId(self.next);
                self.next = self.next.wrapping_add(1);
                let image = images.add(placeholder_image());
                self.slots.insert(
                    id,
                    MediaSlot {
                        kind,
                        surface,
                        image,
                        status: SurfaceStatus::default(),
                        size: UVec2::ONE,
                        touch_materials: Vec::new(),
                        seen_frame: 0,
                        closing: false,
                    },
                );
                Some(id)
            }
            Err(error) => {
                warn!("media surface creation failed: {error}");
                None
            }
        }
    }

    /// The slot for `id`, if live.
    pub(crate) fn get(&self, id: MediaSurfaceId) -> Option<&MediaSlot> {
        self.slots.get(&id)
    }

    /// The mutable slot for `id`, if live.
    pub(crate) fn get_mut(&mut self, id: MediaSurfaceId) -> Option<&mut MediaSlot> {
        self.slots.get_mut(&id)
    }

    /// Requests the surface's close; the slot is pruned once the engine
    /// confirms it.
    pub(crate) fn close(&mut self, id: MediaSurfaceId) {
        if let Some(slot) = self.slots.get_mut(&id) {
            slot.closing = true;
            slot.surface.request_close();
        }
    }
}

/// A 1×1 transparent BGRA image, the placeholder every surface starts on.
fn placeholder_image() -> Image {
    Image::new(
        Extent3d {
            width: 1,
            height: 1,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        vec![0, 0, 0, 0],
        TextureFormat::Bgra8UnormSrgb,
        RenderAssetUsages::default(),
    )
}

/// The media engine plugin. `enabled: false` (from `--disable-web-media`) /
/// `video_enabled: false` (from `--disable-video-media`) register the
/// resources but never initialise that engine, so its consumers see a
/// permanently empty surface table.
pub(crate) struct MediaEnginePlugin {
    /// Whether the web (CEF) engine may initialise at all.
    pub(crate) enabled: bool,
    /// Whether the video (GStreamer) engine may initialise at all.
    pub(crate) video_enabled: bool,
}

impl Plugin for MediaEnginePlugin {
    fn build(&self, app: &mut App) {
        app.insert_non_send(MediaEngine {
            backend: None,
            video_backend: None,
            initialized: false,
            enabled: self.enabled,
            video_enabled: self.video_enabled,
        });
        app.insert_non_send(MediaSurfaces::default());
        app.add_systems(Startup, initialize_media_engine);
        app.add_systems(Update, pump_media_engine.in_set(MediaEngineSystems::Pump));
        app.add_systems(Last, shutdown_media_engine_on_exit);
    }
}

/// Startup: initialise the engine runtimes (once) — CEF pointed at the
/// `sl-cef-helper` binary next to the viewer executable and the shared cache
/// directory, GStreamer with a loud log of the system's playback gaps
/// (missing HTTP source / decoders). Failure is soft: the viewer runs
/// without the failed engine.
fn initialize_media_engine(mut engine: NonSendMut<MediaEngine>) {
    if engine.initialized {
        return;
    }
    engine.initialized = true;
    if engine.video_enabled {
        match sl_gst::GstMediaBackend::initialize() {
            Ok(backend) => {
                engine.video_backend = Some(Box::new(backend));
                info!("video-media engine (GStreamer) initialised");
                for gap in sl_gst::playback_gaps() {
                    warn!("video-media capability gap: {gap}");
                }
            }
            Err(error) => {
                warn!("video-media engine failed to initialise; continuing without it: {error}");
            }
        }
    }
    if !engine.enabled {
        return;
    }
    let subprocess_path = helper_path();
    if subprocess_path.is_none() {
        warn!(
            "sl-cef-helper not found next to the viewer binary; web media (media-on-a-prim, \
             embedded browser) is disabled"
        );
        return;
    }
    let cache_dir = crate::paths::media_engine_cache_dir()
        .unwrap_or_else(|| PathBuf::from(".sl-viewer-cef-cache"));
    let config = BackendConfig {
        cache_dir,
        subprocess_path,
        locale: None,
        user_agent_product: Some(format!("SLClientBevyViewer/{}", clap::crate_version!())),
    };
    match CefMediaBackend::initialize(&config) {
        Ok(backend) => {
            engine.backend = Some(Box::new(backend));
            info!("web-media engine (CEF) initialised");
        }
        Err(error) => {
            warn!("web-media engine failed to initialise; continuing without it: {error}");
        }
    }
}

/// The expected path of the CEF subprocess helper: `sl-cef-helper` next to
/// the running executable. `None` when it does not exist there.
fn helper_path() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let dir = exe.parent()?;
    let helper = dir.join("sl-cef-helper");
    helper.is_file().then_some(helper)
}

/// Per-frame: pump CEF (all paint / status callbacks fire inside), then
/// mirror each surface's new frame into its [`Image`] and refresh its status
/// snapshot. Prunes slots whose browser finished closing.
///
/// The mirror deliberately distinguishes two cases:
/// - **Same size**: mutate the existing image's pixel data in place. Bevy's
///   `GpuImage::prepare_asset` sees an unchanged texture descriptor with
///   `COPY_DST` and streams the new data into the **same** GPU texture, so
///   every material / UI bind group referencing it stays valid — no rebuild.
/// - **Size changed** (first real frame after the 1×1 placeholder, resizes):
///   insert a fresh image (new GPU texture) and touch the materials sampling
///   it once so their cached bind groups rebuild. Replacing the asset every
///   frame instead (the first attempt) starved `bevy_pbr`'s material
///   re-prepare and left faces permanently sampling the placeholder.
fn pump_media_engine(
    mut engine: NonSendMut<MediaEngine>,
    mut surfaces: NonSendMut<MediaSurfaces>,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    if let Some(backend) = engine.backend.as_mut() {
        backend.pump();
    }
    if let Some(backend) = engine.video_backend.as_mut() {
        backend.pump();
    }

    let mut finished: Vec<MediaSurfaceId> = Vec::new();
    for (&id, slot) in &mut surfaces.slots {
        slot.status = slot.surface.status();
        if slot.status.closed {
            if slot.closing {
                finished.push(id);
            }
            continue;
        }
        let mut new_image: Option<Image> = None;
        let mut updated = false;
        let slot_size = slot.size;
        let image_id = slot.image.id();
        slot.surface
            .with_new_frame(&mut slot.seen_frame, &mut |frame| {
                let frame_size = UVec2::new(frame.width, frame.height);
                if frame_size == slot_size
                    && let Some(mut image) = images.get_mut(image_id)
                    && let Some(data) = image.data.as_mut()
                    && data.len() == frame.bgra.len()
                {
                    data.clear();
                    data.extend_from_slice(frame.bgra);
                    updated = true;
                } else {
                    let mut image = Image::new(
                        Extent3d {
                            width: frame.width,
                            height: frame.height,
                            depth_or_array_layers: 1,
                        },
                        TextureDimension::D2,
                        frame.bgra.to_vec(),
                        TextureFormat::Bgra8UnormSrgb,
                        RenderAssetUsages::default(),
                    );
                    // COPY_DST lets later same-size frames stream into this
                    // texture without recreating it (see the system docs).
                    image.texture_descriptor.usage =
                        bevy::render::render_resource::TextureUsages::TEXTURE_BINDING
                            | bevy::render::render_resource::TextureUsages::COPY_DST;
                    // Second Life samples face textures with GL_REPEAT, and a
                    // media face honours the face's authored texture repeats /
                    // offsets (the reference wraps media picks with fmod), so
                    // the media image must wrap too — the default clamp
                    // sampler smears the page's edge pixels across the rest of
                    // the face wherever the transform leaves [0, 1].
                    image.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
                        address_mode_u: ImageAddressMode::Repeat,
                        address_mode_v: ImageAddressMode::Repeat,
                        address_mode_w: ImageAddressMode::Repeat,
                        ..ImageSamplerDescriptor::linear()
                    });
                    new_image = Some(image);
                }
            });
        if let Some(image) = new_image {
            let new_size = UVec2::new(image.width(), image.height());
            let _replaced = images.insert(image_id, image);
            slot.size = new_size;
            // A new GPU texture: rebuild the bind group of every material
            // sampling this image (see MediaSlot::touch_materials).
            slot.touch_materials
                .retain(|handle| materials.get_mut(handle.id()).is_some());
            debug!(
                "media surface resized to {}x{} ({:?}, {} material(s) touched)",
                new_size.x,
                new_size.y,
                image_id,
                slot.touch_materials.len()
            );
            updated = true;
        }
        // Internal debugging: dump each surface's newest mirrored frame as a
        // PPM so a black-face report can be split into "engine sent black"
        // vs "render path lost it".
        if updated && let Some(dir) = std::env::var_os("SL_VIEWER_DUMP_MEDIA_FRAMES") {
            dump_media_frame(std::path::Path::new(&dir), id, &images, slot);
        }
    }
    for id in finished {
        let _removed = surfaces.slots.remove(&id);
    }
}

/// Internal debugging (`SL_VIEWER_DUMP_MEDIA_FRAMES=<dir>`): write `slot`'s
/// newest mirrored frame as `media-surface-<id>.ppm` under `dir`.
fn dump_media_frame(
    dir: &std::path::Path,
    id: MediaSurfaceId,
    images: &Assets<Image>,
    slot: &MediaSlot,
) {
    let Some(image) = images.get(slot.image.id()) else {
        return;
    };
    let Some(data) = image.data.as_ref() else {
        return;
    };
    let mut out = format!("P6\n{} {}\n255\n", slot.size.x, slot.size.y).into_bytes();
    for pixel in data.chunks_exact(4) {
        out.push(pixel.get(2).copied().unwrap_or(0));
        out.push(pixel.get(1).copied().unwrap_or(0));
        out.push(pixel.first().copied().unwrap_or(0));
    }
    let _created = fs_err::create_dir_all(dir);
    let _written = fs_err::write(dir.join(format!("media-surface-{id:?}.ppm")), &out);
}

/// On app exit: close every surface and tear the engine down cleanly (CEF
/// must shut down on its own thread; the backend's `Drop` would also do this,
/// but doing it while the world is still intact keeps the teardown ordered).
fn shutdown_media_engine_on_exit(
    mut exits: MessageReader<AppExit>,
    mut engine: NonSendMut<MediaEngine>,
    mut surfaces: NonSendMut<MediaSurfaces>,
) {
    if exits.read().next().is_none() {
        return;
    }
    for slot in surfaces.slots.values_mut() {
        slot.surface.request_close();
    }
    surfaces.slots.clear();
    if let Some(mut backend) = engine.backend.take() {
        backend.shutdown();
    }
    if let Some(mut backend) = engine.video_backend.take() {
        backend.shutdown();
    }
}
