//! A scene **app** stepped across a moving day cycle — the fixture three scene
//! defects were found without (`viewer-audit-scene-live-daycycle-fixture`).
//!
//! # The blind spot it closes
//!
//! [`viewer-audit-scene-change-guards-day-cycle`], [`viewer-audit-probe-ambient-multiply`]
//! and [`viewer-audit-tonemap-legacy-sky`] were one bug wearing three
//! descriptions: each was **correct under the screenshot harness and wrong on a
//! live grid**, because the harness pins `SL_VIEWER_SKY_DAY_POSITION` to one
//! position for the whole run and a region does not. Under a pinned sun every
//! float-equality guard in the scene holds trivially, nothing downstream of the
//! sky ever moves, and a system that rewrites its target on every single frame
//! looks exactly like one that never writes at all.
//!
//! All three are fixed, and each fix landed with tests on the **pure** function
//! it extracted — `quantised_day_position`, `sky_ambient_light`,
//! `effective_tonemap_mix`. That is where the arithmetic belongs, and it is not
//! where the defects lived: every one of them was a *wiring* fault, between
//! systems, in an app. So this module is the app.
//!
//! # What it asserts, and why the pure tests cannot
//!
//! The thing the guards exist to prevent is a **write**: a `Mut` deref that
//! marks a resource changed, or an `Assets::get_mut` that queues an
//! [`AssetEvent::Modified`] and re-prepares a bind group. A pure function has no
//! write to observe. [`FrameWrites`] observes exactly those, through the same
//! signals the renderer reacts to, so a future system that reintroduces a
//! per-frame write fails a test rather than a profile.
//!
//! # Why it pins the position rather than letting the clock run
//!
//! `crate::sky::day_position` reads `SystemTime::now()`, so an app driven by it
//! samples wherever the wall clock happens to be — and a run of frames that must
//! land in one sampling cell would depend on how close the machine started to a
//! cell boundary. That is a flaky test, and the flake would be worst on the
//! assertion that matters most.
//!
//! So the fixture owns the clock: each frame it computes the day position from
//! its own simulated region time with the crate's real
//! [`quantised_day_position`], and pins *that*. Downstream, a pinned position
//! and a clock-derived one are the same value through the same code —
//! `day_position` returns the pin and every renderer takes it as an argument —
//! but this pin **advances between frames**, which is the one thing the
//! screenshot harness's does not do and the whole reason these three defects
//! survived it.
//!
//! [`viewer-audit-scene-change-guards-day-cycle`]: ../../../roadmap/done/viewer-audit-scene-change-guards-day-cycle.md
//! [`viewer-audit-probe-ambient-multiply`]: ../../../roadmap/done/viewer-audit-probe-ambient-multiply.md
//! [`viewer-audit-tonemap-legacy-sky`]: ../../../roadmap/done/viewer-audit-tonemap-legacy-sky.md

use core::time::Duration;

use bevy::asset::AssetApp as _;
use bevy::prelude::*;
use sl_client_bevy::{
    CloudMaterial, EnvironmentSettings, RegionHandle, SKY_LIGHTING_IMAGE, SkyMaterial, SlEvent,
    SlSessionEvent, StarMaterial, SunDiscMaterial, TerrainMaterial,
};
use sl_settings::SettingsStore;
use sl_viewer_settings::ViewerSettings;
use sl_viewer_world_api::{DecodedTextures, TerrainState, ViewerCamera};
use sl_viewer_world_objects::textures::{TextureDecoded, TextureManager};

use crate::environment::{EnvironmentState, ingest_environment};
use crate::exposure::ExposureRange;
use crate::render_overrides::RenderOverrides;
use crate::sky::{DAY_POSITION_STEPS, SceneSun, SkyPlugin, quantised_day_position};
use crate::terrain::{TerrainTextures, ensure_region};
use crate::tonemap::{SlTonemap, refresh_tonemap_settings};

/// Where in the day the fixture starts: dawn, where the sky changes fastest and
/// so the worst case for every write-on-change guard below it. The pure
/// quantiser tests sample the same point for the same reason.
const DAWN: f64 = 0.25;

/// How many frames run before the first [`DayCycle::step`] is handed back: the
/// `Startup` spawn, the frame that ingests the region environment, and one more
/// for the sky to settle onto it. Without them every test would have to skip its
/// own opening frames, which is the same thing said less clearly.
const PRIME_FRAMES: u32 = 3;

