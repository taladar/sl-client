//! The **executor over a real session**: the whole viewer logged into the fake
//! grid, rendering into an off-screen window, driven through the automation
//! queue alone — the login waited for, a floater opened, a button in it
//! clicked and the effect waited for, a screenshot taken — as a remote or an
//! in-process transport will drive it. The executor's own teeth, and each
//! failure kind's report, are `sl-viewer-automation`'s; the world requests
//! over the fixture world are in [`crate::automation_world_aim`].

#[cfg(test)]
mod full_stack {
    use bevy::prelude::*;
    use pretty_assertions::assert_eq;
    use serde_json::json;
    use sl_automation_proto::{
        Deadline, Locator, LogStream, Probe, Request, RequestBody, RequestId, Response,
        ResponseBody, Role, StateCondition, StateObservation, ValueTest, WaitCondition,
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
}
