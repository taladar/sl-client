//! The **executor over a real session**: the whole viewer logged into the fake
//! grid, rendering into an off-screen window, driven through the automation
//! queue alone — the login waited for, a floater opened, a button in it
//! clicked and the effect waited for, a screenshot taken — as a remote or an
//! in-process transport will drive it. The executor's own teeth, and each
//! failure kind's report, are `sl-viewer-automation`'s; the world requests
//! over the fixture world are in [`crate::automation_world_aim`]. The same
//! session is driven over the automation socket too, as a client in another
//! process would drive it, and through the in-process transport — the same
//! requests, the transport hosting and stepping the viewer — beside a second
//! viewer on the same grid.

#[cfg(test)]
mod full_stack {
    use bevy::prelude::*;
    use pretty_assertions::assert_eq;
    use serde_json::json;
    use sl_automation_proto::{
        Deadline, Locator, LogStream, Probe, Request, RequestBody, RequestId, Response,
        ResponseBody, Role, StateCondition, StateObservation, ValueTest, ViewerMessage,
        WaitCondition,
    };
    use sl_fake_grid::RegionConfig;
    use sl_viewer_automation::AutomationQueue;

    use crate::full_stack_test::{HarnessOptions, ViewerHarness, stock_fixture};
    use crate::render_test::TestError;

    /// The region the account starts in.
    const HOME: &str = "Fake Region";
    /// The off-screen window's size.
    const WINDOW: UVec2 = UVec2::new(1280, 720);
    /// A wait long enough for a login and a cold shader cache.
    const LONG: Deadline = Deadline {
        frames: Some(100_000),
        millis: Some(60_000),
    };

    /// Submit `body` as request `id` and step frames until it is answered.
    fn ask(harness: &mut ViewerHarness, id: u64, body: RequestBody) -> Result<Response, TestError> {
        let id = RequestId(id);
        harness
            .app_world_mut()
            .resource_mut::<AutomationQueue>()
            .submit(Request { id, body });
        harness.run_until("the executor's answer", |harness| {
            harness
                .app_world_mut()
                .resource_mut::<AutomationQueue>()
                .take_response(id)
        })
    }

    /// Whatever carries a request to one viewer and brings its answer back:
    /// the harness's queue, or a transport.
    type Ask<'a> = dyn FnMut(u64, RequestBody) -> Result<Response, TestError> + 'a;

    /// The body of a response that succeeded, or its error with the report.
    fn answered(response: Response) -> Result<ResponseBody, TestError> {
        match response.result {
            Ok(body) => Ok(body),
            Err(error) => {
                Err(format!("the request failed: {error}\n{:#?}", response.report).into())
            }
        }
    }

    /// The inventory window.
    fn inventory_window() -> Locator {
        Locator {
            role: Some(Role::Window),
            ..Locator::test_id("floater:inventory")
        }
    }

    /// A wait for the agent to arrive in `region`, as request `id`.
    fn arrival(id: u64, region: &str) -> Request {
        Request {
            id: RequestId(id),
            body: RequestBody::WaitForState {
                condition: StateCondition::Probe {
                    probe: Probe::Agent,
                    pointer: "/region/name".to_owned(),
                    test: ValueTest::Equals(json!(region)),
                },
                deadline: LONG,
            },
        }
    }

    /// The first half of the acceptance, through `ask` alone: the agent
    /// arrives in its region (request 1), and the scene settles (request 2).
    fn wait_for_login(ask: &mut Ask<'_>) -> Result<(), TestError> {
        let Request { id, body } = arrival(1, HOME);
        let arrived = answered(ask(id.0, body)?)?;
        assert!(
            matches!(arrived, ResponseBody::StateHeld { .. }),
            "{arrived:?}"
        );
        let quiet = answered(ask(
            2,
            RequestBody::WaitForState {
                condition: StateCondition::Quiet,
                deadline: LONG,
            },
        )?)?;
        let ResponseBody::StateHeld {
            observed: StateObservation::Quiet { readout },
        } = quiet
        else {
            return Err(format!("not a quiet scene: {quiet:?}").into());
        };
        assert!(readout.is_quiet());
        Ok(())
    }

