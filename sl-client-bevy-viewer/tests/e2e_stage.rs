//! The end-to-end stage's acceptance (tier E): several viewers on one fake
//! grid, on both backends, driven through the automation driver alone — and
//! taken down with no stranded session and no leftover process, also when
//! the test body panics.

#[cfg(test)]
mod test {
    use core::time::Duration;
    use std::panic::AssertUnwindSafe;
    use std::path::{Path, PathBuf};
    use std::sync::{Arc, Mutex, PoisonError};

    use pretty_assertions::assert_eq;
    use sl_e2e::{Backend, BodyError, Stage, StageBuilder, StageError};

    /// How long a marker or a floater may take once both viewers are in.
    const WAIT: Duration = Duration::from_secs(60);

    /// The viewer binary cargo built for this test.
    const VIEWER: &str = env!("CARGO_BIN_EXE_sl-client-bevy-viewer");

    /// A stage for `name` with the viewers `labels`.
    fn stage(name: &str, labels: &[&str]) -> StageBuilder {
        labels.iter().fold(
            StageBuilder::new(name).viewer_binary(VIEWER),
            |builder, label| builder.viewer(*label),
        )
    }

    /// **The acceptance**: two viewers log in; Alpha opens its inventory
    /// floater by its toolbar button while Beta waits for a grid marker, which
    /// the test sends once Alpha's floater is up. Beta's own inventory stays
    /// shut: the viewers are two, not one seen twice.
    #[test]
    fn one_viewer_opens_a_floater_while_the_other_waits_for_a_marker() -> Result<(), StageError> {
        stage("floater_and_marker", &["Alpha", "Beta"]).run(async |stage: &Stage| {
            let alpha = stage.viewer("Alpha")?;
            let beta = stage.viewer("Beta")?;
            let waiting = stage.wait_marker("Beta", "inventory-open", WAIT);
            let opening = async {
                let ui = alpha.ui();
                let _clicked = ui
                    .test_id("bottom-toolbar-button:toggle-inventory")
                    .click()
                    .await?;
                let _shown = alpha
                    .expect(&ui.window("inventory"))
                    .to_be_visible()
                    .await?;
                stage.mark("Beta", "inventory-open").await?;
                Ok::<_, BodyError>(())
            };
            let (waited, opened) = tokio::join!(waiting, opening);
            opened?;
            waited?;
            assert!(
                !beta.ui().window("inventory").is_visible().await?,
                "Beta's inventory opened with Alpha's"
            );
            Ok(())
        })
    }

    /// The artifact root of a test that reads its own artifacts.
    fn scratch(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("sl-e2e-{name}-{}", std::process::id()))
    }

    /// A body that panics still has its stage taken down cleanly, on each
    /// backend: the panic comes back as it was thrown — not wrapped in a
    /// teardown failure, which is what a viewer that would not log out or a
    /// stranded session would have made of it — and no viewer process is
    /// left. Each backend separately, since a resumed panic ends the run.
    #[test]
    #[expect(
        clippy::panic,
        reason = "the test body panics on purpose: the teardown after a panic is the subject"
    )]
    fn a_panicking_body_still_logs_every_viewer_out() -> Result<(), StageError> {
        for backend in [Backend::InProcess, Backend::Process] {
            let pids = Arc::new(Mutex::new(Vec::new()));
            let builder = stage("panicking_body", &["Gamma"]).backends([backend]);
            let recorded = Arc::clone(&pids);
            let caught = std::panic::catch_unwind(AssertUnwindSafe(|| {
                builder.run(async |stage: &Stage| {
                    if let Some(pid) = stage.pid("Gamma")? {
                        recorded
                            .lock()
                            .unwrap_or_else(PoisonError::into_inner)
                            .push(pid);
                    }
                    std::panic::panic_any(format!("the body panics on the {}", stage.backend()))
                })
            }));
            let payload = match caught {
                Err(payload) => payload,
                // No GPU adapter: the stage skipped, and no body ran.
                Ok(skipped) => return skipped,
            };
            let message = payload
                .downcast_ref::<String>()
                .cloned()
                .unwrap_or_default();
            assert_eq!(
                message,
                format!("the body panics on the {backend}"),
                "the teardown failed after the panic"
            );
            let pids = pids.lock().unwrap_or_else(PoisonError::into_inner).clone();
            assert_eq!(
                pids.len(),
                usize::from(backend == Backend::Process),
                "one viewer process on the process backend, none in process"
            );
            for pid in pids {
                assert!(
                    fs_err::metadata(format!("/proc/{pid}")).is_err(),
                    "viewer process {pid} outlived its stage"
                );
            }
        }
        Ok(())
    }

    /// The teardown has teeth: a viewer process killed outright never logs
    /// out, and the stage says so — the viewer did not quit when asked, and
    /// the grid still holds its session.
    #[test]
    fn a_killed_viewer_fails_the_teardown() -> Result<(), BodyError> {
        let root = scratch("killed");
        let outcome = stage("killed_viewer", &["Delta"])
            .backends([Backend::Process])
            .artifacts(&root)
            .run(async |stage: &Stage| {
                let pid = stage.pid("Delta")?.ok_or("no process")?;
                let killed = std::process::Command::new("kill")
                    .args(["-KILL", &pid.to_string()])
                    .status()?;
                assert!(killed.success(), "kill -KILL {pid} failed");
                Ok(())
            });
        let grid_log = root.join("killed_viewer/process/grid.log");
        match outcome {
            Err(StageError::NoLogout { viewer, reason }) => {
                assert_eq!(viewer, "Delta");
                assert!(reason.contains("without logging out"), "{reason}");
                let log = fs_err::read_to_string(&grid_log)?;
                assert!(
                    log.contains("still holds a session for"),
                    "the stranded session is not in {}",
                    grid_log.display()
                );
            }
            // No GPU adapter: the stage skipped.
            Ok(()) if !Path::new(&grid_log).exists() => {}
            other => return Err(format!("the killed viewer passed the teardown: {other:?}").into()),
        }
        fs_err::remove_dir_all(&root)?;
        Ok(())
    }
}
