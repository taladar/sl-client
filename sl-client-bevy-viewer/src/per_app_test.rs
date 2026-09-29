//! **Two viewers in one process** ([[viewer-automation-per-app-state]]): two
//! whole viewer Apps, each built by [`ViewerAppBuilder`], logged into one
//! loopback grid as two accounts and stepped side by side on this thread — the
//! shape the automation tier's in-process backend runs.
//!
//! What it holds to: each App sees its own agent, keeps its own directories and
//! settings (a setting changed in one is not in the other, on disk or in
//! memory), carries its own render overrides, is attributable in the logs
//! (every line its systems and its session threads write carries its `viewer`
//! span, though the subscriber is one for the process), and can be logged out
//! alone.

use core::time::Duration;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Instant;

use bevy::prelude::*;
use pretty_assertions::assert_eq;
use sl_client_bevy::{LoginParams, LoginRequest, SlIdentity, StartLocation};
use sl_fake_grid::{AccountConfig, FakeGridBuilder, RegionConfig};
use sl_settings::{Scope, SettingValue};
use tracing_subscriber::layer::{Context, SubscriberExt as _};
use tracing_subscriber::registry::LookupSpan;

use crate::assembly::{
    Automation, MediaRuntime, Storage, ViewerApp, ViewerAppBuilder, ViewerAppOptions, ViewerPaths,
    WindowMode,
};
use crate::render_overrides::{ProbeOverrides, RenderOverrides};
use crate::render_readback::{PipelineStatus, gpu_lock};
use crate::render_test::TestError;
use crate::session::TerminationFlag;
use crate::settings::ViewerSettings;

/// The longest the pair may take to log in, or to log out.
const WAIT: Duration = Duration::from_secs(60);

/// How long to sleep between rounds of frames: the logins run on other
/// threads, and a loop that never sleeps starves them.
const FRAME_PAUSE: Duration = Duration::from_millis(2);

/// The most frames each App steps at the end while its pipelines finish
/// compiling (see the full-stack harness's `Drop`).
const DRAIN_FRAMES: u32 = 2000;

/// The password both accounts share on the loopback grid.
const PASSWORD: &str = "password";

/// One log line: the `viewer` span it was logged inside (if any), and its
/// target.
type Line = (Option<String>, String);

/// Every line logged, with the `viewer` span each was logged inside.
#[derive(Clone, Default)]
struct ViewerLines(Arc<Mutex<Vec<Line>>>);

/// The value of a `viewer` span's `name` field, kept in the span's extensions.
struct ViewerName(String);

/// Reads the `name` field of a `viewer` span.
struct NameVisitor(Option<String>);

impl tracing::field::Visit for NameVisitor {
    fn record_str(&mut self, field: &tracing::field::Field, value: &str) {
        if field.name() == "name" {
            self.0 = Some(value.to_owned());
        }
    }

    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn core::fmt::Debug) {
        if field.name() == "name" {
            self.0 = Some(format!("{value:?}"));
        }
    }
}

impl<S: tracing::Subscriber + for<'a> LookupSpan<'a>> tracing_subscriber::Layer<S> for ViewerLines {
    fn on_new_span(
        &self,
        attrs: &tracing::span::Attributes<'_>,
        id: &tracing::span::Id,
        ctx: Context<'_, S>,
    ) {
        if attrs.metadata().name() != "viewer" {
            return;
        }
        let mut visitor = NameVisitor(None);
        attrs.record(&mut visitor);
        if let (Some(name), Some(span)) = (visitor.0, ctx.span(id)) {
            span.extensions_mut().insert(ViewerName(name));
        }
    }

    fn on_event(&self, event: &tracing::Event<'_>, ctx: Context<'_, S>) {
        let viewer = ctx.event_scope(event).and_then(|scope| {
            scope.from_root().find_map(|span| {
                span.extensions()
                    .get::<ViewerName>()
                    .map(|name| name.0.clone())
            })
        });
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push((viewer, event.metadata().target().to_owned()));
    }
}

impl ViewerLines {
    /// The targets each viewer name was seen logging from.
    fn targets_of(&self, viewer: &str) -> BTreeSet<String> {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .iter()
            .filter(|(name, _target)| name.as_deref() == Some(viewer))
            .map(|(_name, target)| target.clone())
            .collect()
    }
}

/// A fresh directory for one viewer's whole tree, under the system temp dir.
fn viewer_root(label: &str) -> Result<PathBuf, TestError> {
    let root = std::env::temp_dir().join(format!(
        "sl-per-app-{}-{label}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_nanos()
    ));
    fs_err::create_dir_all(&root)?;
    Ok(root)
}

