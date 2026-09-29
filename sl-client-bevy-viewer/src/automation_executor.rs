//! The **executor over a real session**: the whole viewer logged into the fake
//! grid, rendering into an off-screen window, driven through the automation
//! queue alone — the login waited for, a floater opened, a button in it
//! clicked and the effect waited for, a screenshot taken — as a remote or an
//! in-process transport will drive it. The executor's own teeth, and each
//! failure kind's report, are `sl-viewer-automation`'s; the world requests
//! over the fixture world are in [`crate::automation_world_aim`]. The same
//! session is driven over the automation socket too, as a client in another
//! process would drive it.

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

    /// **The acceptance**: a login, a floater, a button and its effect, all
    /// through requests — nothing but the queue touches the viewer.
    #[test]
    fn a_session_is_driven_through_requests_alone() -> Result<(), TestError> {
        let mut harness = ViewerHarness::start_in_with(
            vec![stock_fixture().into_region(RegionConfig::default())],
            HarnessOptions::in_offscreen_window(WINDOW),
        )?;

        // The login: the agent arrives in its region, and the scene settles.
        let arrived = answered(ask(
            &mut harness,
            1,
            RequestBody::WaitForState {
                condition: StateCondition::Probe {
                    probe: Probe::Agent,
                    pointer: "/region/name".to_owned(),
                    test: ValueTest::Equals(json!(HOME)),
                },
                deadline: LONG,
            },
        )?)?;
        assert!(
            matches!(arrived, ResponseBody::StateHeld { .. }),
            "{arrived:?}"
        );
        let quiet = answered(ask(
            &mut harness,
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
        if harness.capture()?.is_none() {
            tracing::warn!("no GPU adapter: skipping the executor acceptance");
            return Ok(());
        }

        // A floater, opened by its toolbar button.
        let cursor = harness
            .world()
            .resource::<sl_viewer_automation::EventLog>()
            .cursor();
        let clicked = answered(ask(
            &mut harness,
            3,
            RequestBody::Click {
                locator: Locator::test_id("bottom-toolbar-button:toggle-inventory"),
                deadline: Deadline::default(),
            },
        )?)?;
        assert!(matches!(clicked, ResponseBody::Done { .. }), "{clicked:?}");
        let shown = answered(ask(
            &mut harness,
            4,
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
            &mut harness,
            5,
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
        let path =
            std::env::temp_dir().join(format!("sl-executor-acceptance-{}.png", std::process::id()));
        let shot = answered(ask(
            &mut harness,
            6,
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
        } = shot
        else {
            return Err(format!("not a screenshot: {shot:?}").into());
        };
        assert_eq!((width, height), (WINDOW.x, WINDOW.y));
        assert_eq!(outlined.len(), 1, "the window is outlined");
        assert_eq!(written?.get(1..4), Some(b"PNG".as_slice()));

        // A button in it, and its effect: the window's close button closes it.
        let closed = answered(ask(
            &mut harness,
            7,
            RequestBody::Click {
                locator: Locator::test_id("floater-button:close").within(inventory_window()),
                deadline: Deadline::default(),
            },
        )?)?;
        assert!(matches!(closed, ResponseBody::Done { .. }), "{closed:?}");
        let hidden = answered(ask(
            &mut harness,
            8,
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
}