/// A region on a live four-hour day cycle: the legacy WindLight default with the
/// four ported presets keyframed across the day, so the blended sky actually
/// moves as the day position advances.
///
/// The shipped default is a **single-keyframe** cycle — it returns the same noon
/// frame at every position, so an app driven over it would settle for reasons
/// that have nothing to do with the quantiser and prove nothing. This is also
/// the cycle `sl-crosscheck` dresses a region with when a run pins the day
/// position, and the one `sl-fake-grid` serves.
pub(crate) fn moving_day_cycle() -> EnvironmentSettings {
    let mut settings = EnvironmentSettings::legacy_windlight_default();
    settings.day_length = 14400;
    settings.day_offset = 0;
    sl_viewer_kit::sky_presets::install_preset_day_cycle(&mut settings);
    settings
}

/// [`moving_day_cycle`] with every sky frame declaring a reflection-probe
/// ambiance, which is what makes a sky an **EEP** one rather than a legacy
/// (classic) one — the other side of the tone mapper's exemption.
///
/// The decode collapses `mCanAutoAdjust` to `reflection_probe_ambiance == 0`
/// (see [`ExposureRange`]), so raising the ambiance on the frames is the whole
/// difference between the two skies as far as the scene is concerned.
pub(crate) fn eep_day_cycle() -> EnvironmentSettings {
    let mut settings = moving_day_cycle();
    for sky in settings.day_cycle.sky_frames.values_mut() {
        sky.reflection_probe_ambiance = 1.0;
    }
    settings
}

/// What one frame of the fixture wrote — the four signals the renderer itself
/// reacts to, and the only evidence a write-on-change guard leaves behind.
///
/// Each is a *write*, not a value: a guard that holds leaves the target alone,
/// and a guard that misses marks it changed even when it stores the same number
/// back. The defects this module exists for were all of the second kind.
///
/// Counts rather than flags, because three of the five are genuinely counts —
/// there is one scene sun today and one sky dome, but there are as many terrain
/// materials as loaded regions — and a failure that says *how many* is a failure
/// that says which system did it.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Resource)]
pub(crate) struct FrameWrites {
    /// Writes to [`GlobalAmbientLight`] — the resource
    /// `viewer-audit-probe-ambient-multiply`'s post-pass dirtied unconditionally,
    /// at any probe-ambient share including the idempotent default.
    pub(crate) ambient: usize,
    /// Scene suns whose `Transform` or `DirectionalLight` was written, each of
    /// which rebuilds four shadow cascades and re-culls every outdoor caster.
    pub(crate) suns: usize,
    /// Re-preparations of the sky dome's material.
    pub(crate) sky_materials: usize,
    /// Re-uploads of the shared sky-lighting texture ([`SKY_LIGHTING_IMAGE`]),
    /// which every lit surface in the world samples.
    pub(crate) sky_lighting: usize,
    /// Re-preparations of a [`TerrainMaterial`]. The sky must never cause one:
    /// the lighting lives in the shared texture above precisely so a moving day
    /// cycle costs one small upload rather than a bind group per region.
    pub(crate) terrain_materials: usize,
}

impl FrameWrites {
    /// Whether the frame wrote anything at all.
    pub(crate) const fn any(self) -> bool {
        self.ambient > 0
            || self.suns > 0
            || self.sky_materials > 0
            || self.sky_lighting > 0
            || self.terrain_materials > 0
    }
}

/// One frame of the fixture.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Frame {
    /// The quantised day position this frame rendered.
    pub(crate) position: f32,
    /// Whether the day cycle entered a **new** sampling cell this frame. A frame
    /// that did is entitled to rewrite everything the sky drives; a frame that
    /// did not must write nothing.
    pub(crate) stepped: bool,
    /// What the frame wrote.
    pub(crate) writes: FrameWrites,
}

/// A headless scene app whose day cycle advances by a fixed slice of region time
/// each frame.
///
/// It runs the crate's real [`SkyPlugin`] — the dome, the sun and its shadow-free
/// mirror, the discs, the clouds, the stars and the ambient — plus the tone
/// mapper's per-frame refresh, over a camera at ground level. No window, no
/// renderer, no GPU, no login.
pub(crate) struct DayCycle {
    /// The app being stepped.
    app: App,
    /// The region time (seconds since the Unix epoch) the next frame samples.
    now: f64,
    /// How far region time advances per frame.
    step: f64,
    /// The bit pattern of the day position the previous frame sampled, which is
    /// what the guards downstream compare — so "the same cell" is decided the
    /// same way here as it is there.
    previous: Option<u32>,
}

