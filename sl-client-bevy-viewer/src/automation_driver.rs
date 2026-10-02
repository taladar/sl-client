//! The **driver's acceptance**: one test body, written against
//! `sl_viewer_driver` alone, drives two headless viewers logged into one fake
//! grid — one reached over an in-process link, the other over its automation
//! socket — and passes on both: the login waited for, a floater opened by its
//! toolbar button and closed by its close button, each waited for with an
//! expectation, and an object's pie opened by a pick-verified right click. A deliberately failing expectation then leaves its three
//! artifacts (screenshot, tree, event tail) and names them in its message.
//!
//! Both viewers are Apps on an `InProcessHost`, each stepped on a thread of
//! its own as a process runs; what differs is only how the driver reaches
//! each.

#[cfg(test)]
mod full_stack {
    use core::time::Duration;
    use std::path::{Path, PathBuf};

    use bevy::prelude::*;
    use pretty_assertions::assert_eq;
    use serde_json::json;
    use sl_automation_proto::{
        AutomationError, Locator, LogStream, Probe, Role, WaitCondition, WorldKind, WorldLocator,
    };
    use sl_client_bevy::{LoginParams, LoginRequest, StartLocation};
    use sl_fake_grid::{AccountConfig, FakeGrid, FakeGridBuilder, RegionConfig};
    use sl_viewer_automation::{InProcessHost, ViewerHandle};
    use sl_viewer_driver::{DriverError, Viewer, ViewerOptions};

    use crate::assembly::{
        Automation, MediaRuntime, Storage, ViewerApp, ViewerAppBuilder, ViewerAppOptions,
        WindowMode,
    };
    use crate::full_stack_test::stock_fixture;
    use crate::render_test::TestError;
    use crate::session::TerminationFlag;

    /// The region the accounts start in.
    const HOME: &str = "Fake Region";
    /// The off-screen window's size.
    const WINDOW: UVec2 = UVec2::new(1280, 720);
    /// The password every account shares on the loopback grid.
    const PASSWORD: &str = "password";
    /// Long enough for a login and a cold shader cache.
    const LOGIN: Duration = Duration::from_secs(90);
    /// Short: the expectation that is meant to fail.
    const SHORT: Duration = Duration::from_millis(500);

    /// A grid serving the stock region with an account `Driver <last>` for
    /// each of `accounts`, on its own runtime — which must outlive it.
    fn grid(accounts: &[&str]) -> Result<(tokio::runtime::Runtime, FakeGrid), TestError> {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()?;
        let mut builder = FakeGridBuilder::new()
            // A long hold, so the CAPS long-poll does not compete with the
            // renders for the cores.
            .event_queue_hold(Duration::from_secs(2))
            .region(stock_fixture().into_region(RegionConfig::default()));
        for last in accounts {
            builder = builder.account(AccountConfig::new("Driver", *last, PASSWORD));
        }
        let grid = runtime.block_on(builder.start())?;
        Ok((runtime, grid))
    }

    /// The login of `Driver <last>` into `grid`.
    fn login(grid: &FakeGrid, last: &str) -> LoginParams {
        LoginParams {
            login_uri: grid.login_uri(),
            request: LoginRequest::new(
                "Driver",
                last,
                PASSWORD,
                StartLocation::region(HOME, sl_proto::RegionCoordinates::new(128.0, 128.0, 26.0)),
                "sl-viewer-driver-test",
                "0.0",
            ),
        }
    }

    /// A headless viewer logging in with `params` as `last`, fed by
    /// `automation`, with a termination flag of its own.
    fn viewer(
        params: LoginParams,
        last: &str,
        automation: Automation,
    ) -> Result<ViewerApp, sl_viewer_automation::BuildError> {
        let mut options = ViewerAppOptions::new(params);
        options.window = WindowMode::Headless {
            size: WINDOW,
            watch: false,
        };
        options.storage = Storage::Ephemeral;
        options.audio_device = false;
        options.media = MediaRuntime::OFF;
        options.render_overrides = Some(crate::render_overrides::RenderOverrides::default());
        options.avatar_overrides = Some(crate::avatar_overrides::AvatarOverrides::default());
        options.content.fetch_server_chat_history = false;
        options.automation = automation;
        options.log_label = Some(last.to_owned());
        let mut viewer = ViewerAppBuilder::from_options(options)
            .build()
            .map_err(|error| error.to_string())?;
        viewer.app_mut().insert_resource(TerminationFlag::own());
        viewer.finish();
        Ok(viewer)
    }