/// One test viewer logging in as `first last` under `root`, labelled `label`
/// in the logs, with `overrides` stated.
fn build_viewer(
    login_uri: url::Url,
    (first, last): (&str, &str),
    root: &Path,
    label: &str,
    overrides: RenderOverrides,
) -> Result<ViewerApp, TestError> {
    let params = LoginParams {
        login_uri,
        request: LoginRequest::new(
            first,
            last,
            PASSWORD,
            StartLocation::region(
                RegionConfig::default().name,
                sl_proto::RegionCoordinates::new(128.0, 128.0, 26.0),
            ),
            "sl-per-app-test",
            "0.0",
        ),
    };
    let mut options = ViewerAppOptions::new(params);
    options.window = WindowMode::Windowless;
    options.storage = Storage::Directories(ViewerPaths::under(root));
    options.audio_device = false;
    options.media = MediaRuntime::OFF;
    options.render_overrides = Some(overrides);
    options.avatar_overrides = Some(crate::avatar_overrides::AvatarOverrides::default());
    options.content.fetch_server_chat_history = false;
    // The pipeline-status recorder the teardown drains on comes with it.
    options.automation = Automation::InProcess;
    options.log_label = Some(label.to_owned());
    let mut viewer = ViewerAppBuilder::from_options(options).build()?;
    // Out of the signal's reach: this test quits each viewer on its own.
    viewer.app_mut().insert_resource(TerminationFlag::own());
    viewer.finish();
    Ok(viewer)
}

/// Step both viewers, a frame each per round, until `done` holds for the pair
/// or [`WAIT`] runs out.
fn step_until(
    viewers: &mut [ViewerApp; 2],
    what: &str,
    mut done: impl FnMut(&[ViewerApp; 2]) -> bool,
) -> Result<(), TestError> {
    let deadline = Instant::now().checked_add(WAIT).ok_or("clock overflow")?;
    loop {
        for viewer in viewers.iter_mut() {
            viewer.update();
        }
        if done(viewers) {
            return Ok(());
        }
        if Instant::now() >= deadline {
            return Err(format!("timed out waiting for {what}").into());
        }
        std::thread::sleep(FRAME_PAUSE);
    }
}

/// Whether `viewer` is logged in and has its account settings loaded.
fn logged_in(viewer: &ViewerApp) -> bool {
    let world = viewer.app().world();
    world.resource::<SlIdentity>().agent_id.is_some()
        && world.resource::<ViewerSettings>().account_loaded()
}

/// The avatar directories under `<base>/<grid>/`.
fn account_dirs(base: &Path) -> BTreeSet<String> {
    let Ok(grids) = fs_err::read_dir(base) else {
        return BTreeSet::new();
    };
    grids
        .filter_map(Result::ok)
        .filter_map(|grid| fs_err::read_dir(grid.path()).ok())
        .flatten()
        .filter_map(Result::ok)
        .map(|avatar| avatar.file_name().to_string_lossy().into_owned())
        // The UUID index beside the avatar directories.
        .filter(|name| !name.starts_with('.'))
        .collect()
}