impl DayCycle {
    /// A scene app over `settings`, advancing region time by one
    /// `frames_per_cell`-th of a day-position sampling cell each frame.
    ///
    /// The step is expressed in cells rather than seconds because that is the
    /// unit the assertions are about: `frames_per_cell` frames in a row should
    /// resolve one sky and write nothing after the first.
    pub(crate) fn new(settings: EnvironmentSettings, frames_per_cell: u32) -> Self {
        let day_length = f64::from(settings.day_length.max(1));
        let mut cycle = Self {
            app: build_app(&settings),
            now: day_length * DAWN,
            step: day_length / DAY_POSITION_STEPS / f64::from(frames_per_cell.max(1)),
            previous: None,
        };
        for _frame in 0..PRIME_FRAMES {
            let _primed = cycle.step();
        }
        cycle
    }

    /// Give the scene `count` regions' worth of terrain, through the real
    /// `ensure_region` so each material is bound the way a region's is.
    ///
    /// Terrain is not part of the sky fold and no system in this app touches it
    /// again, which is the point: any [`FrameWrites::terrain_materials`] a later
    /// frame reports came from the day cycle.
    pub(crate) fn with_terrain(mut self, count: u32) -> Self {
        let world = self.app.world_mut();
        world.resource_scope(|world, mut state: Mut<'_, TerrainState>| {
            world.resource_scope(|world, mut textures: Mut<'_, TerrainTextures>| {
                world.resource_scope(|world, mut images: Mut<'_, Assets<Image>>| {
                    let mut materials = world.resource_mut::<Assets<TerrainMaterial>>();
                    for region in 0..count {
                        ensure_region(
                            &mut state,
                            &mut textures,
                            RegionHandle(u64::from(region)),
                            &mut images,
                            &mut materials,
                        );
                    }
                });
            });
        });
        // The creations themselves are `Added` events, but settle a frame anyway
        // so a caller's first sample is an ordinary one.
        let _settled = self.step();
        self
    }

    /// Advance region time by one step and run a frame.
    pub(crate) fn step(&mut self) -> Frame {
        let (day_length, day_offset) = {
            let environment = self.app.world().resource::<EnvironmentState>();
            (
                environment.settings.day_length,
                environment.settings.day_offset,
            )
        };
        let position = quantised_day_position(self.now, day_length, day_offset);
        self.app
            .world_mut()
            .resource_mut::<EnvironmentState>()
            .pinned_day_position = Some(position);
        self.app
            .world_mut()
            .resource_mut::<Time>()
            .advance_by(Duration::from_secs_f64(self.step));
        self.now += self.step;
        self.app.update();

        let stepped = self.previous != Some(position.to_bits());
        self.previous = Some(position.to_bits());
        Frame {
            position,
            stepped,
            writes: *self.app.world().resource::<FrameWrites>(),
        }
    }

    /// Mark every terrain material modified by hand, exactly as a system that
    /// wrote one would, and say how many there were.
    ///
    /// The fixture's terrain assertion is a zero, and a zero that could never
    /// have been anything else says nothing — so this is how that assertion
    /// proves its own recorder is wired up.
    pub(crate) fn touch_terrain_materials(&mut self) -> usize {
        let mut materials = self
            .app
            .world_mut()
            .resource_mut::<Assets<TerrainMaterial>>();
        let ids: Vec<_> = materials.ids().collect();
        for id in &ids {
            if let Some(mut material) = materials.get_mut(*id) {
                // A mutable borrow is the whole write: `Assets::get_mut` queues
                // the `Modified` event on deref, not on the value changing.
                let _touched: &mut TerrainMaterial = &mut material;
            }
        }
        ids.len()
    }

    /// Run `count` frames and hand back what each of them did.
    pub(crate) fn run(&mut self, count: u32) -> Vec<Frame> {
        let mut frames = Vec::new();
        for _frame in 0..count {
            frames.push(self.step());
        }
        frames
    }

    /// The ambient light the app currently holds.
    pub(crate) fn ambient(&self) -> GlobalAmbientLight {
        self.app.world().resource::<GlobalAmbientLight>().clone()
    }

    /// The sky the app is rendering at ground level right now, if one resolves.
    pub(crate) fn rendered_sky(&self) -> Option<sl_client_bevy::SkySettings> {
        self.app
            .world()
            .resource::<EnvironmentState>()
            .rendered_sky()
    }

    /// The tone-mapper settings on the app's camera.
    pub(crate) fn tonemap(&mut self) -> Option<SlTonemap> {
        self.app
            .world_mut()
            .query_filtered::<&SlTonemap, With<ViewerCamera>>()
            .iter(self.app.world())
            .next()
            .copied()
    }
}