    /// **The test body**, through the driver alone: the login, a floater
    /// opened by a click and closed by one, each checked with an expectation,
    /// and the click found in the event log.
    async fn open_and_close_the_inventory(viewer: &Viewer) -> Result<(), DriverError> {
        let _arrived = viewer
            .expect_state(Probe::Agent)
            .at("/region/name")
            .timeout(LOGIN)
            .to_equal(json!(HOME))
            .await?;
        viewer.wait_until_quiet(LOGIN).await?;
        let identity = viewer.hello().await?;
        assert_eq!(
            identity.agent_name.as_deref(),
            Some(format!("Driver {}", viewer.label()).as_str()),
            "each viewer is its own agent"
        );

        let mut events = viewer.events().await?;
        let ui = viewer.ui();
        let inventory = ui.window("inventory");
        let _clicked = ui
            .test_id("bottom-toolbar-button:toggle-inventory")
            .click()
            .await?;
        let shown = viewer.expect(&inventory).to_be_visible().await?;
        assert_eq!(shown.len(), 1, "one inventory window");
        assert!(
            !events.read(&[LogStream::UiAction]).await?.is_empty(),
            "the toolbar click is in the event log"
        );
        assert!(inventory.is_visible().await?);

        let _closed = inventory.test_id("floater-button:close").click().await?;
        let _hidden = viewer.expect(&inventory).to_be_hidden().await?;

        // A world handle: the stock box's pie, opened by a pick-verified
        // right click, and shut again by Escape.
        let stock_box = viewer.world().locator(WorldLocator::full_id(
            sl_fake_grid::scenario::stock_scripted_object().uuid(),
        ));
        let pied = stock_box.open_pie().await?;
        assert_eq!(pied.kind, WorldKind::Object, "the box, not the avatar");
        let pie = viewer.locator(Locator {
            role: Some(Role::Menu),
            ..Locator::test_id("pie-menu")
        });
        let _open = viewer.expect(&pie).to_be_visible().await?;
        viewer.press("Escape").await?;
        let _shut = viewer.expect(&pie).to_be_detached().await?;
        Ok(())
    }

    /// **The failure**: an expectation on a button the window does not have
    /// times out, and its message names a screenshot, the tree around the
    /// window and the event tail — all saved under `artifacts`.
    async fn a_failing_expectation_leaves_its_artifacts(
        viewer: &Viewer,
        artifacts: &Path,
    ) -> Result<(), TestError> {
        let missing = viewer.ui().window("inventory").button_key("no-such-button");
        let Err(error) = viewer.expect(&missing).timeout(SHORT).to_be_visible().await else {
            return Err("an expectation on a missing button held".into());
        };
        assert!(
            matches!(
                error.automation_error(),
                Some(AutomationError::TimedOut {
                    condition: Some(WaitCondition::Visible),
                    ..
                })
            ),
            "{error}"
        );
        let failure = error.failure().ok_or("not a viewer failure")?;
        let message = error.to_string();
        for (artifact, name) in [
            (&failure.artifacts.screenshot, "screenshot.png"),
            (&failure.artifacts.tree, "tree.txt"),
            (&failure.artifacts.events, "events.txt"),
        ] {
            let path = artifact
                .clone()
                .ok_or_else(|| format!("no {name}"))?
                .map_err(|reason| format!("{name} was not saved: {reason}"))?;
            assert!(path.starts_with(artifacts), "{}", path.display());
            assert!(path.ends_with(name), "{}", path.display());
            assert!(
                message.contains(&path.display().to_string()),
                "the message names the {name}: {message}"
            );
        }
        let screenshot = failure
            .artifacts
            .screenshot
            .clone()
            .ok_or("no screenshot")??;
        assert_eq!(
            fs_err::read(screenshot)?.get(1..4),
            Some(b"PNG".as_slice()),
            "the screenshot is a PNG"
        );
        let tree = failure.artifacts.tree.clone().ok_or("no tree")??;
        assert!(
            fs_err::read_to_string(tree)?.contains("#floater:inventory"),
            "the tree is the excerpt around the window"
        );
        Ok(())
    }