    /// The second half, through `ask` alone (requests 3–9): a floater opened
    /// by its toolbar button, the click in the event log, a screenshot with
    /// the window outlined (written under `shot`), and the window's close
    /// button clicked and the window waited hidden.
    fn drive_the_inventory(ask: &mut Ask<'_>, shot: &str) -> Result<(), TestError> {
        // Where the event log stands now: the cursor past everything in it.
        let before = answered(ask(
            3,
            RequestBody::ReadLog {
                cursor: 0,
                streams: vec![LogStream::UiAction],
                limit: None,
            },
        )?)?;
        let ResponseBody::Log { page } = before else {
            return Err(format!("not a log: {before:?}").into());
        };
        let cursor = page.next;

        // A floater, opened by its toolbar button.
        let clicked = answered(ask(
            4,
            RequestBody::Click {
                locator: Locator::test_id("bottom-toolbar-button:toggle-inventory"),
                button: sl_automation_proto::PointerButton::Left,
                double: false,
                deadline: Deadline::default(),
            },
        )?)?;
        assert!(matches!(clicked, ResponseBody::Done { .. }), "{clicked:?}");
        let shown = answered(ask(
            5,
            RequestBody::WaitFor {
                locator: inventory_window(),
                condition: WaitCondition::Visible,
                deadline: Deadline::default(),
            },
        )?)?;
        assert!(
            matches!(&shown, ResponseBody::Satisfied { nodes } if nodes.len() == 1),
            "{shown:?}"
        );
        let logged = answered(ask(
            6,
            RequestBody::ReadLog {
                cursor,
                streams: vec![LogStream::UiAction],
                limit: None,
            },
        )?)?;
        let ResponseBody::Log { page } = logged else {
            return Err(format!("not a log: {logged:?}").into());
        };
        assert!(
            !page.entries.is_empty(),
            "the toolbar click is in the event log as a UI action"
        );

        // A screenshot of it, the window outlined.
        let path = std::env::temp_dir().join(format!("{shot}-{}.png", std::process::id()));
        let screenshot = answered(ask(
            7,
            RequestBody::Screenshot {
                path: path.display().to_string(),
                outline: Some(inventory_window()),
            },
        )?)?;
        let written = fs_err::read(&path);
        let _removed = fs_err::remove_file(&path);
        let ResponseBody::Screenshot {
            width,
            height,
            outlined,
            ..
        } = screenshot
        else {
            return Err(format!("not a screenshot: {screenshot:?}").into());
        };
        assert_eq!((width, height), (WINDOW.x, WINDOW.y));
        assert_eq!(outlined.len(), 1, "the window is outlined");
        assert_eq!(written?.get(1..4), Some(b"PNG".as_slice()));

        // A button in it, and its effect: the window's close button closes it.
        let closed = answered(ask(
            8,
            RequestBody::Click {
                locator: Locator::test_id("floater-button:close").within(inventory_window()),
                button: sl_automation_proto::PointerButton::Left,
                double: false,
                deadline: Deadline::default(),
            },
        )?)?;
        assert!(matches!(closed, ResponseBody::Done { .. }), "{closed:?}");
        let hidden = answered(ask(
            9,
            RequestBody::WaitFor {
                locator: inventory_window(),
                condition: WaitCondition::Hidden,
                deadline: Deadline::default(),
            },
        )?)?;
        assert!(
            matches!(hidden, ResponseBody::Satisfied { .. }),
            "{hidden:?}"
        );
        Ok(())
    }