/// Stand up the app [`DayCycle`] drives: the asset stores the scene systems
/// write into, the resources they read, the real [`SkyPlugin`], the tone
/// mapper's per-frame refresh, a ground-level camera, and the recorder.
///
/// The environment arrives the way a region's does — as an
/// [`SlSessionEvent::Environment`] folded in by the real [`ingest_environment`]
/// — rather than being written into the resource, so what the fixture renders is
/// what the grid's own reply would have produced.
fn build_app(settings: &EnvironmentSettings) -> App {
    let mut app = App::new();
    app.add_plugins((TaskPoolPlugin::default(), AssetPlugin::default()));
    app.init_asset::<Mesh>()
        .init_asset::<Image>()
        .init_asset::<SkyMaterial>()
        .init_asset::<CloudMaterial>()
        .init_asset::<StarMaterial>()
        .init_asset::<SunDiscMaterial>()
        .init_asset::<TerrainMaterial>();
    app.init_resource::<Time>()
        .init_resource::<EnvironmentState>()
        .init_resource::<TerrainState>()
        .init_resource::<TerrainTextures>()
        .init_resource::<TextureManager>()
        .init_resource::<DecodedTextures>()
        .init_resource::<ExposureRange>()
        .init_resource::<RenderOverrides>()
        .init_resource::<FrameWrites>();

    let mut viewer_settings = ViewerSettings::from_store_for_test(SettingsStore::new());
    crate::tonemap::register_settings(&mut viewer_settings);
    app.insert_resource(viewer_settings);

    app.add_message::<TextureDecoded>();
    app.add_message::<SlEvent>();
    app.add_plugins(SkyPlugin);
    // Unordered against the sky fold, exactly as the viewer's own schedule
    // leaves them: `refresh_tonemap_settings` may read a frame-old
    // `ExposureRange`, and a fixture that quietly ordered that lag away would be
    // testing a schedule the viewer does not run. Nothing here samples a single
    // frame, so the lag cannot reach an assertion.
    app.add_systems(Update, (ingest_environment, refresh_tonemap_settings));
    app.add_systems(Last, record_writes);

    // The camera the sky is drawn around, at ground level, carrying the tone
    // mapper's per-view settings like the viewer's own main camera.
    app.world_mut().spawn((
        ViewerCamera,
        Transform::default(),
        GlobalTransform::default(),
        SlTonemap::default(),
    ));
    app.world_mut()
        .write_message(SlEvent(SlSessionEvent::Environment(Box::new(
            settings.clone(),
        ))));
    app
}

/// Record what this frame wrote into [`FrameWrites`].
///
/// In `Last` so it sees the whole frame, and specifically after `PostUpdate`'s
/// `AssetEventSystems`, which is where `Assets::<A>::asset_events` turns the
/// queued [`AssetEvent`]s into readable messages.
fn record_writes(
    mut writes: ResMut<FrameWrites>,
    ambient: Res<GlobalAmbientLight>,
    suns: Query<(Ref<'static, Transform>, Ref<'static, DirectionalLight>), With<SceneSun>>,
    mut sky_materials: MessageReader<AssetEvent<SkyMaterial>>,
    mut images: MessageReader<AssetEvent<Image>>,
    mut terrain: MessageReader<AssetEvent<TerrainMaterial>>,
) {
    // Every reader is drained with `count()` rather than short-circuited with
    // `any()`: a message left unread this frame would be reported by the next
    // one, which is the sort of off-by-one frame a fixture must not have.
    *writes = FrameWrites {
        ambient: usize::from(ambient.is_changed()),
        suns: suns
            .iter()
            .filter(|(transform, light)| transform.is_changed() || light.is_changed())
            .count(),
        sky_materials: sky_materials
            .read()
            .filter(|event| matches!(**event, AssetEvent::Modified { .. }))
            .count(),
        sky_lighting: images
            .read()
            .filter(|event| {
                matches!(**event, AssetEvent::Modified { id } if id == SKY_LIGHTING_IMAGE.id())
            })
            .count(),
        terrain_materials: terrain
            .read()
            .filter(|event| matches!(**event, AssetEvent::Modified { .. }))
            .count(),
    };
}