    /// Whether the hosted `viewer` has a GPU to render with.
    async fn has_gpu(
        host: &InProcessHost<ViewerApp>,
        viewer: ViewerHandle,
    ) -> Result<bool, TestError> {
        Ok(host
            .with_app(viewer, |viewer| {
                viewer
                    .app()
                    .world()
                    .contains_resource::<bevy::render::renderer::RenderDevice>()
            })
            .await?)
    }

    /// Raise each viewer's termination flag and wait until each has logged
    /// out and exited.
    async fn log_out(
        host: &InProcessHost<ViewerApp>,
        viewers: &[ViewerHandle],
    ) -> Result<(), TestError> {
        for &viewer in viewers {
            host.with_app(viewer, |viewer| {
                viewer.app().world().resource::<TerminationFlag>().raise();
            })
            .await?;
        }
        for &viewer in viewers {
            tokio::time::timeout(LOGIN, host.exited(viewer)).await??;
        }
        Ok(())
    }

    /// A scratch directory of this test's own, with a short path: a socket
    /// path must stay under about a hundred bytes.
    fn scratch() -> PathBuf {
        std::env::temp_dir().join(format!("sl-driver-{}", std::process::id()))
    }

    /// **The acceptance**: the same body through both transports against one
    /// fake grid, run side by side, then the failure's artifacts.
    #[test]
    fn the_same_test_body_drives_a_viewer_over_either_transport() -> Result<(), TestError> {
        let _gpu = crate::render_readback::gpu_lock();
        let (runtime, grid) = grid(&["Link", "Socket"])?;
        let scratch = scratch();
        fs_err::create_dir_all(&scratch)?;
        let socket = scratch.join("v.sock");
        let (link_login, socket_login) = (login(&grid, "Link"), login(&grid, "Socket"));
        let outcome = runtime.block_on(async {
            let host = InProcessHost::<ViewerApp>::new();
            let (linked, link) = host
                .host("link", move || {
                    viewer(link_login, "Link", Automation::InProcess)
                })
                .await?;
            let (socketed, _unused_link) = {
                let socket = socket.clone();
                host.host("socket", move || {
                    viewer(socket_login, "Socket", Automation::Socket(socket))
                })
                .await?
            };
            if !has_gpu(&host, linked).await? {
                tracing::warn!("no GPU adapter: skipping the driver acceptance");
                log_out(&host, &[linked, socketed]).await?;
                return Ok::<_, TestError>(());
            }
            let over_link = Viewer::over_link(
                link.requests,
                link.messages,
                ViewerOptions::new("Link").with_artifacts(scratch.join("Link")),
            )
            .await?;
            let over_socket = Viewer::connect(
                &socket,
                ViewerOptions::new("Socket").with_artifacts(scratch.join("Socket")),
            )
            .await?;

            let (linked_run, socket_run) = tokio::join!(
                open_and_close_the_inventory(&over_link),
                open_and_close_the_inventory(&over_socket)
            );
            linked_run?;
            socket_run?;
            a_failing_expectation_leaves_its_artifacts(&over_link, &scratch.join("Link")).await?;
            a_failing_expectation_leaves_its_artifacts(&over_socket, &scratch.join("Socket"))
                .await?;

            drop(over_socket);
            drop(over_link);
            log_out(&host, &[linked, socketed]).await?;
            host.stop()?;
            Ok(())
        });
        drop(grid);
        drop(runtime);
        outcome?;
        fs_err::remove_dir_all(&scratch)?;
        Ok(())
    }
}