    /// **The acceptance**: a login, a floater, a button and its effect, all
    /// through requests — nothing but the queue touches the viewer.
    #[test]
    fn a_session_is_driven_through_requests_alone() -> Result<(), TestError> {
        let mut harness = ViewerHarness::start_in_with(
            vec![stock_fixture().into_region(RegionConfig::default())],
            HarnessOptions::in_offscreen_window(WINDOW),
        )?;
        wait_for_login(&mut |id, body| ask(&mut harness, id, body))?;
        if harness.capture()?.is_none() {
            tracing::warn!("no GPU adapter: skipping the executor acceptance");
            return Ok(());
        }
        drive_the_inventory(
            &mut |id, body| ask(&mut harness, id, body),
            "sl-executor-acceptance",
        )?;
        assert_eq!(
            harness
                .app_world_mut()
                .resource_mut::<AutomationQueue>()
                .drain_responses(),
            Vec::new(),
            "every response was taken"
        );
        Ok(())
    }

    /// A client of the automation socket, reading without blocking the
    /// harness's frames.
    struct SocketClient {
        /// Where it writes.
        stream: std::os::unix::net::UnixStream,
        /// Where it reads.
        reader: std::io::BufReader<std::os::unix::net::UnixStream>,
        /// A line read in part.
        partial: String,
    }

    impl SocketClient {
        /// Connect to the socket at `path`.
        fn connect(path: &std::path::Path) -> Result<Self, TestError> {
            let stream = std::os::unix::net::UnixStream::connect(path)?;
            stream.set_nonblocking(true)?;
            let reader = std::io::BufReader::new(stream.try_clone()?);
            Ok(Self {
                stream,
                reader,
                partial: String::new(),
            })
        }

        /// Send `body` as request `id`.
        fn send(&mut self, id: u64, body: RequestBody) -> Result<(), TestError> {
            use std::io::Write as _;
            let mut line = serde_json::to_string(&Request {
                id: RequestId(id),
                body,
            })?;
            line.push('\n');
            self.stream.write_all(line.as_bytes())?;
            Ok(())
        }

        /// A whole line, if one has arrived.
        fn poll(&mut self) -> Result<Option<ViewerMessage>, TestError> {
            use std::io::BufRead as _;
            match self.reader.read_line(&mut self.partial) {
                Ok(0) => Err("the viewer closed the automation connection".into()),
                Ok(_read) if self.partial.ends_with('\n') => {
                    let line = core::mem::take(&mut self.partial);
                    Ok(Some(serde_json::from_str(&line)?))
                }
                Ok(_read) => Ok(None),
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => Ok(None),
                Err(error) => Err(error.into()),
            }
        }

        /// Step frames until a response arrives.
        fn response(&mut self, harness: &mut ViewerHarness) -> Result<Response, TestError> {
            let outcome =
                harness.run_until("an answer on the socket", |_harness| match self.poll() {
                    Ok(Some(ViewerMessage::Response(response))) => Some(Ok(*response)),
                    Ok(Some(ViewerMessage::Notification(notification))) => {
                        Some(Err(format!("an unexpected notification: {notification:?}")))
                    }
                    Ok(None) => None,
                    Err(error) => Some(Err(error.to_string())),
                })?;
            Ok(outcome?)
        }
    }