/// Two viewers in one process log into one grid as two accounts, and nothing
/// of one leaks into the other: agent, directories, settings, overrides, log
/// lines, and quitting.
#[test]
fn two_viewers_in_one_process_keep_to_themselves() -> Result<(), TestError> {
    let _gpu = gpu_lock();
    let lines = ViewerLines::default();
    let _log_guard =
        tracing::subscriber::set_default(tracing_subscriber::registry().with(lines.clone()));

    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;
    let grid = runtime.block_on(
        FakeGridBuilder::new()
            .account(AccountConfig::new("Per", "One", PASSWORD))
            .account(AccountConfig::new("Per", "Two", PASSWORD))
            .event_queue_hold(Duration::from_secs(2))
            .region(crate::full_stack_test::stock_fixture().into_region(RegionConfig::default()))
            .start(),
    )?;
    let mut logins = grid.logins();

    let roots = [viewer_root("one")?, viewer_root("two")?];
    let first = build_viewer(
        grid.login_uri(),
        ("Per", "One"),
        &roots[0],
        "one",
        RenderOverrides::default(),
    )?;
    let second = build_viewer(
        grid.login_uri(),
        ("Per", "Two"),
        &roots[1],
        "two",
        RenderOverrides {
            probes: ProbeOverrides {
                ambient_scale: 0.5,
                ..ProbeOverrides::default()
            },
            ..RenderOverrides::default()
        },
    )?;
    // Each App opens a wgpu device of its own; what that costs is written down
    // in `roadmap/context/automation.md`.
    let mut viewers = [first, second];

    step_until(&mut viewers, "both logins", |viewers| {
        viewers.iter().all(logged_in)
    })?;

    // Each sees its own agent: the one the grid logged in under its name.
    let mut agents = std::collections::HashMap::new();
    while let Ok(notice) = logins.try_recv() {
        let _previous = agents.insert(notice.last_name.clone(), notice.agent_id);
    }
    for (viewer, last) in viewers.iter().zip(["One", "Two"]) {
        let agent = viewer.app().world().resource::<SlIdentity>().agent_id;
        assert_eq!(agent, agents.get(last).copied(), "the agent of Per {last}");
    }

    // Each keeps its own tree: the paths it was given, and the account
    // directory its own login resolved, under its own root only.
    for (viewer, root) in viewers.iter().zip(&roots) {
        let paths = viewer.app().world().resource::<ViewerPaths>();
        assert_eq!(
            paths.global_settings_file(),
            Some(root.join("config/viewer-settings.toml"))
        );
    }
    let [first_accounts, second_accounts] = roots
        .each_ref()
        .map(|root| account_dirs(&root.join("config/accounts")));
    assert_eq!(
        first_accounts,
        BTreeSet::from([sl_account_dirs::avatar_dir_name("Per", "One")])
    );
    assert_eq!(
        second_accounts,
        BTreeSet::from([sl_account_dirs::avatar_dir_name("Per", "Two")])
    );

    // A setting changed in one is changed in that one only — in memory, and on
    // disk once both save.
    let name = crate::tonemap::SETTING_TONEMAP_MIX;
    let default = viewers[1]
        .app()
        .world()
        .resource::<ViewerSettings>()
        .store()
        .get_f32(name)?;
    viewers[0]
        .app_mut()
        .world_mut()
        .resource_mut::<ViewerSettings>()
        .set(Scope::Global, name, SettingValue::F32(0.25));
    for viewer in &viewers {
        viewer.app().world().resource::<ViewerSettings>().save();
    }
    let stored = |viewer: &ViewerApp| {
        viewer
            .app()
            .world()
            .resource::<ViewerSettings>()
            .store()
            .get_f32(name)
    };
    assert_eq!(stored(&viewers[0])?.to_bits(), 0.25_f32.to_bits());
    assert_eq!(stored(&viewers[1])?.to_bits(), default.to_bits());
    let on_disk = |root: &Path| fs_err::read_to_string(root.join("config/viewer-settings.toml"));
    assert!(on_disk(&roots[0])?.contains(name));
    assert!(!on_disk(&roots[1])?.contains(name));

    // Each carries the render overrides it was built with.
    let ambient = |viewer: &ViewerApp| {
        viewer
            .app()
            .world()
            .resource::<RenderOverrides>()
            .probes
            .ambient_scale
    };
    assert_eq!(
        (
            ambient(&viewers[0]).to_bits(),
            ambient(&viewers[1]).to_bits()
        ),
        (0.0_f32.to_bits(), 0.5_f32.to_bits())
    );

    // Each is attributable in the one subscriber: its systems' lines and its
    // session threads' lines both carry its span.
    for label in ["one", "two"] {
        let targets = lines.targets_of(label);
        assert!(
            targets
                .iter()
                .any(|target| target.starts_with("sl_client_bevy")),
            "no session line of viewer {label} carries its span: {targets:?}"
        );
        assert!(
            targets.iter().any(|target| target.starts_with("sl_viewer")),
            "no system line of viewer {label} carries its span: {targets:?}"
        );
    }

    // One quits while the other stays: the first's flag is its own.
    viewers[0]
        .app()
        .world()
        .resource::<TerminationFlag>()
        .raise();
    let deadline = Instant::now().checked_add(WAIT).ok_or("clock overflow")?;
    while viewers[0].app().should_exit().is_none() {
        for viewer in &mut viewers {
            viewer.update();
        }
        assert!(Instant::now() < deadline, "the first viewer never quit");
        std::thread::sleep(FRAME_PAUSE);
    }
    // A logout takes the grid's reply and a frame or two; give the second
    // viewer as long again, and it is still in world.
    for _frame in 0..120 {
        viewers[1].update();
    }
    assert!(
        viewers[1].app().should_exit().is_none(),
        "the second viewer quit with the first"
    );
    assert!(logged_in(&viewers[1]));
    viewers[1]
        .app()
        .world()
        .resource::<TerminationFlag>()
        .raise();
    step_until(&mut viewers, "the second viewer to quit", |viewers| {
        viewers[1].app().should_exit().is_some()
    })?;

    // Nothing compiling when the test returns (see the full-stack harness's
    // `Drop` for what an unfinished pipeline compile does at process exit).
    for viewer in &mut viewers {
        for _frame in 0..DRAIN_FRAMES {
            let waiting = viewer
                .app()
                .world()
                .get_resource::<PipelineStatus>()
                .map_or(0, PipelineStatus::waiting);
            if waiting == 0 {
                break;
            }
            viewer.update();
        }
    }
    drop(viewers);
    drop(grid);
    for root in &roots {
        fs_err::remove_dir_all(root)?;
    }
    Ok(())
}