#[cfg(test)]
mod tests {
    #![expect(
        clippy::expect_used,
        reason = "a failed expectation is the intended failure signal in a unit test"
    )]

    use bevy::light::GlobalAmbientLight;
    use pretty_assertions::assert_eq;

    use super::{DayCycle, Frame, eep_day_cycle, moving_day_cycle};
    use crate::tonemap::DEFAULT_TONEMAP_MIX;

    /// Frames per day-position sampling cell. Four is enough for "several frames
    /// resolve one sky" to mean something while keeping the runs short; on a
    /// four-hour day a cell is 0.44 s, so this is a ~9 fps sampling of it.
    const FRAMES_PER_CELL: u32 = 4;

    /// Long enough to cross several cells at [`FRAMES_PER_CELL`], so the run
    /// contains both kinds of frame.
    const RUN: u32 = 40;

    /// The most frames of a [`RUN`] that may write anything: `RUN /
    /// FRAMES_PER_CELL` sampling cells, plus one for the cell the run starts
    /// part-way through. Spelled as a literal because the workspace denies
    /// unchecked integer arithmetic; it is 40 / 4 + 1.
    const WRITE_BUDGET: usize = 11;

    /// How many of `frames` entered a new sampling cell.
    fn stepped(frames: &[Frame]) -> usize {
        frames.iter().filter(|frame| frame.stepped).count()
    }

    /// How many of `frames` wrote anything the sky drives.
    fn wrote(frames: &[Frame]) -> usize {
        frames.iter().filter(|frame| frame.writes.any()).count()
    }

    /// **The scene settles between day-cycle steps.**
    ///
    /// The regression this pins is the one `viewer-audit-scene-change-guards-day-cycle`
    /// described: with the day position advancing continuously, every
    /// float-equality guard below the sky misses on every frame, so the ambient,
    /// the sun's transform, the dome material and the shared sky-lighting texture
    /// are all rewritten sixty times a second forever. Quantising the shared
    /// input holds them bit-identical between steps — and *this* is the assertion
    /// that a pure test of the quantiser cannot make, because what has to not
    /// happen is a write.
    ///
    /// It is stated as a **budget over region time**, not as "a frame whose
    /// pinned position repeated wrote nothing": the fixture pins the position the
    /// crate's own quantiser returns, so a quantiser that stopped quantising
    /// would give every frame a fresh position and make the second phrasing
    /// vacuous. Region time advances by a fixed slice whatever the quantiser
    /// does, so the budget holds it to account.
    #[test]
    fn the_scene_writes_nothing_between_day_cycle_steps() {
        let mut cycle = DayCycle::new(moving_day_cycle(), FRAMES_PER_CELL);
        let frames = cycle.run(RUN);
        assert!(
            wrote(&frames) <= WRITE_BUDGET,
            "{} of {RUN} frames wrote something over {RUN} frames of region \
             time spanning {} sampling cells",
            wrote(&frames),
            stepped(&frames),
        );
        // The same statement per frame, which additionally catches a guard that
        // misses on an input it has already seen (an unconditional write where a
        // `set_if_neq` used to be).
        for frame in frames.iter().filter(|frame| !frame.stepped) {
            assert!(
                !frame.writes.any(),
                "a frame inside one sampling cell (day position {}) wrote {:?}",
                frame.position,
                frame.writes,
            );
        }
    }

    /// **…and the sky still moves.** The counterpart to the assertion above: a
    /// scene that settled because the day cycle stopped advancing would pass it
    /// trivially, so a frame that *does* step has to relight the world — through
    /// each of the channels the recorder watches, so a silent one cannot make the
    /// budget above look good.
    #[test]
    fn a_new_sampling_cell_relights_the_scene() {
        let mut cycle = DayCycle::new(moving_day_cycle(), FRAMES_PER_CELL);
        let frames = cycle.run(RUN);
        assert!(
            stepped(&frames) >= 4,
            "{RUN} frames at {FRAMES_PER_CELL} per cell should cross several \
             sampling cells, crossed {}",
            stepped(&frames),
        );
        assert!(
            frames.iter().any(|frame| frame.writes.sky_lighting > 0),
            "no frame re-uploaded the shared sky lighting over {} cells",
            stepped(&frames),
        );
        assert!(
            frames.iter().any(|frame| frame.writes.sky_materials > 0),
            "no frame re-prepared the sky dome's material over {} cells",
            stepped(&frames),
        );
        assert!(
            frames.iter().any(|frame| frame.writes.ambient > 0),
            "no frame rewrote the ambient over {} cells",
            stepped(&frames),
        );
    }

    /// **The day cycle never re-prepares a terrain material.**
    ///
    /// A region's [`TerrainMaterial`](sl_client_bevy::TerrainMaterial) binds the
    /// shared sky-lighting texture instead of carrying the sky, so relighting the
    /// ground is one 2×1 texture upload however many regions are loaded. Before
    /// that, `drive_terrain_lighting` compared `Vec3` colours by float equality
    /// and marked **every region's** material modified every frame. This is the
    /// app-level statement of why that cannot come back.
    #[test]
    fn the_day_cycle_never_re_prepares_a_terrain_material() {
        let mut cycle = DayCycle::new(moving_day_cycle(), FRAMES_PER_CELL).with_terrain(4);
        let frames = cycle.run(RUN);
        for frame in &frames {
            assert_eq!(
                frame.writes.terrain_materials, 0,
                "the day cycle re-prepared {} terrain material(s) at day position {}",
                frame.writes.terrain_materials, frame.position,
            );
        }
        // The sky did relight the ground over the same run — through the one
        // shared texture, which is the whole point.
        assert!(
            frames.iter().any(|frame| frame.writes.sky_lighting > 0),
            "the run never re-uploaded the shared sky lighting, so the zero \
             above says nothing",
        );
        // …and the zero is a real observation rather than a channel that never
        // reports: written by hand, the same materials are counted.
        let touched = cycle.touch_terrain_materials();
        assert_eq!(touched, 4, "the fixture should have built four regions");
        assert_eq!(
            cycle.step().writes.terrain_materials,
            touched,
            "the fixture does not see a terrain material being written, so its \
             zero above proves nothing",
        );
    }

    /// **The ambient is this frame's sky, not a function of the frames before
    /// it.**
    ///
    /// `viewer-audit-probe-ambient-multiply` had a `PostUpdate` system
    /// multiplying [`GlobalAmbientLight`] by the probe share *after* the sky had
    /// written the absolute value it wanted. Two consequences, both app-level:
    /// the resource was dirtied on every frame of every app that added the probe
    /// plugin (the multiply was unconditional, so it dirtied the resource even at
    /// the idempotent `0.0` default — which is why nobody noticed), and on the
    /// frames `drive_sky` early-returns the multiply ran unopposed and the
    /// ambient decayed geometrically toward zero.
    ///
    /// So: the ambient holds still while the sky frame does, and it equals what
    /// this frame's sky asks for rather than anything accumulated. That the share
    /// itself is applied *proportionally* is the pure half, tested on
    /// `sky_ambient_light` — the knob is a process-wide `OnceLock` over an
    /// environment variable, and this workspace does not `set_var` in tests.
    #[test]
    fn the_ambient_holds_its_value_while_the_sky_frame_does() {
        let mut cycle = DayCycle::new(moving_day_cycle(), FRAMES_PER_CELL);
        let before = cycle.ambient();
        let frames = cycle.run(RUN);
        for frame in frames.iter().filter(|frame| !frame.stepped) {
            assert!(
                frame.writes.ambient == 0,
                "the ambient was rewritten inside one sampling cell (day \
                 position {})",
                frame.position,
            );
        }
        // And the value the app arrived at is the absolute one the sky in force
        // asks for — recomputed here from scratch, so an ambient that had been
        // scaled once per frame for {RUN} frames could not match it.
        let sky = cycle
            .rendered_sky()
            .expect("the preset cycle resolves a sky at every position");
        let expected = crate::sky::sky_ambient_light(
            crate::sky::resolve_sky(&sky).ambient,
            crate::probes::probe_ambient_scale(),
        );
        let actual = cycle.ambient();
        assert_eq!(actual.color, expected.0, "ambient tint");
        assert_eq!(
            actual.brightness.to_bits(),
            expected.1.to_bits(),
            "ambient brightness {} is not the value this frame's sky asks for \
             ({})",
            actual.brightness,
            expected.1,
        );
        // Nothing about the starting ambient leaked into it either: the app was
        // built with a stated zero (`SkyPlugin`), not Bevy's 80-nit default.
        assert!(
            before.brightness.is_finite(),
            "the ambient the app started from is a real number",
        );
    }

    /// The ambient an app with no sky at all holds: a stated zero rather than
    /// Bevy's 80-nit default, so a world between login and its first
    /// `EnvironmentSettings` does not flash a flat fill the sky then takes away.
    #[test]
    fn an_app_without_a_sky_starts_at_a_stated_zero() {
        let mut app = bevy::app::App::new();
        app.add_plugins((
            bevy::app::TaskPoolPlugin::default(),
            bevy::asset::AssetPlugin::default(),
        ));
        bevy::asset::AssetApp::init_asset::<bevy::image::Image>(&mut app);
        bevy::asset::AssetApp::init_asset::<bevy::mesh::Mesh>(&mut app);
        bevy::asset::AssetApp::init_asset::<sl_client_bevy::SkyMaterial>(&mut app);
        bevy::asset::AssetApp::init_asset::<sl_client_bevy::CloudMaterial>(&mut app);
        bevy::asset::AssetApp::init_asset::<sl_client_bevy::StarMaterial>(&mut app);
        bevy::asset::AssetApp::init_asset::<sl_client_bevy::SunDiscMaterial>(&mut app);
        app.add_plugins(crate::sky::SkyPlugin);
        let ambient = app.world().resource::<GlobalAmbientLight>();
        assert_eq!(
            ambient.brightness.to_bits(),
            0.0_f32.to_bits(),
            "SkyPlugin should state the pre-sky ambient, not inherit Bevy's",
        );
    }

    /// **A legacy sky reaches the tone mapper as `no_post`, every frame of the
    /// cycle.**
    ///
    /// `viewer-audit-tonemap-legacy-sky`: the reference exempts a classic sky
    /// from the tone mapper entirely, and both aditi and the local OpenSim serve
    /// classic skies — so this is the path nearly every capture takes. The
    /// arithmetic is pure and tested (`is_classic_sky`, `effective_tonemap_mix`),
    /// but the *wiring* is three systems apart: `drive_sky` publishes
    /// [`ExposureRange`](crate::exposure::ExposureRange) from the blended sky,
    /// and `refresh_tonemap_settings` turns that into the camera's `no_post` and
    /// mix. Nothing but an app can say those two agree.
    #[test]
    fn a_legacy_sky_reaches_the_tone_mapper_as_no_post() {
        let mut cycle = DayCycle::new(moving_day_cycle(), FRAMES_PER_CELL);
        for _frame in 0..RUN {
            let frame = cycle.step();
            let tonemap = cycle
                .tonemap()
                .expect("the fixture's camera carries the tone-mapper settings");
            assert_eq!(
                tonemap.no_post, 1,
                "a legacy sky at day position {} should take the reference's \
                 NO_POST path",
                frame.position,
            );
            assert!(
                tonemap.tonemap_mix.abs() < 1e-6,
                "a legacy sky at day position {} should mix in none of the tone \
                 curve, got {}",
                frame.position,
                tonemap.tonemap_mix,
            );
        }
    }

    /// …and an EEP sky does not, so the exemption tracks the sky rather than
    /// sticking. The live `tonemap_mix` field is re-derived from source every
    /// frame precisely so a classic sky's zero cannot survive the sky changing.
    #[test]
    fn an_eep_sky_is_tone_mapped() {
        let mut cycle = DayCycle::new(eep_day_cycle(), FRAMES_PER_CELL);
        let _frames = cycle.run(RUN);
        let tonemap = cycle
            .tonemap()
            .expect("the fixture's camera carries the tone-mapper settings");
        assert_eq!(
            tonemap.no_post, 0,
            "an EEP sky should not take the NO_POST path",
        );
        assert!(
            (tonemap.tonemap_mix - DEFAULT_TONEMAP_MIX).abs() < 1e-6,
            "an EEP sky should take the stored RenderTonemapMix, got {}",
            tonemap.tonemap_mix,
        );
    }
}