    /// **The remote transport's acceptance**: a viewer built with an
    /// automation socket answers a hello naming its agent, and a floater
    /// opened over the socket is answered once it is shown — with a wait for
    /// it already in flight, whose answer comes when it holds.
    #[test]
    fn a_session_is_driven_over_the_automation_socket() -> Result<(), TestError> {
        let dir = std::env::temp_dir().join(format!("sl-automation-socket-{}", std::process::id()));
        fs_err::create_dir_all(&dir)?;
        let path = dir.join("viewer.sock");
        let mut harness = ViewerHarness::start_in_with(
            vec![stock_fixture().into_region(RegionConfig::default())],
            HarnessOptions::in_offscreen_window(WINDOW).with_automation_socket(path.clone()),
        )?;
        let mut client = SocketClient::connect(&path)?;

        client.send(
            1,
            RequestBody::WaitForState {
                condition: StateCondition::Probe {
                    probe: Probe::Agent,
                    pointer: "/region/name".to_owned(),
                    test: ValueTest::Equals(json!(HOME)),
                },
                deadline: LONG,
            },
        )?;
        let arrived = client.response(&mut harness)?;
        assert_eq!(arrived.id, RequestId(1));
        assert!(
            matches!(answered(arrived)?, ResponseBody::StateHeld { .. }),
            "the login is waited for over the socket"
        );

        client.send(2, RequestBody::Hello)?;
        let hello = client.response(&mut harness)?;
        let ResponseBody::Hello { protocol, viewer } = answered(hello)? else {
            return Err("not a hello".into());
        };
        assert_eq!(protocol, sl_automation_proto::PROTOCOL_VERSION);
        assert_eq!(viewer.viewer, crate::build_info::VIEWER_NAME);
        assert_eq!(viewer.pid, std::process::id());
        assert!(viewer.grid.is_some(), "{viewer:?}");
        assert!(viewer.agent_name.is_some(), "{viewer:?}");
        assert!(viewer.agent_id.is_some(), "logged in: {viewer:?}");

        // The wait first, so it is in flight when the floater opens.
        client.send(
            3,
            RequestBody::WaitFor {
                locator: inventory_window(),
                condition: WaitCondition::Visible,
                deadline: LONG,
            },
        )?;
        client.send(
            4,
            RequestBody::OpenFloater {
                floater: "inventory".to_owned(),
            },
        )?;
        let opened = client.response(&mut harness)?;
        assert_eq!(opened.id, RequestId(4), "the open is answered first");
        assert!(
            matches!(answered(opened)?, ResponseBody::Opened { .. }),
            "the floater opened"
        );
        let shown = client.response(&mut harness)?;
        assert_eq!(shown.id, RequestId(3));
        assert!(
            matches!(answered(shown)?, ResponseBody::Satisfied { nodes } if nodes.len() == 1),
            "the wait holds once the window is shown"
        );

        drop(harness);
        assert!(!path.exists(), "the socket goes with the viewer");
        fs_err::remove_dir_all(&dir)?;
        Ok(())
    }

    /// **Through the in-process transport**: viewer Apps built by the
    /// viewer's own builder, hosted and stepped by the transport, driven by
    /// the same requests as above.
    mod in_process {
        use core::time::Duration;

        use pretty_assertions::assert_eq;
        use sl_automation_proto::{
            Locator, NodeVisibility, Request, RequestBody, RequestId, Response, ResponseBody,
            WaitCondition,
        };
        use sl_client_bevy::{LoginParams, LoginRequest, StartLocation};
        use sl_fake_grid::{AccountConfig, FakeGrid, FakeGridBuilder, RegionConfig};
        use sl_viewer_automation::{InProcessTransport, ViewerHandle};

        use super::{
            HOME, LONG, WINDOW, answered, arrival, drive_the_inventory, inventory_window,
            wait_for_login,
        };
        use crate::assembly::{
            Automation, MediaRuntime, Storage, ViewerApp, ViewerAppBuilder, ViewerAppOptions,
            WindowMode,
        };
        use crate::full_stack_test::stock_fixture;
        use crate::render_test::TestError;
        use crate::session::TerminationFlag;

        /// The password every account shares on the loopback grid.
        const PASSWORD: &str = "password";

        /// The longest a pair of viewers may take to log out.
        const LOGOUT: Duration = Duration::from_secs(60);

