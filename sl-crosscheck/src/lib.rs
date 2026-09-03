//! The Firestorm cross-check runner: one fake grid, two viewers, one directory
//! of artefacts.
//!
//! A rendering question that neither viewer can answer alone — *is our sky too
//! dark, or is Firestorm's too bright; is that prim at the wrong height, or is
//! its texture at the wrong scale* — is answered by photographing the same scene
//! with both viewers and looking at the two frames. This crate is the half that
//! produces the frames: it starts [`sl_fake_grid`] on a fixed port with a named
//! scenario, launches this workspace's viewer and Firestorm against it with the
//! same capture size, captured layers, camera and time of day, waits for both,
//! and collects what they wrote into
//!
//! ```text
//! <run>/
//!   run.json                  what was asked for, and what each viewer did
//!   config/                   the credentials + grid files both viewers read
//!   sl-client/
//!     frame_000.png …         the numbered capture sequence
//!     scene.json              the structured scene dump (when the viewer writes one)
//!     harness-status.json     whether the run happened at all
//!   firestorm/
//!     …the same three
//! ```
//!
//! Comparing those artefacts is a separate step with a separate audience, and
//! keeping the collection honest is easier when it has no opinion about what the
//! frames should look like — so the runner ([`crate::process`], [`summary`]) has
//! never looked at a pixel, and the comparison ([`report`], [`frames`],
//! [`scene_diff`], [`sheet`]) is reached through its own binary,
//! `sl-crosscheck-report <run>`.
//!
//! # The three things that decide whether a run is usable
//!
//! **A viewer must be asked to quit, never killed.** A session the simulator
//! still believes is logged in makes the *next* run fail to log in, and that
//! failure looks exactly like a viewer bug — it is the single most expensive
//! mistake this runner can make, because it is paid by whoever debugs the next
//! run. So the escalation is `SIGTERM` (both viewers turn it into a logout),
//! then the logout grace, and only then `SIGKILL`. Firestorm's own
//! `--quitafter` is unusable for the same reason: it calls `forceQuit()`, which
//! sends no `LogoutRequest`.
//!
//! **The status file, not the exit code, says whether a run happened.** Neither
//! viewer's shutdown path carries a status out reliably, and a viewer that never
//! got in world still writes a full set of frames — black, and on schedule. Both
//! write `harness-status.json` before they log out; a missing file means the run
//! did not reach that point. "The viewers differ" and "the run did not happen"
//! must never be reported the same way, so [`summary`] keeps them apart in both
//! its JSON and its printed report.
//!
//! **Each viewer gets its own state directory.** Firestorm's
//! `FIRESTORM_X64_USER_DIR` and this viewer's `XDG_*` roots are pointed inside
//! the run directory, so a run cannot rewrite the operator's real settings,
//! cannot inherit last run's texture cache (which is how a fixture whose pixels
//! changed under a stable id goes unnoticed), and two runs cannot fight over the
//! same files.
//!
//! # Modules
//!
//! - [`plan`] — what both viewers are told: the scene, the camera, the capture,
//!   the timings, and the one environment block that configures both.
//! - [`files`] — the credentials and grid files the viewers read, written into
//!   the run directory so no real credential file is involved.
//! - [`launch`] — each viewer's program, arguments and environment.
//! - [`process`] — spawning one viewer and getting it to stop.
//! - [`status`] — reading back `harness-status.json` and the artefacts beside it.
//! - [`summary`] — `run.json` and the printed report.
//!
//! And the comparison, which reads a collected run and never runs a viewer:
//!
//! - [`dump`] — reading a `scene.json`, in either viewer's dialect.
//! - [`scene_diff`] — the structured comparison, which is what names a cause.
//! - [`frames`] — the image diff: a number, not a verdict.
//! - [`sheet`] and [`font`] — the contact sheet, and the labels on it.
//! - [`report`] — the three of them, written into `<run>/report`.

pub mod dump;
pub mod files;
pub mod font;
pub mod frames;
pub mod launch;
pub mod plan;
pub mod process;
pub mod report;
pub mod scene_diff;
pub mod sheet;
pub mod status;
pub mod summary;