        /// A grid serving the stock region, with an account for each surname
        /// in `accounts` (first name `Transport`), on its own runtime — which
        /// must outlive it.
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
                builder = builder.account(AccountConfig::new("Transport", *last, PASSWORD));
            }
            let grid = runtime.block_on(builder.start())?;
            Ok((runtime, grid))
        }

        /// A headless test viewer logging into `grid` as `Transport <last>`,
        /// labelled `last` in the logs, with the executor fed in process and
        /// a termination flag of its own.
        fn viewer(grid: &FakeGrid, last: &str) -> Result<ViewerApp, TestError> {
            let params = LoginParams {
                login_uri: grid.login_uri(),
                request: LoginRequest::new(
                    "Transport",
                    last,
                    PASSWORD,
                    StartLocation::region(
                        HOME,
                        sl_proto::RegionCoordinates::new(128.0, 128.0, 26.0),
                    ),
                    "sl-in-process-transport-test",
                    "0.0",
                ),
            };
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
            options.automation = Automation::InProcess;
            options.log_label = Some(last.to_owned());
            let mut viewer = ViewerAppBuilder::from_options(options).build()?;
            viewer.app_mut().insert_resource(TerminationFlag::own());
            viewer.finish();
            Ok(viewer)
        }

        /// Whether `viewer` has a GPU to render with: without one, `finish`
        /// publishes no render device.
        fn has_gpu(transport: &InProcessTransport<ViewerApp>, viewer: ViewerHandle) -> bool {
            transport.app(viewer).is_some_and(|viewer| {
                viewer
                    .app()
                    .world()
                    .contains_resource::<bevy::render::renderer::RenderDevice>()
            })
        }

        /// `body` as request `id`.
        const fn request(id: u64, body: RequestBody) -> Request {
            Request {
                id: RequestId(id),
                body,
            }
        }

        /// Quit every viewer by its own flag and step them until each has
        /// exited.
        fn log_out(
            transport: &mut InProcessTransport<ViewerApp>,
            viewers: &[ViewerHandle],
        ) -> Result<(), TestError> {
            for &handle in viewers {
                transport
                    .app(handle)
                    .ok_or("no such viewer")?
                    .app()
                    .world()
                    .resource::<TerminationFlag>()
                    .raise();
            }
            let deadline = std::time::Instant::now()
                .checked_add(LOGOUT)
                .ok_or("clock overflow")?;
            while !viewers.iter().all(|&handle| {
                transport
                    .app(handle)
                    .is_some_and(|viewer| viewer.app().should_exit().is_some())
            }) {
                if std::time::Instant::now() >= deadline {
                    return Err("the viewers never logged out".into());
                }
                transport.step();
                std::thread::sleep(sl_viewer_automation::FRAME_PAUSE);
            }
            Ok(())
        }

        /// **The acceptance, unchanged, through the transport**: the same
        /// requests the queue test sends, and the same answers.
        #[test]
        fn the_executor_acceptance_passes_through_the_in_process_transport() -> Result<(), TestError>
        {
            let _gpu = crate::render_readback::gpu_lock();
            let (_runtime, grid) = grid(&["Solo"])?;
            let mut transport = InProcessTransport::new();
            let solo = transport.host("solo", viewer(&grid, "Solo")?)?;
            if !has_gpu(&transport, solo) {
                tracing::warn!("no GPU adapter: skipping the in-process transport acceptance");
                return Ok(());
            }
            let mut ask = |id: u64, body: RequestBody| -> Result<Response, TestError> {
                Ok(transport.request(solo, request(id, body))?)
            };
            wait_for_login(&mut ask)?;
            drive_the_inventory(&mut ask, "sl-in-process-acceptance")?;
            log_out(&mut transport, &[solo])?;
            drop(transport);
            drop(grid);
            Ok(())
        }

        /// **Two viewers in one process, one grid, one transport**: each
        /// answers for its own agent under the same ids, and a wait in flight
        /// on one is not satisfied by what the other does.
        #[test]
        fn two_viewers_on_one_grid_answer_through_one_transport() -> Result<(), TestError> {
            let _gpu = crate::render_readback::gpu_lock();
            let (_runtime, grid) = grid(&["One", "Two"])?;
            let mut logins = grid.logins();
            let mut transport = InProcessTransport::new();
            let one = transport.host("one", viewer(&grid, "One")?)?;
            let two = transport.host("two", viewer(&grid, "Two")?)?;
            if !has_gpu(&transport, one) {
                tracing::warn!("no GPU adapter: skipping the two-viewer transport test");
                return Ok(());
            }

            // Both logins waited for at once, under the same id.
            for handle in [one, two] {
                transport.send(handle, arrival(1, HOME))?;
            }
            for handle in [one, two] {
                let arrived = answered(transport.response(handle, RequestId(1))?)?;
                assert!(
                    matches!(arrived, ResponseBody::StateHeld { .. }),
                    "{arrived:?}"
                );
            }

            // Each names its own agent: the one the grid logged in under its
            // name.
            let mut agents = std::collections::HashMap::new();
            while let Ok(notice) = logins.try_recv() {
                let _previous = agents.insert(notice.last_name.clone(), notice.agent_id);
            }
            for (handle, last) in [(one, "One"), (two, "Two")] {
                let hello = answered(transport.request(handle, request(2, RequestBody::Hello))?)?;
                let ResponseBody::Hello { viewer, .. } = hello else {
                    return Err(format!("not a hello: {hello:?}").into());
                };
                assert_eq!(
                    viewer.agent_name.as_deref(),
                    Some(format!("Transport {last}").as_str())
                );
                assert_eq!(
                    viewer.agent_id.map(sl_client_bevy::AgentKey::from),
                    agents.get(last).copied(),
                    "the agent of Transport {last}"
                );
            }

            // A wait on the first for its inventory, in flight while the
            // second opens its own by a click: the second's holds, the first's
            // does not.
            let wait = RequestBody::WaitFor {
                locator: inventory_window(),
                condition: WaitCondition::Visible,
                deadline: LONG,
            };
            transport.send(one, request(3, wait.clone()))?;
            transport.send(two, request(3, wait))?;
            let clicked = answered(transport.request(
                two,
                request(
                    4,
                    RequestBody::Click {
                        locator: Locator::test_id("bottom-toolbar-button:toggle-inventory"),
                        button: sl_automation_proto::PointerButton::Left,
                        double: false,
                        deadline: sl_automation_proto::Deadline::default(),
                    },
                ),
            )?)?;
            assert!(matches!(clicked, ResponseBody::Done { .. }), "{clicked:?}");
            let shown = answered(transport.response(two, RequestId(3))?)?;
            assert!(
                matches!(&shown, ResponseBody::Satisfied { nodes } if nodes.len() == 1),
                "{shown:?}"
            );
            let hidden = answered(transport.request(
                one,
                request(
                    5,
                    RequestBody::Find {
                        locator: inventory_window(),
                    },
                ),
            )?)?;
            let ResponseBody::Found { nodes } = hidden else {
                return Err(format!("not a find: {hidden:?}").into());
            };
            assert!(
                nodes
                    .iter()
                    .all(|node| node.visibility == NodeVisibility::Hidden),
                "the first viewer's inventory stays shut: {nodes:?}"
            );

            // Then the first opens its own, and its wait holds.
            let opened = answered(transport.request(
                one,
                request(
                    6,
                    RequestBody::OpenFloater {
                        floater: "inventory".to_owned(),
                    },
                ),
            )?)?;
            assert!(matches!(opened, ResponseBody::Opened { .. }), "{opened:?}");
            let shown = answered(transport.response(one, RequestId(3))?)?;
            assert!(
                matches!(&shown, ResponseBody::Satisfied { nodes } if nodes.len() == 1),
                "{shown:?}"
            );

            log_out(&mut transport, &[one, two])?;
            drop(transport);
            drop(grid);
            Ok(())
        }
    }
}
